# How work routes to agents

**By work item, not by path.** 27% of gallery's recent commits touch both
Swift and `core/*.rs`, and they are the architecturally significant ones.
"Move Places, pack, and merge policy into the core" is 26 Swift files and
23 Rust files. Routing by path routes *file edits inside a work item*.

**Additive FFI is the discipline that makes this work.** New functions
land alongside old; Swift migrates; the old surface is removed. `main` is
never knowingly unbuildable for a platform, so `macos-26` stays a signal
rather than an expected-red job — which is precisely when
committed-binding drift would otherwise go unnoticed.

## Where it runs

Shared crates now live in `core/`, and GTK shell work lives in `shells/`.
Gallery's pre-migration Rust workspaces remain under `apps/gallery/` until
its vertical moves; route by the whole work item across those boundaries.

| Work | Environment | Verified by |
|---|---|---|
| `core/**`, `shells/**`, `apps/gallery/core/**`, `apps/gallery/linux/**` | Fedora container (`docker/`) | relevant workspace `cargo test` |
| `apps/health/**` | Fedora container (`docker/`) | `CGO_ENABLED=1 GOFLAGS=-mod=vendor go test ./...` |
| `docs/**`, `conformance/**` | Fedora container (`docker/`) | conformance checks and textual consistency searches |
| FFI surface change | container, then Mac | Linux `swift build` shim (Phase 0.4), then `macos-26` |
| app Swift | Mac (`mac/`) | `macos-26` by default; Mac VM interactively when more than one round is needed |

Two agents in one checkout fight over the git index. Before a long edit,
`git worktree add .worktrees/<task> -b <branch>` and work there.

## Task shape

One requirement, one fixture, one PR. "Make `contacts-core` satisfy
ADR 0005 R7, here is the fixture directory, the check must go red to
green." Reviewable in minutes because the check is the review — for the
requirements where that is true.

What CI cannot check is named in ADR 0007 R16. The pull-request
template asks the review question; a person answers it in prose.

## Do not

- Split a work item onto a Swift agent and a Rust agent by path.
- Leave `main` unbuildable on one platform while the other moves.
- Treat a container `cargo test` as an iOS verification, or an Xcode
  build as a Linux one.
