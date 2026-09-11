# gallery-scan

Folder traversal: the tree, the flat photo list, and the diff against
the last scan. Classification stays here; `localcore-walk` is not on
this branch.

## 20k harness

There is no 20k tree in the repo. Walk an external library through
this crate (headless, no FFI, no shell):

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

Not CI-gated. The ignored `scan_bench` test writes a synthetic 10k
tree; this example is the one that points at a real library.
