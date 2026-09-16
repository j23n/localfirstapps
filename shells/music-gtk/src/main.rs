use adw::prelude::*;

fn main() {
    let comet = music_gtk::wants_comet(std::env::args());
    let app = adw::Application::builder()
        .application_id(music_gtk::APP_ID)
        .build();
    app.connect_activate(move |app| music_gtk::Window::present(app, comet));
    let arguments = std::env::args()
        .filter(|argument| argument != "--comet")
        .collect::<Vec<_>>();
    app.run_with_args(&arguments);
}
