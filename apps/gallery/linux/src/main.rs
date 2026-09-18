//! GTK / libadwaita shell. One window: laptop chrome or the Comet collapse.

use std::env;

use adw::prelude::*;

const APP_ID: &str = "com.j23n.LocalGallery.Reference";

fn main() {
    localcore_trace::init(localcore_trace::Init {
        app: "localgallery-reference",
        lanes: &[
            localcore_trace::Lane {
                kind: "FOREGROUND",
                detail: "leftover GTK (frozen): open_library, grid bind, viewer",
            },
            localcore_trace::Lane {
                kind: "BACKGROUND",
                detail: "scan, EXIF enrich, DecodePool, folder watch",
            },
        ],
    });
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(move |app| {
        let comet = env::args().any(|a| a == "--comet");
        localgallery::ui::Window::present(app, comet);
    });
    let argv: Vec<String> = env::args().filter(|a| a != "--comet").collect();
    app.run_with_args(&argv);
}
