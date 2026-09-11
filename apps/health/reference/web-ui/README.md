# Health web UI — Phase 6 brief

This is the loopback UI that used to ship as `archive serve`. It is **not
built** and is **not a product surface**. Phase 1 removed the listen
path so the binary no longer opens a socket (ADR 0006 R9).

Keep this tree. Phase 6 regenerates native shells from it. Read the
templates and goldens for information architecture; read `plot.go` /
`cards.go` for what a "chart" actually has to show (grain, empty
buckets, min–max range bars, source provenance). Do not port Chart.js,
the Recursive font, or the HTTP server.

ADR 0004 R4 has no `chart` or metric-card kind. Health's screens that
do not fit `list` / `detail` / `status-row` are a **genuine gap**
(R5): amend R4 with one binding per platform, or revise the screen.
A one-off widget in one shell is the failure mode R4 exists to prevent.

Nested `go.mod` so `go test ./...` from `apps/health` does not compile
this package.
