//! GTK4 / libadwaita bindings for the ADR 0004 closed vocabulary.
//!
//! One native widget (or dialog) per kind. No app core, no `localcore`
//! folder crate — only `localcore-ui` for the generated enums
//! (ADR 0001 R2 / R9).
//!
//! A shell MUST NOT read a UI spec at runtime. It matches on these
//! enums. An unhandled kind is a compile error.

pub use localcore_ui::{
    count_detail, found_detail, ActionRole, Affordance, ContactsScreen, GalleryScreen,
    HealthScreen, ItemKind, MusicScreen, NavIntent, ProgressDisplay, ScreenKind, StatusSeverity,
    WorkProgress, REVEAL_AFTER,
};

mod affordance;
mod builder;
mod chrome;
mod data;
mod diagnostics;
mod item;
mod nav;
mod reuse;
mod screen;

#[cfg(debug_assertions)]
pub mod snapshot;

pub use affordance::*;
pub use builder::*;
pub use chrome::*;
pub use data::*;
pub use diagnostics::*;
pub use item::*;
pub use nav::*;
pub use reuse::*;
pub use screen::*;

/// Kit stylesheet: token-driven named-colour bridge plus `.thumb`.
const KIT_STYLE: &str = include_str!("../data/style.css");

/// Load generated token CSS, then the kit stylesheet.
///
/// Each string is a separate `CssProvider` at
/// `STYLE_PROVIDER_PRIORITY_APPLICATION`. Tokens are registered first so
/// `data/style.css` can map libadwaita named colours onto
/// `--accent-bg-color` / `--accent-fg-color` without touching
/// `accent_color` / `--accent-color`.
pub fn init_style(app_token_css: &str) {
    apply_token_css(app_token_css);
    apply_token_css(KIT_STYLE);
}

/// Load one CSS string onto the default display at application priority.
fn apply_token_css(css: &str) {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(css);
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("a display"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

#[cfg(test)]
pub(crate) fn with_adw(test: impl FnOnce()) {
    use std::sync::Mutex;
    static ADW: Mutex<Option<bool>> = Mutex::new(None);
    let mut slot = ADW.lock().expect("adw test lock");
    let ok = *slot.get_or_insert_with(|| adw::init().is_ok() || gtk::is_initialized());
    if ok {
        test();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_binding() {
        for kind in ScreenKind::ALL {
            let _ = bind_screen(*kind);
        }
        for kind in ItemKind::ALL {
            let _ = bind_item(*kind);
        }
        for kind in Affordance::ALL {
            let _ = bind_affordance(*kind);
        }
        for kind in NavIntent::ALL {
            let _ = bind_nav(*kind);
        }
    }

    #[test]
    fn kit_style_bridges_accent_bg_without_clobbering_accent_text() {
        assert!(KIT_STYLE.contains("@define-color accent_bg_color var(--accent-bg-color)"));
        assert!(KIT_STYLE.contains("@define-color accent_fg_color var(--accent-fg-color)"));
        assert!(!KIT_STYLE.contains("@define-color accent_color"));
        assert!(!KIT_STYLE.contains("--accent-color:"));
        assert!(KIT_STYLE.contains(".thumb {"));
        assert!(KIT_STYLE.contains("border-radius: var(--thumb-radius);"));
        assert!(!KIT_STYLE.contains("Newsreader"));
        assert!(!KIT_STYLE.contains(".memory-title"));
        assert!(!KIT_STYLE.contains("font-family"));
    }

    #[test]
    fn contacts_spec_uses_only_bound_kinds() {
        for screen in ContactsScreen::ALL {
            let _ = bind_screen(screen.kind());
        }
        assert_eq!(
            ContactsScreen::SyncConflictGroup.as_str(),
            "sync-conflict-group"
        );
    }
}
