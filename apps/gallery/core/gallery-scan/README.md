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
Scan totals: N files in F folders, list=Xms hits=H slow=S probe=0
```

The ignored `e2e_generated_library` test walks that tree (cold + light)
and is **not** a PR gate. Run it with `apps/gallery/scripts/e2e_20k.sh`
or the `E2E 20k` workflow (`workflow_dispatch` only). The ignored
`scan_bench` test writes a synthetic 10k tree of empty files.
