use adw::prelude::*;

fn main() {
    localcore_trace::init(localcore_trace::Init {
        app: "localmusic",
        lanes: &[
            localcore_trace::Lane {
                kind: "FOREGROUND",
                detail: "open_folder, metadata batch, list refill, artwork decode",
            },
            localcore_trace::Lane {
                kind: "BACKGROUND",
                detail: "folder walk, lofty metadata, gstreamer transport (if enabled)",
            },
        ],
    });
    let launch = match music_gtk::parse_launch_args(std::env::args()) {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let app = adw::Application::builder()
        .application_id(music_gtk::APP_ID)
        .build();
    app.connect_activate(move |app| music_gtk::Window::present(app, &launch));
    let arguments = vec![std::env::args()
        .next()
        .unwrap_or_else(|| "localmusic".into())];
    app.run_with_args(&arguments);
}
