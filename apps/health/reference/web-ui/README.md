# Health web UI — Phase 6 brief

This is the loopback UI that used to ship as `archive serve`. It is **not
built** and is **not a product surface**. Phase 1 removed the listen
path so the binary no longer opens a socket (ADR 0006 R9).

This is a frozen reference, not a maintained application or test target. Its
nested module imports packages that no longer belong to it and does not build
standalone. Before Phase 6, curate the screen map, screenshots, chart data
contracts and representative goldens, then delete the executable HTTP/server
husk. That curation is deliberately deferred from the current cleanup. Do not
port Chart.js, the Recursive font, or the HTTP server.

ADR 0004 R4 has no `chart` or metric-card kind. Health's screens that
do not fit `list` / `detail` / `status-row` are a **genuine gap**
(R5): amend R4 with one binding per platform, or revise the screen.
A one-off widget in one shell is the failure mode R4 exists to prevent.

The nested `go.mod` only keeps root `go test ./...` from compiling this
package; it is isolation, not evidence that the reference builds.
