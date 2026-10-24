//! OpenCADStudio selection overlay frame benchmark suite.
//!
//! Times one static overlay layer two ways over the same fixture: rebuilding
//! the constraint-glyph pills into a fresh frame every frame (the pre-cache
//! path) versus replaying them through `CachedOverlayLayer` with an unchanged
//! key (the cached path). Both arms report the same pill count, so any
//! difference between the arms is cache overhead only.

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use iced::widget::canvas;
use iced::{Point, Rectangle, Size, Theme};
use OpenCADStudio::scene::parametric_constraints::GlyphEntry;
use OpenCADStudio::ui::overlay::{
    count_constraint_glyph_pills, draw_constraint_glyph_layer, CachedOverlayLayer, GlyphLayerKey,
};

const GLYPH_COUNT: usize = 50;
const FRAMES_PER_RUN: usize = 2_000;
const RUNS: usize = 7;
const WARMUP_FRAMES: usize = 200;

const LABELS: [&str; 10] = ["⊥", "=", "H", "V", "F", "T", "◎", "G²", "∥", "↔ 25.00"];

fn fixture() -> (Arc<[GlyphEntry]>, Arc<[bool]>) {
    let glyphs: Arc<[GlyphEntry]> = (0..GLYPH_COUNT)
        .map(|i| {
            let col = i % 10;
            let row = i / 10;
            let anchor = Point::new(120.0 + col as f32 * 140.0, 90.0 + row as f32 * 120.0);
            GlyphEntry {
                id: i as u32,
                anchor,
                outward: [0.0, -1.0],
                label: Arc::from(LABELS[i % LABELS.len()]),
                conflicting: i % 7 == 0,
                hover: Arc::from(vec![
                    Point::new(anchor.x - 40.0, anchor.y),
                    Point::new(anchor.x + 40.0, anchor.y),
                ]),
                size: Size::new(28.0, 22.0),
                tangent_dx: 6.0,
                top_left: Point::new(anchor.x - 14.0, anchor.y - 11.0),
            }
        })
        .collect();
    let selected: Arc<[bool]> = (0..GLYPH_COUNT).map(|i| i % 5 == 0).collect();
    (glyphs, selected)
}

fn main() {
    let renderer = iced_test::futures::futures::executor::block_on(
        <iced::Renderer as iced_test::core::renderer::Headless>::new(
            iced_test::core::renderer::Settings::default(),
            None,
        ),
    )
    .expect("headless renderer");

    let (glyphs, selected) = fixture();
    let theme = Theme::Dark;
    let palette = *theme.palette();
    let frame_bounds = Rectangle::with_size(Size::new(1600.0, 900.0));

    // Both arms must agree on the sink before any timing: the cached arm
    // replays geometry instead of drawing it, so it counts from the fixture.
    let probe = {
        let mut frame = canvas::Frame::new(&renderer, frame_bounds.size());
        draw_constraint_glyph_layer(&mut frame, &glyphs, &selected, &palette)
    };
    let pills = count_constraint_glyph_pills(&glyphs);
    assert_eq!(
        probe, pills,
        "both timing arms must report the same pill count"
    );

    let rebuild = |sink: &mut usize| {
        let mut frame = canvas::Frame::new(&renderer, frame_bounds.size());
        *sink += draw_constraint_glyph_layer(&mut frame, &glyphs, &selected, &palette);
        black_box(frame.into_geometry());
    };

    let cache: CachedOverlayLayer<GlyphLayerKey> = CachedOverlayLayer::default();
    let cached = |sink: &mut usize| {
        let key = GlyphLayerKey::new(frame_bounds, &glyphs, &selected, palette);
        let geometry = cache.draw(&renderer, frame_bounds, &key, |frame| {
            draw_constraint_glyph_layer(frame, &glyphs, &selected, &palette);
        });
        *sink += count_constraint_glyph_pills(&glyphs);
        black_box(geometry);
    };

    // Prime the cache (its first call misses and fills it) and warm both
    // paths so neither arm pays one-time setup costs inside a timed run.
    let mut warm_sink = 0usize;
    for _ in 0..WARMUP_FRAMES {
        rebuild(&mut warm_sink);
        cached(&mut warm_sink);
    }

    let mut rebuild_sink = 0usize;
    let mut samples_rebuild = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        for _ in 0..FRAMES_PER_RUN {
            rebuild(&mut rebuild_sink);
        }
        samples_rebuild.push(start.elapsed().as_secs_f64() * 1_000_000.0 / FRAMES_PER_RUN as f64);
    }

    let mut cached_sink = 0usize;
    let mut samples_cached = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        for _ in 0..FRAMES_PER_RUN {
            cached(&mut cached_sink);
        }
        samples_cached.push(start.elapsed().as_secs_f64() * 1_000_000.0 / FRAMES_PER_RUN as f64);
    }

    samples_rebuild.sort_by(f64::total_cmp);
    samples_cached.sort_by(f64::total_cmp);

    let measured_frames = RUNS * FRAMES_PER_RUN;
    assert_eq!(
        rebuild_sink, cached_sink,
        "both arms must accumulate the same sink over the same frame count"
    );

    println!(
        "selection overlay glyph layer: {GLYPH_COUNT} glyphs, {pills} pills/frame, \
         {measured_frames} frames/arm"
    );
    println!(
        "rebuild every frame: median {:.1} µs/frame; samples {:?}",
        samples_rebuild[RUNS / 2],
        samples_rebuild
    );
    println!(
        "cached layer, stable key: median {:.1} µs/frame; samples {:?}",
        samples_cached[RUNS / 2],
        samples_cached
    );
    println!(
        "sinks identical: rebuild {rebuild_sink} == cached {cached_sink} \
         ({} frames/arm incl. warmup)",
        WARMUP_FRAMES + measured_frames
    );
}
