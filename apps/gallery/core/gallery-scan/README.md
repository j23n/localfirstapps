# gallery-scan

Folder traversal: the tree, the flat photo list, and the diff against
the last scan. Classification stays here; the walk is `localcore-walk`.

## 20k harness

There is no 20k tree in the repo. Generate one with
`apps/gallery/scripts/generate_test_library.py` (default 20k, seed 42),
then walk it through this crate (headless, no FFI, no shell):

```sh
cargo run -p gallery-scan --release --example scan_tree -- /path/to/photos
```

From this directory, same binary:

```sh
cargo run --release --example scan_tree -- /path/to/photos
```

It prints the CoreScanner totals line:

```
Scan totals: N files in F folders, list=Xms hits=H slow=S
```

The ignored `e2e_generated_library` test is the 20k performance-regression
suite: cold + light scan, sidecar/EXIF enrich, then `gallery-index` search
and `gallery-memories` generate + 7-day horizon on that same table.
Structural counts are committed under `tests/e2e_baselines/`; timings
live next to the generated tree (`LOCALGALLERY_E2E_RECORD=1` rewrites
both). Root `rust.yml` `gallery-core` runs
`apps/gallery/scripts/e2e_20k.sh` as a required step (no 20k tree
in-repo; the script generates one). The ignored `scan_bench` test
writes a synthetic 10k tree of empty files.
