//! Custom-grip drag lifecycle (API v11/v12): the plugin-owned twin of the
//! built-in grip drag in `update/viewport.rs`.
//!
//! Same ride as built-ins — Esc cancels, mouse-up settles, another command
//! starting mid-drag cancels, tab switches cancel — but the host applies
//! NOTHING semantically. Delivery honors the binding invariant of the v11.1
//! hardening (a plugin must never be able to freeze the host): every
//! out-of-process phase is issued WITHOUT waiting and collected at pump
//! ticks, nested COMMAND frames are refused in the delivery context (they
//! would need the message loop the blocked UI thread cannot pump), and a
//! settle that outlives its deadline is struck: the first strike cancels
//! the drag while keeping the plugin's connection, grips and session (a
//! starved pump must not amputate a healthy plugin); the third missed
//! delivery inside the strike window tears the connection down, ends the
//! drag as a cancel, names the plugin and queues a grip re-declaration.
//!
//! v11.2 live geometry: a move/begin answer may carry `live_entities` —
//! the plugin-directed visual phase (the plugin computes exactly what the
//! settle will commit). The host swaps those entities live through the same
//! visual-only dynamic semantics its own grip drags show: elements visibly
//! move, ZERO history steps mid-drag, everything is restored byte-identically
//! before the settle lands, and cancel discards it untouched by any undo
//! step. A preview-only response behaves exactly as before.

use std::time::Duration;

use glam::DVec3;
use iced::{Point, Rectangle, Task};

use crate::app::{Message, OpenCADStudio};
use crate::plugin::grip_points::{self, ActivePluginGrip, GripCandidate};

/// Poll slice for an outstanding delivery (kept tiny: the pump reschedules
/// itself, so the app loop never waits on plugin latency).
const MOVE_POLL_SLICE: Duration = Duration::from_millis(8);
/// How long the settle waits for outstanding moves before issuing (the
/// CursorMove floor): a plugin slower than this is treated as unresponsive
/// for the drag, and the settle is still delivered.
const MOVE_DRAIN_BUDGET: Duration = Duration::from_millis(500);
/// Self-rescheduling pump cadence while a drag is live.
const PUMP_CADENCE: Duration = Duration::from_millis(10);
/// Tighter cadence while a settle is outstanding: the plugin's writes
/// stream in as nested requests served by the pump, so the finalize should
/// land promptly after the callback returns.
const SETTLING_CADENCE: Duration = Duration::from_millis(2);
/// The watchdog floor for the settle phase (matches the transport's per-kind
/// floor): beyond this the delivery is struck — see
/// [`strike_plugin_grip_settle`].
pub(crate) const SETTLE_WATCHDOG: Duration = Duration::from_secs(10);
/// The strike window: three missed deliveries inside it mean the plugin is
/// genuinely wedged under a live pump, and the connection is torn down.
/// A starved UI thread or one slow settle never accumulates that far
/// because one strike only cancels the drag.
const SETTLE_STRIKE_WINDOW: Duration = Duration::from_secs(600);
/// Missed deliveries allowed before the teardown applies.
const SETTLE_STRIKES_UNTIL_TEARDOWN: usize = 3;

