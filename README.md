# localfiles

Four apps, one repository. Each app projects a user-selected folder of files
into a browsable view and performs no runtime networking.

| Path | App | Build from |
|---|---|---|
| [`apps/gallery`](apps/gallery) | localgallery | `apps/gallery` |
| [`apps/contacts`](apps/contacts) | localcontacts | `apps/contacts` |
| [`apps/music`](apps/music) | localmusic | `apps/music` |
| [`apps/health`](apps/health) | localhealth | `apps/health` |

Specification: [`docs/spec/README.md`](docs/spec/README.md).
Plan: [`docs/IMPLEMENTATION-PLAN.md`](docs/IMPLEMENTATION-PLAN.md).
Pages: [`docs/index.html`](docs/index.html).
Agent container: [`docker/README.md`](docker/README.md).
Mac (iOS shells): [`mac/README.md`](mac/README.md).

CI (root `.github/workflows` only — nested `apps/*/.github` does not run here):

| Workflow | What it proves |
|---|---|
| `conformance.yml` | Currently-green runtime dependency graph with reviewed build-time exceptions, retired-host-API greps, ADR 0003 R6 Record inventory (`gallery-ffi` expected-red; contacts syntax-green with serialized-vCard debt), ADR 0004 R14 `--check` |
| `bindings.yml` | UniFFI drift (gallery + contacts) + Linux Swift shim |
| `rust.yml` | `cargo test --locked --workspace` for `core/` and `apps/gallery/core` |
| `apps.yml` | Gallery Linux + iOS, shells workspace (`shell-kit-gtk` + `contacts-gtk`), contacts iOS, music iOS, health log/event |
