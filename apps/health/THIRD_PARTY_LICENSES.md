# Third-party licenses

Go module dependencies live under `vendor/` with their own `LICENSE` files.
The removed web reference assets (Chart.js and the Recursive font) are not
distributed by this tree and therefore are no longer listed here.

| Module | License |
|---|---|
| `github.com/mattn/go-sqlite3` | MIT (Go wrapper). Bundled `sqlite3-binding.c` is the sqlite.org amalgamation (public domain / blessing). |

`github.com/muktihari/fit` is on the frozen dependency list for the FIT adapter
and is not vendored.
