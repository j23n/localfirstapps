//! Walk a library tree through `gallery-scan` and print the scan-totals line.
//!
//! There is no 20k tree in the repo. Point this at an external library:
//!
//! ```sh
//! cargo run -p gallery-scan --release --example scan_tree -- /path/to/photos
//! ```
//!
//! Prints the same counters CoreScanner logs (`files`, `folders`, `list`,
//! `hits`, `slow`). The walk is `localcore-walk` via `gallery-scan` over
//! `StdVfs`. Not CI-gated.

use std::env;
use std::path::Path;
use std::process::ExitCode;

use gallery_scan::{scan, ScanInput};
use gallery_vfs::StdVfs;

fn main() -> ExitCode {
    let Some(root) = env::args().nth(1) else {
        eprintln!("usage: scan_tree <directory>");
        return ExitCode::FAILURE;
    };
    if !Path::new(&root).is_dir() {
        eprintln!("scan_tree: not a directory: {root}");
        return ExitCode::FAILURE;
    }

    let outcome = scan(&StdVfs::new(), &root, &ScanInput::default());
    let s = outcome.stats;
    let list_ms = s.list_micros / 1000;
    println!(
        "Scan totals: {} files in {} folders, list={}ms hits={} slow={}",
        outcome.flat_photos.len(),
        s.folders,
        list_ms,
        s.cache_hits,
        s.slow_path,
    );
    ExitCode::SUCCESS
}
