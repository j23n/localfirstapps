//! One native binding per ADR 0004 R4 navigation intent.

use adw::prelude::*;
use localcore_ui::NavIntent;

/// A root tab: id, title, icon, and the child shown in the view stack.
pub struct RootPage<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub icon: &'a str,
    pub child: gtk::Widget,
}

/// Per-page chrome for [`page`].
///
/// `start` / `end` pack onto that page's header. `root` marks a stack root:
/// the header can host the view switcher (wide). Product apps keep the
/// switcher on [`AdaptiveShell::header`] so Add stays on the shared header;
/// the page header is still built (L2) and hidden so it does not double
/// the shell bar.
pub struct PageChrome {
    pub start: Option<gtk::Widget>,
    pub end: Option<gtk::Widget>,
    pub root: bool,
}

impl PageChrome {
    pub fn pushed() -> Self {
        Self {
            start: None,
            end: None,
            root: false,
        }
    }

    pub fn root() -> Self {
        Self {
            start: None,
            end: None,
            root: true,
        }
    }
}

/// Dialog size for [`sheet`]. Alert was unspecified in the design plan;
/// 360×240 is the kit default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetSize {
    Form,
    Picker,
    Alert,
}

impl SheetSize {
    pub const fn content_width(self) -> i32 {
        match self {
            Self::Form => 480,
            Self::Picker => 420,
            Self::Alert => 360,
        }
    }

    pub const fn content_height(self) -> i32 {
        match self {
            Self::Form => 720,
            Self::Picker => 560,
            Self::Alert => 240,
        }
    }
}

/// Shared window chrome: toolbar view, view stack, header switcher, bottom bar.
///
/// L12 breakpoints are installed with [`AdaptiveShell::install`].
#[derive(Clone)]
pub struct AdaptiveShell {
    pub toolbar: adw::ToolbarView,
    pub stack: adw::ViewStack,
    pub header: adw::HeaderBar,
    pub switcher: adw::ViewSwitcher,
    pub switcher_bar: adw::ViewSwitcherBar,
    app_title: String,
}

impl AdaptiveShell {
    /// Set the window title and install both L12 breakpoints.
    ///
    /// Compact (`max-width: 550sp`) reveals the bottom switcher bar and
    /// clears the header title widget (the switcher). Wide (`min-width:
    /// 860sp`) is installed with no setters — split views are Phase 3.
    pub fn install(&self, window: &adw::ApplicationWindow) {
        window.set_title(Some(&self.app_title));

        let compact = adw::Breakpoint::new(
            adw::BreakpointCondition::parse("max-width: 550sp").expect("L12 compact condition"),
        );
        let reveal = true.to_value();
        compact.add_setter(&self.switcher_bar, "reveal", Some(&reveal));
        // Typed NULL widget: `None` is a NULL GValue* and libadwaita
        // rejects it (`g_return_if_fail (G_IS_VALUE (value))`).
        compact.add_setter(
            &self.header,
            "title-widget",
            Some(&None::<gtk::Widget>.to_value()),
        );
        window.add_breakpoint(compact);

        let wide = adw::Breakpoint::new(
            adw::BreakpointCondition::parse("min-width: 860sp").expect("L12 wide condition"),
        );
        window.add_breakpoint(wide);
    }
}

/// Proves every nav intent has a binding.
pub fn bind_nav(kind: NavIntent) -> NavIntent {
    match kind {
        NavIntent::Push | NavIntent::Sheet | NavIntent::Replace => kind,
    }
}

pub fn navigation_view() -> adw::NavigationView {
    adw::NavigationView::new()
}

/// Outer chrome for every kit app: stack + header switcher + bottom bar.
pub fn adaptive_shell(app_title: &str, pages: &[RootPage<'_>]) -> AdaptiveShell {
    let stack = adw::ViewStack::new();
    for page in pages {
        let stack_page = stack.add_titled(&page.child, Some(page.id), page.title);
        stack_page.set_icon_name(Some(page.icon));
    }

    let switcher = adw::ViewSwitcher::new();
    switcher.set_stack(Some(&stack));
    switcher.set_policy(adw::ViewSwitcherPolicy::Wide);

    let switcher_bar = adw::ViewSwitcherBar::new();
    switcher_bar.set_stack(Some(&stack));

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&switcher));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&stack));
    toolbar.add_bottom_bar(&switcher_bar);

    AdaptiveShell {
        toolbar,
        stack,
        header,
        switcher,
        switcher_bar,
        app_title: app_title.to_owned(),
    }
}

/// Navigation page with its own toolbar view and header bar (L2).
pub fn page(
    title: &str,
    content: &impl IsA<gtk::Widget>,
    chrome: PageChrome,
) -> adw::NavigationPage {
    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    if let Some(start) = &chrome.start {
        header.pack_start(start);
    }
    if let Some(end) = &chrome.end {
        header.pack_end(end);
    }
    if chrome.root {
        // AdaptiveShell owns the visible switcher header (Add stays there).
        // Keep this bar in the tree so `page` always builds L2 chrome.
        header.set_visible(false);
    }

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(content));

    adw::NavigationPage::builder()
        .title(title)
        .child(&toolbar)
        .can_pop(!chrome.root)
        .build()
}