impl OpenCADStudio {
    /// Custom grips of tab `i` that are visible in the current view, with
    /// their tile-relative screen positions. Hover, press and render share
    /// this one query — it goes through the per-tab uniform grid with
    /// viewport culling, never a linear scan of every declared grip.
    pub(in crate::app) fn plugin_grips_in_view(&self, i: usize) -> Vec<(GripCandidate, Point)> {
        if self.tabs.len() <= i || self.tabs[i].plugin_grips.is_empty() {
            return Vec::new();
        }
        let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
        let edit_frame = self.tabs[i].scene.viewport_edit_frame((vw, vh));
        let tile = match &edit_frame {
            Some((_, full)) => *full,
            None => self.tabs[i].scene.active_model_tile_bounds(vw, vh),
        };
        let bounds = Rectangle {
            width: tile.width,
            height: tile.height,
            ..Rectangle::default()
        };
        let cam = edit_frame.map_or_else(
            || self.tabs[i].scene.camera.borrow().clone(),
            |(cam, _)| cam,
        );
        // World rectangle of the view: unproject the tile corners on the
        // camera's target plane (the same plane cursor picks fall back to).
        let corners = [
            Point::new(0.0, 0.0),
            Point::new(bounds.width, 0.0),
            Point::new(0.0, bounds.height),
            Point::new(bounds.width, bounds.height),
        ];
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        let mut any = false;
        for corner in corners {
            let world = cam.pick_on_target_plane(corner, bounds);
            min[0] = min[0].min(world.x);
            min[1] = min[1].min(world.y);
            max[0] = max[0].max(world.x);
            max[1] = max[1].max(world.y);
            any = true;
        }
        if !any {
            return Vec::new();
        }
        let visible = grip_points::visibility(&self.disabled_plugins);
        let candidates = self.tabs[i].plugin_grips.query_rect(min, max, &visible);
        let mut out = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            if let Some(screen) = cam.project(candidate.point, bounds) {
                out.push((candidate, iced::Point::new(screen.x, screen.y)));
            }
        }
        out
    }

    /// Nearest visible plugin grip within the grip aperture of the
    /// tile-relative cursor — the custom-grip twin of `find_hit_grip`,
    /// feeding both hover and press. Tested before any wire pick, so a
    /// grip coincident with click-through (reference-only) geometry stays
    /// fully hittable and wins the hit.
    pub(in crate::app) fn hit_plugin_grip(&self, i: usize, cursor: Point) -> Option<GripCandidate> {
        let candidates = self.plugin_grips_in_view(i);
        let mut best_dist = crate::scene::pick::grip::GRIP_THRESHOLD_PX;
        let mut best = None;
        for (candidate, screen) in candidates {
            let dx = screen.x - cursor.x;
            let dy = screen.y - cursor.y;
            let d = (dx * dx + dy * dy).sqrt();
            if d < best_dist {
                best_dist = d;
                best = Some(candidate);
            }
        }
        best
    }

    /// Press on a plugin-declared grip: engage the drag and deliver the begin
    /// event without waiting (a missing owner — process died between render
    /// and press — is a no-op). Returns immediately in every case: the UI
    /// thread never blocks on plugin answers.
    pub(in crate::app) fn begin_plugin_grip_drag(
        &mut self,
        i: usize,
        candidate: &GripCandidate,
    ) -> Task<Message> {
        if self.plugin_grip_deliveries.is_settling() {
            // A settle is still streaming; refuse new engagements until it
            // finalizes (the write window discipline).
            return Task::none();
        }
        self.plugin_grip_deliveries = Default::default();
        self.plugin_grip_live_originals = Vec::new();
        let active = ActivePluginGrip {
            tab: i,
            plugin: candidate.plugin.clone(),
            grip_id: candidate.grip_id,
            anchor: candidate.anchor,
            origin_world: candidate.point,
            axis: candidate.axis,
            last_world: candidate.point,
            moved: false,
        };
        self.tabs[i].active_plugin_grip = Some(active);
        let event = self.event_for_active(
            i,
            ocs_plugin_api::host::CustomGripPhase::Begin,
            candidate.point,
        );
        if let Some(event) = event {
            self.issue_phase(i, ocs_plugin_api::host::CustomGripPhase::Begin, event);
        }
        self.pump_plugin_grips()
    }

    /// Park the newest constrained cursor point for the active custom drag
    /// (the viewport move branch computes the point; the channel drops
    /// superseded ones) and drive delivery. Returns immediately.
    pub(in crate::app) fn move_plugin_grip_drag(
        &mut self,
        i: usize,
        point: DVec3,
    ) -> Task<Message> {
        if let Some(active) = self.tabs[i].active_plugin_grip.as_mut() {
            active.last_world = point;
            active.moved = true;
        }
        self.plugin_grip_deliveries.park_move(point);
        self.pump_plugin_grips()
    }

    /// The pump: process landed answers in order (preview + live geometry),
    /// poll the next outstanding delivery (bounded), send the newest parked
    /// move when the channel is free, run the settle watchdog, and reschedule
    /// itself while the drag lives. NEVER blocks the app loop on plugin
    /// latency.
    pub(in crate::app) fn pump_plugin_grips(&mut self) -> Task<Message> {
        let Some(active) = self.active_plugin_grip() else {
            self.plugin_grip_deliveries = Default::default();
            return Task::none();
        };
        let tab = active.tab;
        let plugin = active.plugin.clone();

        // 1. Process landed answers in send order. Settle landings finalize
        //    the drag; late begin/move answers after the drag ended apply
        //    nothing.
        let mut finalize_pendings = Vec::new();
        while let Some((phase, response)) = self.plugin_grip_deliveries.pop_landed() {
            match phase {
                ocs_plugin_api::host::CustomGripPhase::Begin
                | ocs_plugin_api::host::CustomGripPhase::Move => {
                    if self.tabs[tab].active_plugin_grip.is_some() {
                        self.apply_plugin_grip_move_response(tab, phase, response);
                    } else if !response.live_entities.is_empty() {
                        // The drag already ended; a live-geometry answer that
                        // arrives late must not mutate anything.
                        self.restore_plugin_grip_live_geometry(tab);
                    }
                }
                ocs_plugin_api::host::CustomGripPhase::Settle => {
                    self.plugin_grip_deliveries.settle_landed();
                    if self.tabs[tab].active_plugin_grip.is_some() {
                        finalize_pendings.push(tab);
                    }
                }
                ocs_plugin_api::host::CustomGripPhase::Cancel => {
                    // The drag ends locally at cancel time; the answer is a
                    // courtesy and applies nothing.
                }
            }
        }
        for tab in finalize_pendings {
            self.finalize_plugin_grip_settle(tab);
        }

        // 2. Poll the next outstanding out-of-process answer (bounded slice;
        //    nested plugin requests — including the settle's writes — are
        //    served inline here on UI ticks; command frames are refused).
        if let Some((id, response)) =
            grip_points::poll_grip_event(self, tab, &plugin, MOVE_POLL_SLICE)
        {
            if let Some(phase) = self.plugin_grip_deliveries.resolve(id) {
                self.plugin_grip_deliveries.push_landed(phase, response);
                // Recurse once to keep order: landed answers first.
                return self.pump_plugin_grips();
            }
        }

        // 3. Coalesced move send: keep at most one in flight — then loop so
        //    the fresh answer (in-process deliveries land synchronously) is
        //    processed in THIS pump pass, never left to lag behind a
        //    later phase (e.g. re-applying stale live geometry after the
        //    settle already restored it).
        if let Some(point) = self.plugin_grip_deliveries.take_sendable_move() {
            let event =
                self.event_for_active(tab, ocs_plugin_api::host::CustomGripPhase::Move, point);
            if let Some(event) = event {
                self.issue_phase(tab, ocs_plugin_api::host::CustomGripPhase::Move, event);
                if self.plugin_grip_deliveries.has_landed() {
                    // Process the fresh landing now (order-preserving).
                    return self.pump_plugin_grips();
                }
            }
        }

        // 4. The settle watchdog: a delivery that outlives its deadline is
        //    struck. The first strike ends the drag as a cancel and keeps the
        //    plugin's connection, grips and session alive (a starved pump
        //    must not amputate a healthy plugin); the third missed delivery
        //    inside the window tears the connection down. The UI loop never
        //    waits.
        let now = std::time::Instant::now();
        if self.plugin_grip_deliveries.settle_timed_out(now) {
            self.strike_plugin_grip_settle(tab);
            return Task::none();
        }

        // 5. Reschedule while the drag (or its finalization) is alive.
        self.schedule_plugin_grip_pump()
    }

    /// Settle (mouse release, or Enter): restore the live-geometry phase
    /// byte-identically FIRST (so the plugin's `push_undo` captures the
    /// pre-drag state, never a mid-drag visual), drain outstanding moves,
    /// then issue the settle — WITHOUT waiting. The plugin's writes stream
    /// back as nested requests served at pump ticks; its answer finalize
    /// closes any plugin-opened pending history into the drag's exactly one
    /// plugin-labeled undo step. A plugin that never answers is abandoned
    /// by the watchdog. Returns immediately in every case.
    pub(in crate::app) fn settle_plugin_grip_drag(&mut self) -> Task<Message> {
        let Some(active) = self.active_plugin_grip() else {
            return Task::none();
        };
        let tab = active.tab;
        let final_point = active.last_world;
        if self.plugin_grip_deliveries.is_settling() {
            // A second settle while settling: ignore (Enter after mouse-up).
            return Task::none();
        }
        // Drain outstanding moves first (bounded, never coalesced away):
        // the settle would otherwise race the move stream.
        self.drain_plugin_grip_moves(tab);
        // Restore the visual-only phase byte-identically BEFORE the settle:
        // the plugin's push_undo must see the pre-drag document.
        self.restore_plugin_grip_live_geometry(tab);
        let event = self.event_for_active(
            tab,
            ocs_plugin_api::host::CustomGripPhase::Settle,
            final_point,
        );
        if let Some(event) = event {
            self.issue_phase(tab, ocs_plugin_api::host::CustomGripPhase::Settle, event);
            self.plugin_grip_deliveries.begin_settle_watch(
                self.plugin_grip_settle_watchdog,
                std::time::Instant::now(),
            );
        } else {
            // The owner vanished mid-drag: end as a cancel.
            let _ = self.cancel_plugin_grip_drag();
        }
        self.pump_plugin_grips()
    }

    /// Cancel (Esc, tab switch, another command starting, process death):
    /// restore the visual-only phase, issue the cancel phase so the plugin
    /// drops preview state, and write nothing. Ends the drag immediately —
    /// the cancel answer is a courtesy and applies nothing. Zero undo
    /// steps, byte-identical drawing.
    pub(in crate::app) fn cancel_plugin_grip_drag(&mut self) -> Task<Message> {
        let Some(active) = self.active_plugin_grip() else {
            return Task::none();
        };
        let tab = active.tab;
        let plugin = active.plugin.clone();
        let last_point = active.last_world;
        self.restore_plugin_grip_live_geometry(tab);
        let event = self.event_for_active(
            tab,
            ocs_plugin_api::host::CustomGripPhase::Cancel,
            last_point,
        );
        if let Some(event) = event {
            self.issue_phase(tab, ocs_plugin_api::host::CustomGripPhase::Cancel, event);
        }
        // A well-behaved plugin wrote nothing during the drag; a badly
        // behaved one may have streamed writes with its cancel — close any
        // scope it opened so the cancel cannot leave history dangling.
        self.finish_pending_history(tab);
        self.end_plugin_grip_drag(tab);
        let _ = plugin;
        Task::none()
    }

    /// The idle state gate for the settle path: the drag is still settling
    /// (the plugin's writes are streaming through the pump).
    pub(in crate::app) fn is_plugin_grip_settling(&self) -> bool {
        self.plugin_grip_deliveries.is_settling()
    }

    /// Drive pump ticks until the outstanding settle finalizes (or the
    /// watchdog abandons it). Bounded by the settle floor plus a small
    /// margin — the automation/MCP route may wait for its outcome, the UI
    /// route never calls this. Returns `true` when the settle finalized.
    pub(in crate::app) fn drive_plugin_grip_settle_to_completion(
        &mut self,
        budget: Duration,
    ) -> bool {
        let deadline = std::time::Instant::now() + budget;
        loop {
            let _ = self.pump_plugin_grips();
            if !self.is_plugin_grip_settling() {
                return self.active_plugin_grip().is_none();
            }
            if std::time::Instant::now() >= deadline {
                // The watchdog has not fired yet but the automation budget
                // is spent: let the watchdog own the abandonment.
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// The active custom drag (one at a time, app-wide).
    pub(in crate::app) fn active_plugin_grip(&self) -> Option<&ActivePluginGrip> {
        for tab in &self.tabs {
            if let Some(active) = &tab.active_plugin_grip {
                return Some(active);
            }
        }
        None
    }

    // ── internals ────────────────────────────────────────────────────────

    /// Build the event for the active drag's current point, or `None` when
    /// no drag is active.
    fn event_for_active(
        &self,
        tab: usize,
        phase: ocs_plugin_api::host::CustomGripPhase,
        point: DVec3,
    ) -> Option<ocs_plugin_api::host::CustomGripEvent> {
        self.tabs[tab]
            .active_plugin_grip
            .as_ref()
            .map(|active| active.event(phase, point))
    }

    /// Issue one delivery phase: in-process sinks dispatch synchronously and
    /// land immediately; out-of-process endpoints send WITHOUT waiting (the
    /// pump collects at ticks; nested command frames are refused in that
    /// context; the watchdog guards the settle).
    fn issue_phase(
        &mut self,
        tab: usize,
        phase: ocs_plugin_api::host::CustomGripPhase,
        event: ocs_plugin_api::host::CustomGripEvent,
    ) {
        let Some(active) = self.tabs[tab].active_plugin_grip.as_ref() else {
            return;
        };
        let plugin = active.plugin.clone();
        if grip_points::sink_installed(&plugin) {
            let response = grip_points::dispatch_grip_event(self, tab, &plugin, event.clone());
            if let Some(response) = response {
                self.plugin_grip_deliveries.push_landed(phase, response);
            } else {
                crate::plugin::push_guard_error(format!(
                    "custom grip event delivery to {plugin} produced no answer"
                ));
            }
            return;
        }
        match grip_points::send_grip_event(self, tab, &plugin, event) {
            Some(id) => self.plugin_grip_deliveries.track(id, phase),
            None => {
                // No live endpoint (death raced the drag): end the drag for
                // terminal phases; moves just stop being delivered.
                if matches!(
                    phase,
                    ocs_plugin_api::host::CustomGripPhase::Settle
                        | ocs_plugin_api::host::CustomGripPhase::Begin
                ) {
                    self.end_plugin_grip_drag(tab);
                }
            }
        }
    }

    /// Drain any outstanding move answer before settle/cancel so those
    /// phases never meet a stale frame (bounded; in-flight moves resolve
    /// their phase when they land).
    fn drain_plugin_grip_moves(&mut self, tab: usize) {
        if !self.plugin_grip_deliveries.is_move_in_flight() {
            return;
        }
        // In-process delivery is synchronous — nothing to wait for.
        let plugin = self.tabs[tab]
            .active_plugin_grip
            .as_ref()
            .map(|active| active.plugin.clone());
        let Some(plugin) = plugin else {
            return;
        };
        if grip_points::sink_installed(&plugin) {
            return;
        }
        let deadline = std::time::Instant::now() + MOVE_DRAIN_BUDGET;
        while self.plugin_grip_deliveries.is_move_in_flight() {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match grip_points::poll_grip_event(self, tab, &plugin, remaining.min(MOVE_POLL_SLICE)) {
                Some((id, response)) => {
                    if let Some(phase) = self.plugin_grip_deliveries.resolve(id) {
                        self.plugin_grip_deliveries.push_landed(phase, response);
                    }
                }
                None => {
                    if !grip_points::grip_move_in_flight(&plugin) {
                        break;
                    }
                }
            }
        }
        // Process what landed (previews keep flowing; no mutation of drag
        // state).
        while let Some((phase, response)) = self.plugin_grip_deliveries.pop_landed() {
            if matches!(
                phase,
                ocs_plugin_api::host::CustomGripPhase::Begin
                    | ocs_plugin_api::host::CustomGripPhase::Move
            ) {
                self.apply_plugin_grip_move_response(tab, phase, response);
            }
        }
    }

    /// Apply a begin/move answer: the plugin's preview set REPLACES its
    /// previous one, and its live-geometry entities swap in through the
    /// visual-only phase (originals stashed once; zero history steps).
    fn apply_plugin_grip_move_response(
        &mut self,
        tab: usize,
        phase: ocs_plugin_api::host::CustomGripPhase,
        response: ocs_plugin_api::host::CustomGripResponse,
    ) {
        let _ = phase;
        let wires = crate::app::plugin_host::convert_preview_wires(response.preview.clone());
        self.tabs[tab].scene.set_preview_wires(wires);
        self.apply_plugin_grip_live_geometry(tab, &response.live_entities);
        self.tabs[tab].dirty = true;
    }

    /// Swap the plugin's live-geometry entities in (visual-only: originals
    /// stashed once per handle, no history, no notifications). Capped; a
    /// too-large payload is refused with an error line naming the constraint.
    fn apply_plugin_grip_live_geometry(
        &mut self,
        tab: usize,
        live_entities: &[(codec::Handle, codec::EntityType)],
    ) {
        if live_entities.is_empty() {
            return;
        }
        if live_entities.len() > ocs_plugin_api::host::MAX_PLUGIN_LIVE_ENTITIES {
            crate::plugin::push_guard_error(format!(
                "custom-grip live geometry refused: {} entities exceeds the per-response cap of {}",
                live_entities.len(),
                ocs_plugin_api::host::MAX_PLUGIN_LIVE_ENTITIES
            ));
            return;
        }
        for (handle, live_entity) in live_entities {
            let Some(current) = self.tabs[tab].scene.document.get_entity(*handle) else {
                crate::plugin::push_guard_error(format!(
                    "custom-grip live geometry names unknown entity {handle:?}"
                ));
                continue;
            };
            // Live geometry moves an existing editable entity; it never swaps
            // in another handle or kind, nor edits a locked layer.
            if live_entity.common().handle != *handle
                || std::mem::discriminant(live_entity) != std::mem::discriminant(current)
                || self.tabs[tab].scene.is_layer_locked(*handle)
            {
                crate::plugin::push_guard_error(format!(
                    "custom-grip live geometry for {handle:?} refused: it must keep the \
                     entity's handle and type on an unlocked layer"
                ));
                continue;
            }
            if !self
                .plugin_grip_live_originals
                .iter()
                .any(|(stashed, _)| stashed == handle)
            {
                self.plugin_grip_live_originals
                    .push((*handle, current.clone()));
            }
            if let Some(entity) = self.tabs[tab].scene.document.get_entity_mut(*handle) {
                *entity = live_entity.clone();
            }
            self.tabs[tab]
                .scene
                .bump_entities_restyled(&[(*handle, crate::scene::ChangeKind::Modified)]);
        }
    }

    /// Restore every live-geometry original byte-identically (the drag's
    /// visual state is discarded whether settling or cancelling; the
    /// restore happens BEFORE the settle so the plugin's push_undo sees the
    /// pre-drag document).
    fn restore_plugin_grip_live_geometry(&mut self, tab: usize) {
        let originals = std::mem::take(&mut self.plugin_grip_live_originals);
        for (handle, original) in originals {
            if let Some(entity) = self.tabs[tab].scene.document.get_entity_mut(handle) {
                *entity = original;
            }
            self.tabs[tab]
                .scene
                .bump_entities_restyled(&[(handle, crate::scene::ChangeKind::Modified)]);
        }
        self.tabs[tab].dirty = true;
    }

    /// The settle landed: close the plugin-opened pending history (exactly
    /// one plugin-labeled undo step when it wrote, zero when it did not),
    /// clear the drag, refresh. A landed settle is recovery: the plugin's
    /// strike count resets, so one missed deadline alone never accumulates
    /// toward the teardown.
    fn finalize_plugin_grip_settle(&mut self, tab: usize) {
        if let Some(active) = self.tabs[tab].active_plugin_grip.as_ref() {
            self.plugin_grip_settle_strikes.remove(&active.plugin);
        }
        self.finish_pending_history(tab);
        self.end_plugin_grip_drag(tab);
        self.refresh_properties();
    }

    /// The watchdog fired: the plugin never answered its settle within the
    /// settle floor. Strike semantics keep a healthy plugin healthy: the
    /// first strike ends the drag as a cancel (the live-geometry restore and
    /// the pending history closed — no dangling undo scope) while the
    /// plugin's connection, declared grips and session all stay; only the
    /// third missed delivery inside the strike window tears the connection
    /// down, because a plugin that misses three deadlines under a live pump
    /// is genuinely wedged. A struck delivery's late answer is drained
    /// gracefully by the pump's in-order poll: the nested requests it
    /// carries (the plugin's own settle transaction) are served by the host
    /// session exactly as they arrive, and the answer can never finalize a
    /// drag — its request id is no longer tracked. After a teardown the
    /// plugin is queued for a grip re-declaration the reap tick re-dispatches
    /// once it is visible again. Every strike names the plugin, the budget
    /// and the strike count in the error queue.
    fn strike_plugin_grip_settle(&mut self, tab: usize) {
        let plugin = self.tabs[tab]
            .active_plugin_grip
            .as_ref()
            .map(|active| active.plugin.clone())
            .unwrap_or_default();
        let now = std::time::Instant::now();
        let budget = self.plugin_grip_settle_watchdog;
        let strikes = {
            let ledger = self
                .plugin_grip_settle_strikes
                .entry(plugin.clone())
                .or_default();
            ledger.retain(|at| now.saturating_duration_since(*at) < SETTLE_STRIKE_WINDOW);
            ledger.push(now);
            ledger.len()
        };
        if strikes >= SETTLE_STRIKES_UNTIL_TEARDOWN {
            grip_points::abandon_grip_delivery(&plugin);
            crate::plugin::push_guard_error(format!(
                "custom-grip settle delivery to {plugin:?} exceeded its {budget:?} deadline \
                 (strike {strikes}/{}): the delivery was abandoned, the connection torn down \
                 and the drag cancelled — a live pump is starved or the plugin is wedged",
                SETTLE_STRIKES_UNTIL_TEARDOWN
            ));
            self.plugin_grip_settle_strikes.remove(&plugin);
            self.pending_plugin_grips_rearm.insert(plugin);
        } else {
            crate::plugin::push_guard_error(format!(
                "custom-grip settle delivery to {plugin:?} exceeded its {budget:?} deadline \
                 (strike {strikes}/{}): the connection is kept and the drag cancelled — a live \
                 pump is starved or the plugin is wedged",
                SETTLE_STRIKES_UNTIL_TEARDOWN
            ));
        }
        self.restore_plugin_grip_live_geometry(tab);
        self.finish_pending_history(tab);
        self.end_plugin_grip_drag(tab);
    }

    /// Shared end-of-drag cleanup: state off, preview off, channel idle,
    /// live stash gone, tab marked dirty.
    fn end_plugin_grip_drag(&mut self, tab: usize) {
        self.tabs[tab].active_plugin_grip = None;
        self.plugin_grip_deliveries = Default::default();
        self.plugin_grip_live_originals = Vec::new();
        self.tabs[tab].scene.set_preview_wires(Vec::new());
        self.tabs[tab].dirty = true;
    }

    fn schedule_plugin_grip_pump(&mut self) -> Task<Message> {
        let drag_alive = self.active_plugin_grip().is_some();
        let settling = self.is_plugin_grip_settling();
        if drag_alive || settling || self.plugin_grip_deliveries.is_busy() {
            let cadence = if settling {
                SETTLING_CADENCE
            } else {
                PUMP_CADENCE
            };
            Task::perform(grip_pump_delay(cadence), |()| Message::PluginGripPump)
        } else {
            Task::none()
        }
    }

    /// Reap the grip sets of plugins that died or were unloaded, and cancel
    /// the drag/hover if the dying plugin owned it. Called from the periodic
    /// plugin drain tick; nothing else changes ("removes the grip and
    /// nothing else"). Liveness is the same visibility the render/hit paths
    /// apply (loaded, alive, not disabled).
    pub(in crate::app) fn reap_plugin_grips(&mut self) {
        let (dying_drag, dead_hover) = {
            let visible = grip_points::visibility(&self.disabled_plugins);
            for i in 0..self.tabs.len() {
                self.tabs[i]
                    .plugin_grips
                    .retain_alive(|plugin| visible(plugin));
            }
            let dying_drag = self
                .active_plugin_grip()
                .map(|active| !visible(&active.plugin))
                .unwrap_or(false);
            let dead_hover = self
                .plugin_grip_hover
                .as_ref()
                .is_some_and(|hover| !visible(&hover.plugin));
            (dying_drag, dead_hover)
        };
        if dying_drag {
            let _ = self.cancel_plugin_grip_drag();
        }
        if dead_hover {
            self.plugin_grip_hover = None;
        }
        // The visibility predicate borrows the disabled set; a copy lets the
        // re-arm take `&mut self` after the borrow-heavy reap work above.
        let disabled = self.disabled_plugins.clone();
        let visible = grip_points::visibility(&disabled);
        self.rearm_struck_plugin_grips(&visible);
    }

    /// The strike-teardown re-arm: a plugin whose connection the watchdog
    /// tore down keeps its grip DECLARATIONS only as long as it is visible
    /// (loaded, alive, not disabled). While a queued re-arm plugin is not
    /// visible it stays queued and drops out when it is gone for good; once
    /// it is visible again, the `SelectionChanged` fingerprint dedup is
    /// cleared and the broadcast re-issues even for an unchanged selection,
    /// so the plugin re-declares its grips without human intervention
    /// (a mouse-only session never issues a dispatch of its own).
    fn rearm_struck_plugin_grips(&mut self, visible: &impl Fn(&str) -> bool) {
        if self.pending_plugin_grips_rearm.is_empty() {
            return;
        }
        let back: Vec<String> = self
            .pending_plugin_grips_rearm
            .iter()
            .filter(|plugin| visible(plugin))
            .cloned()
            .collect();
        if back.is_empty() {
            return;
        }
        for plugin in back {
            self.pending_plugin_grips_rearm.remove(&plugin);
        }
        self.last_plugin_selection = None;
        self.notify_plugins_selection_changed();
    }
}

/// A future that resolves after the pump cadence (the same thread+oneshot
/// shape the dock hover delay uses — `iced::time::sleep` is not available in
/// this iced version).
fn grip_pump_delay(cadence: Duration) -> impl std::future::Future<Output = ()> {
    #[cfg(not(target_arch = "wasm32"))]
    let rx = {
        let (tx, rx) = iced::futures::channel::oneshot::channel::<()>();
        std::thread::spawn(move || {
            std::thread::sleep(cadence);
            let _ = tx.send(());
        });
        rx
    };
    #[cfg(target_arch = "wasm32")]
    let rx = {
        // No custom grips exist on wasm (no plugin processes); a resolved
        // future keeps the type uniform.
        let (_tx, rx) = iced::futures::channel::oneshot::channel::<()>();
        rx
    };
    async move {
        let _ = rx.await;
    }
}
