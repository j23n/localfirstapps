//! LocalContacts GTK binary. `--comet` is the compact default size;
//! chrome follows window width (ADR 0004 R8).

use adw::prelude::*;

fn main() {
    let launch = match contacts_gtk::parse_launch_args(std::env::args()) {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let app = adw::Application::builder()
        .application_id(contacts_gtk::APP_ID)
        .build();
    app.connect_activate(move |app| {
        contacts_gtk::Window::present(app, &launch);
    });
    let argv = vec![std::env::args()
        .next()
        .unwrap_or_else(|| "localcontacts".into())];
    app.run_with_args(&argv);
}
