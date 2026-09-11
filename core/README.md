# core

This workspace holds `localcore-*` crates (ADR 0001 R9). Gallery remains
at `apps/gallery/core` until later verticals move here.

## 20k scan harness

There is no 20k tree in this repo, and `localcore-walk` is not on this
branch. Walk an external library through `gallery-scan`:

```
cd apps/gallery/core
cargo run -p gallery-scan --release --example scan_tree -- /path/to/photos
```

Prints the `Scan totals:` line CoreScanner logs. Not CI-gated.
See `apps/gallery/core/gallery-scan/README.md`.
