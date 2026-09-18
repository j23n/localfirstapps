# localcore-trace

Shared debug traces for every LocalFiles core and shell. Cores emit;
GTK binaries call `init` once on the UI thread.

This is not `localcore-log` (user-data NDJSON) and not the in-app
Diagnostics `LogStore`.

```
LOCALFILES_DEBUG=1 cargo run -p gallery-gtk
LOCALFILES_DEBUG=2 cargo run -p music-gtk
RUST_LOG=lf=debug cargo run -p contacts-gtk
```

`1` prints summaries and spans ≥5 ms. `2` prints every event.
`RUST_LOG` / `LOCALFILES_LOG` override the filter (`lf=debug`).
`LOCALGALLERY_DEBUG` is accepted as an alias of `LOCALFILES_DEBUG`.

`[lf main]` is the thread that called `init` (jank if a line is slow).
`[lf work]` is every other thread (scan, decode pool, queues).
Each line is stamped `HH:MM:SS.mmmZ +S.sss` (UTC, then seconds since `init`).
