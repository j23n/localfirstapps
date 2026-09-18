//! LocalGallery GTK binary. `--comet` is the compact default size;
//! chrome follows window width (ADR 0004 R8).

use adw::prelude::*;

fn main() {
    localcore_trace::init(localcore_trace::Init {
        app: "localgallery",
        lanes: &[
            localcore_trace::Lane {
                kind: "FOREGROUND",
                detail: "widget bind, chrome WorkProgress, idle ListStore chunks, wake-on-result Texture drain",
            },
            localcore_trace::Lane {
                kind: "BACKGROUND",
                detail: "FFI scanner.scan, prepare_ui, leftover_open snapshot persist, MemoryGenerator, DecodePool, folder watch",
            },
            localcore_trace::Lane {
                kind: "IDLE",
                detail: "Places/EXIF/thumb queues and AnalysisSession are not started — warm start hydrates People from the leftover JSON snapshot",
            },
        ],
    });
    let launch = match gallery_gtk::parse_launch_args(std::env::args()) {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let app = adw::Application::builder()
        .application_id(gallery_gtk::APP_ID)
        .build();
    app.connect_activate(move |app| {
        gallery_gtk::Window::present(app, &launch);
    });
    let argv = vec![std::env::args()
        .next()
        .unwrap_or_else(|| "localgallery".into())];
    app.run_with_args(&argv);
}
