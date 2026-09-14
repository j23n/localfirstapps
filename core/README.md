# core

This workspace holds `localcore-*` crates (ADR 0001 R9), the first
app core, and the R14 vocabulary crate:

`localcore-vfs`, `localcore-id`, `localcore-walk`, `localcore-conflict`,
`localcore-queue`, `localcore-log`, `localcore-blob`, `localcore-geo`,
`localcore-ui` (ADR 0004 kinds + token hex; no UI toolkit),
`contacts-core` (display rows and logged actions; both shells),
`contacts-ffi` (R6-clean; iOS writes and logs through it).

Token tables live in `design/tokens/` (sourced dark accents as of
3.5). Contacts screens live in `apps/contacts/ui-spec/`. Regenerate
with `python3 scripts/gen_r14.py`; CI runs `--check`. GTK lives in
`shells/` (`shell-kit-gtk`, `contacts-gtk`), not here.

Gallery remains at `apps/gallery/core` until later verticals move here.

`.github/workflows/rust.yml` is the crate gate: `cargo test --locked
--workspace --all-targets` on this workspace (`localcore` job) and on
`apps/gallery/core` (`gallery-core` job). Gallery Linux GTK, iOS
`xcodebuild`, contacts, music, and the health log suite are
`.github/workflows/apps.yml`.

## 20k scan harness

There is no 20k tree in this repo. Generate one with
`apps/gallery/scripts/generate_test_library.py`, then walk it through
`gallery-scan` (which uses `localcore-walk`):

```
cd apps/gallery/core
cargo run -p gallery-scan --release --example scan_tree -- /path/to/photos
```

Prints the `Scan totals:` line CoreScanner logs. The ignored
`e2e_generated_library` suite (`apps/gallery/scripts/e2e_20k.sh`) walks
the generated tree, enriches it, and regresses index + memories against
a recorded baseline. Local-only; not CI.
See `apps/gallery/core/gallery-scan/README.md`.
