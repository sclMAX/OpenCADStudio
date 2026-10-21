# Unfinished local integration harness snapshot

Restore these Python files under ignored `target/assoc-arc-integration/` before using them. They are preserved work in progress, not a completed build or an executable release.

Start with [the checkpoint guide](../../docs/checkpoints/dwg2010-codec-integration.md). It explains recovery of the exact codec commit, required local worktrees/configuration and the two known preparation issues: all-platform offline metadata requests Android manifests, and the snapshot assumes the main checkout still has the pre-checkpoint HEAD.

- `codec-local.toml.in`: replace `__LOCAL_CODEC_PATH__` with this machine's absolute codec checkout path.
- `prepare.py`: protected-state/provenance and lock-delta checks; the latest attempt failed before provenance closure.
- `verify.py`: intended post-build stdio checks; not executed against a new application binary yet.
- `Cargo.lock.integration.patch`: records the sole temporary lock delta observed in the isolated worktree. Do not apply it to the public root lockfile or treat it as a permanent dependency update.

Private inputs, raw outputs and builds are intentionally not included.
