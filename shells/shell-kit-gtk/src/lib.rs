//! GTK4 / libadwaita bindings for the ADR 0004 closed vocabulary.
//!
//! One native widget (or dialog) per kind. No app core, no `localcore`
//! folder crate — only `localcore-ui` for the generated enums
//! (ADR 0001 R2 / R9).
//!
//! A shell MUST NOT read a UI spec at runtime. It matches on these
//! enums. An unhandled kind is a compile error.

pub use localcore_ui::{
    ActionRole, Affordance, ContactsScreen, ItemKind, NavIntent, ScreenKind, StatusSeverity,
};

mod affordance;
mod data;
mod diagnostics;
mod item;
mod nav;
mod reuse;
mod screen;

pub use affordance::*;
pub use data::*;
pub use diagnostics::*;
pub use item::*;
pub use nav::*;
pub use reuse::*;
pub use screen::*;

/// Load generated token CSS onto the default display.
pub fn apply_token_css(css: &str) {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(css);
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("a display"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
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
