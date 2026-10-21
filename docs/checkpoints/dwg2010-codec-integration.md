# Resume the DWG 2010 codec integration

**Checkpoint only: the codec fix is verified; the new application has not been built.** Work stopped at the user's request. Resume from branch `checkpoint/dwg2010-codec-integration` in `sclMAX/OpenCADStudio`. No installed application was replaced.

## What is preserved

| Component | State |
|---|---|
| Application source base | `0b414389cb1a954ed22ddb95c44727fe0a5bd075` |
| Codec fix | `b058a27d36b5bfa83b9ba16e72cae3a8185fb935`, branch `fix/dwg2010-associative-arc` |
| Codec transport | `patches/opencadcodec/dwg2010-associative-arc.bundle` preserves the exact commit; adjacent `.patch` is readable source |
| Integration scripts | `scripts/local_codec_integration/` contains the current, unfinished harness source snapshot |
| Application manifests | Unchanged; all five codec declarations still use upstream revision `14fdae5` |
| Temporary integration lock delta | Saved as `scripts/local_codec_integration/Cargo.lock.integration.patch`; not applied to the public source lockfile |
| Not included | Private DWG/DXF/PDF samples, binaries, build directories, caches, Engram data, ODD conversation/task logs |

There is no `sclMAX/opencadcodec` fork. No new repository was created and nothing was pushed to the codec upstream. The source bundle carries the local fix inside this checkpoint instead.

## Restore the code on another PC

These commands assume a fresh Linux checkout. Install the native prerequisites described in the project README; adapt environment setup for other operating systems.

```bash
git clone --branch checkpoint/dwg2010-codec-integration https://github.com/sclMAX/OpenCADStudio.git
cd OpenCADStudio
ROOT="$(pwd)"
mkdir -p target

git clone https://github.com/HakanSeven12/opencadcodec.git target/codec-assoc-2010
git -C target/codec-assoc-2010 fetch origin 14fdae51c24ea6cbfdaf2af257079a71ef806351
git -C target/codec-assoc-2010 bundle verify "$ROOT/patches/opencadcodec/dwg2010-associative-arc.bundle"
git -C target/codec-assoc-2010 fetch "$ROOT/patches/opencadcodec/dwg2010-associative-arc.bundle" \
  refs/heads/fix/dwg2010-associative-arc:refs/heads/fix/dwg2010-associative-arc
git -C target/codec-assoc-2010 switch fix/dwg2010-associative-arc
git -C target/codec-assoc-2010 rev-parse HEAD
# Expected: b058a27d36b5bfa83b9ba16e72cae3a8185fb935

# Local-only workspace boundary for the nested codec checkout.
printf '[workspace]\nmembers = ["codec-assoc-2010"]\nresolver = "2"\n' > target/Cargo.toml

git worktree add -b build/assoc-arc-codec-integration \
  target/worktrees/assoc-arc-integration 0b414389cb1a954ed22ddb95c44727fe0a5bd075
mkdir -p target/assoc-arc-integration
cp scripts/local_codec_integration/prepare.py scripts/local_codec_integration/verify.py \
  target/assoc-arc-integration/
python3 - <<'PY'
from pathlib import Path
root = Path.cwd()
template = (root / 'scripts/local_codec_integration/codec-local.toml.in').read_text()
config = template.replace('__LOCAL_CODEC_PATH__', (root / 'target/codec-assoc-2010').as_posix())
(root / 'target/assoc-arc-integration/codec-local.toml').write_text(config)
PY
```

The bundle requires the upstream base commit above; it is not a standalone full repository. Use the bundle to retain the exact commit identity. The adjacent textual patch is an alternative transport, not an additional patch to apply after importing the bundle.

Copy private samples separately if functional verification is needed. Expected paths are `diagnostics/region-dwg/originals/8216.dwg`, the 2013/2018 DWG and DXF variants, and `diagnostics/region-dwg/variants/8216_v2010.{dwg,dxf}`. They remain ignored and must not be published.

## Exact stopping point and next work

1. **Fix the unfinished provenance check before rerunning the harness.** `prepare.py` called full `cargo metadata --offline`; Cargo demanded uncached `android-activity v0.6.1`, even though this is a native Linux build. That command exited 101. Do not fetch irrelevant Android dependencies or call the integration verified without deciding the appropriate native dependency inspection. Cargo documentation says `--filter-platform` filters `resolve`, but still includes all packages; it is not proven to solve the manifest-fetch issue. Native `cargo tree --target x86_64-unknown-linux-gnu -p OpenCADStudio -i acadrust` / `cargo pkgid` are possible approaches, **not yet executed or proven**.
2. The snapshot also assumes the main checkout HEAD equals the application base. The checkpoint commit necessarily advances the main checkout. Adjust that assumption while keeping the **build worktree** pinned to the exact application base and protecting the main checkout before/after. Do not run the snapshots directly from `scripts/`; restore them under ignored `target/` first.
3. Validate resolution of the source-wide override to the clean local codec commit. The only observed worktree `Cargo.lock` change was removal of the Git `source` from the `acadrust 0.6.4` entry. No other dependency/version changed. The saved lock patch is evidence of that temporary delta, not a permanent source dependency update.
4. Once provenance and the lock delta are validated, compile the application. The command below is the intended next build, **not a completed build**:

```bash
ROOT="$(git rev-parse --show-toplevel)"
cd target/worktrees/assoc-arc-integration
cargo --config "$ROOT/target/assoc-arc-integration/codec-local.toml" \
  build --release --bin OpenCADStudio --offline --locked \
  --target-dir "$ROOT/target/assoc-arc-integration/build"
```

5. Run the restored `verify.py` against the resulting executable using isolated HOME/XDG state. It uses public stdio `--serve --read-only` requests and inspects all five typed auxiliary handles across seven files. Legacy `open` drops read statistics: success does not establish strict-open success or zero recovered errors. Verify the actual codec source/build provenance independently before declaring completion.

No AppImage packaging, installation, publishing or unrelated REGION correction is authorized by this checkpoint.

## Prior codec verification and limitations

- Five focused public synthetic regressions passed after observed RED.
- Independent verification decoded all five auxiliary objects in seven inputs and passed **70 exact object-data comparisons** through DWG and DXF save/reopen, without synthesized scalar values.
- The full codec suite has a properties-table assertion failure also reproduced on the untouched base. Excluding that one known failure: **2013 passed, 35 ignored**. Do not call the unfiltered suite green.
- Repository-wide formatting differences predate this fix; partially encoded-scalar truncation lacks an independent fixture (coverage advisory).
- Source fix is limited to terminal root AC1024 associative arcs with no prior reader error and exact main-stream exhaustion. Modern four-scalar and nested parsing remain unchanged. REGION framing in other drawings is a separate unresolved problem.
