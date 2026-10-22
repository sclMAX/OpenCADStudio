# Local codec integration: current recipe and historical snapshots

Use [the current checkpoint guide](../../docs/checkpoints/dwg2010-codec-integration.md) to restore both verified codec fixes and build the compatible upstream application. Do not execute the Python snapshots for the current checkpoint.

| File | Purpose |
|---|---|
| `codec-local.toml.in` | Current local source override template. Replace `__LOCAL_CODEC_PATH__` with the absolute restored codec path. |
| `Cargo.lock.integration.patch` | Current temporary lock delta for app `ea4537429` and codec base `ff75fdd`: removes only acadrust's Git source. Apply only in the isolated build worktree, never to the root lockfile. |
| `prepare.py` | Historical unfinished provenance harness for app `0b414389c`; assumes an obsolete root HEAD and uses all-platform offline metadata. |
| `verify.py` | Historical seven-private-fixture stdio harness; never executed for the current build and does not prove strict-open or zero-recovery status. |

The old application is incompatible with the newer codec's plugin type registry. The guide replaces its preparation/build recipe with native-target `cargo tree`, `cargo pkgid`, focused public tests and a compatible pinned application worktree.

Private inputs, raw outputs, builds and new memory/task logs are not included in this checkpoint update.
