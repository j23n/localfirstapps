//! LocalContacts GTK binary. `--comet` is the compact default size;
//! chrome follows window width (ADR 0004 R8).

use adw::prelude::*;

fn main() {
    let comet = contacts_gtk::wants_comet(std::env::args());
    let app = adw::Application::builder()
        .application_id(contacts_gtk::APP_ID)
        .build();
    app.connect_activate(move |app| {
        contacts_gtk::Window::present(app, comet);
    });
    let argv: Vec<String> = std::env::args().filter(|a| a != "--comet").collect();
    app.run_with_args(&argv);
}