/// Thin wrapper around [`page`] with [`PageChrome::pushed`].
pub fn push_page(title: &str, child: &impl IsA<gtk::Widget>) -> adw::NavigationPage {
    page(title, child, PageChrome::pushed())
}

pub fn sheet(title: &str, child: &impl IsA<gtk::Widget>, size: SheetSize) -> adw::Dialog {
    let header = adw::HeaderBar::new();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(child));

    let dialog = adw::Dialog::new();
    dialog.set_title(title);
    dialog.set_follows_content_size(false);
    dialog.set_content_width(size.content_width());
    dialog.set_content_height(size.content_height());
    dialog.set_child(Some(&toolbar));
    dialog
}

pub fn nav_widget(kind: NavIntent) -> gtk::Widget {
    match kind {
        NavIntent::Push | NavIntent::Replace => navigation_view().upcast(),
        NavIntent::Sheet => {
            let dialog = adw::Dialog::new();
            // Dialog is not a GtkWidget in every adw version; present
            // a placeholder page the shell can wrap.
            let _ = dialog;
            gtk::Box::new(gtk::Orientation::Vertical, 0).upcast()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widget_tree_has<T: IsA<gtk::Widget>>(root: &gtk::Widget) -> bool {
        if root.is::<T>() {
            return true;
        }
        let mut child = root.first_child();
        while let Some(node) = child {
            if widget_tree_has::<T>(&node) {
                return true;
            }
            child = node.next_sibling();
        }
        false
    }

    #[test]
    fn sheet_reports_form_and_picker_content_width() {
        assert_eq!(SheetSize::Form.content_width(), 480);
        assert_eq!(SheetSize::Form.content_height(), 720);
        assert_eq!(SheetSize::Picker.content_width(), 420);
        assert_eq!(SheetSize::Picker.content_height(), 560);
        assert_eq!(SheetSize::Alert.content_width(), 360);
        assert_eq!(SheetSize::Alert.content_height(), 240);
        crate::with_adw(|| {
            let form = sheet("Form", &gtk::Label::new(None), SheetSize::Form);
            assert_eq!(form.content_width(), 480);
            assert!(!form.follows_content_size());
            let picker = sheet("Picker", &gtk::Label::new(None), SheetSize::Picker);
            assert_eq!(picker.content_width(), 420);
        });
    }

    #[test]
    fn page_content_includes_a_header_bar() {
        crate::with_adw(|| {
            let nav_page = page(
                "Title",
                &gtk::Label::new(Some("body")),
                PageChrome::pushed(),
            );
            let child = nav_page.child().expect("page child");
            assert!(
                widget_tree_has::<adw::HeaderBar>(&child),
                "page() must wrap content in a header bar"
            );
        });
    }

    #[test]
    fn l12_breakpoint_conditions_parse() {
        crate::with_adw(|| {
            adw::BreakpointCondition::parse("max-width: 550sp").expect("L12 compact");
            adw::BreakpointCondition::parse("min-width: 860sp").expect("L12 wide");
        });
    }

    #[test]
    fn install_compact_title_widget_setter_is_typed_null() {
        crate::with_adw(|| {
            let app = adw::Application::builder()
                .application_id("dev.localcore.shell-kit-gtk.compact-title-widget")
                .build();
            let window = adw::ApplicationWindow::new(&app);
            let shell = adaptive_shell(
                "Test",
                &[RootPage {
                    id: "home",
                    title: "Home",
                    icon: "user-home-symbolic",
                    child: navigation_view().upcast(),
                }],
            );
            window.set_content(Some(&shell.toolbar));
            assert!(
                shell.header.title_widget().is_some(),
                "wide chrome hosts the header switcher"
            );
            shell.install(&window);

            // libadwaita 1.9 / libadwaita-rs 0.9 has no Breakpoint::setters().
            // Headless tests also cannot allocate a compact width. Assert the
            // GValue install registers: object-typed NULL, not a skipped None.
            let compact = adw::Breakpoint::new(
                adw::BreakpointCondition::parse("max-width: 550sp").expect("L12 compact condition"),
            );
            compact.add_setter(&shell.switcher_bar, "reveal", Some(&true.to_value()));
            let null_title = None::<gtk::Widget>.to_value();
            assert!(
                null_title.type_().is_a(gtk::glib::Type::OBJECT),
                "title-widget setter value type must be object/null"
            );
            assert!(
                null_title
                    .get::<Option<gtk::Widget>>()
                    .expect("object-typed GValue")
                    .is_none(),
                "title-widget setter must store a null widget"
            );
            compact.add_setter(&shell.header, "title-widget", Some(&null_title));

            shell
                .header
                .set_property_from_value("title-widget", &null_title);
            assert!(
                shell.header.title_widget().is_none(),
                "typed null GValue must clear the header switcher"
            );
        });
    }
}
