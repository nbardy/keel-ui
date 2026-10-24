//! Menus: popup hierarchy, disabled actions, keyboard navigation and context placement.
use kit_core::kurbo::Point;
use kit_core::{Key, NamedKey, Theme, WidgetId};
use kit_state::{Cx, Ident, Intent, Life};
use kit_test::{Asked, Handle, TestApp};
use kit_view::{Node, Role, Toggle};
use kit_widgets::Press;
use kit_widgets::menu::{Menu, MenuEntry, MenuItem, MenuKind, menu};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Id;
impl Ident<TestApp> for Id {
    fn life(&self, _: &TestApp) -> Life {
        Life::Live
    }
}
fn view(cx: &Cx<'_, TestApp>, theme: &Theme) -> Node<Intent<TestApp>> {
    build(cx, theme, MenuKind::Button)
}
fn context(cx: &Cx<'_, TestApp>, theme: &Theme) -> Node<Intent<TestApp>> {
    build(
        cx,
        theme,
        MenuKind::Context {
            at: Point::new(24.0, 24.0),
            opener: WidgetId::of(&Id),
        },
    )
}
fn build(cx: &Cx<'_, TestApp>, theme: &Theme, kind: MenuKind) -> Node<Intent<TestApp>> {
    let action =
        |label: &'static str| MenuItem::Item(label, Press::Sends(Intent::Ask(Asked(label.into()))));
    let children = [
        action("Sloop"),
        MenuItem::Item("Locked brig", Press::Disabled),
        action("Cutter"),
    ];
    let entries = [
        MenuEntry::Item(action("New voyage")),
        MenuEntry::Item(MenuItem::Separator),
        MenuEntry::Item(MenuItem::Item("Save", Press::Disabled)),
        MenuEntry::Submenu("Ship", &children),
        MenuEntry::Item(action("Quit")),
    ];
    menu(
        cx,
        Menu {
            theme,
            key: Id,
            label: "Voyage",
            kind,
            entries: &entries,
        },
    )
    .pad(theme.spacing.sm)
}
fn key(
    h: &mut Handle<TestApp, impl Fn(&Cx<'_, TestApp>, &Theme) -> Node<Intent<TestApp>>>,
    key: NamedKey,
) {
    h.press_key(Key::Named(key));
}
#[test]
fn golden_closed_root_submenu_and_context() {
    let mut h = Handle::new(TestApp::new(), (360.0, 240.0), view);
    h.assert_golden("menu_closed");
    h.click(Role::Button, "Voyage");
    h.assert_golden("menu_open");
    h.click(Role::Button, "Ship");
    h.assert_golden("menu_submenu");
    Handle::new(TestApp::new(), (360.0, 240.0), context).assert_golden("menu_context");
}
#[test]
fn escape_closes_only_submenu_but_selection_emits_and_closes_all() {
    let mut h = Handle::new(TestApp::new(), (360.0, 240.0), view);
    h.click(Role::Button, "Voyage");
    h.click(Role::Button, "Ship");
    assert!(h.shows(Role::Button, "Sloop"));
    key(&mut h, NamedKey::Escape);
    assert!(!h.shows(Role::Button, "Sloop"));
    assert!(
        h.shows(Role::Button, "New voyage"),
        "Escape preserves the root menu"
    );
    assert_eq!(h.toggle_of(Role::Button, "Voyage"), Toggle::On);
    assert!(h.app().asked.is_empty());
    h.click(Role::Button, "Ship");
    h.click(Role::Button, "Sloop");
    assert_eq!(h.app().asked, [Asked("Sloop".into())]);
    assert!(!h.shows(Role::Button, "Sloop"));
    assert!(!h.shows(Role::Button, "New voyage"));
    assert_eq!(h.toggle_of(Role::Button, "Voyage"), Toggle::Off);
}
#[test]
fn arrows_skip_disabled_items_enter_selects_and_left_closes_submenu() {
    let mut h = Handle::new(TestApp::new(), (360.0, 240.0), view);
    for k in [
        NamedKey::Tab,
        NamedKey::Enter,
        NamedKey::ArrowDown,
        NamedKey::ArrowDown,
        NamedKey::ArrowRight,
    ] {
        key(&mut h, k);
    }
    assert!(
        h.shows(Role::Button, "Sloop"),
        "Right on Ship opens the submenu, skipping Save"
    );
    key(&mut h, NamedKey::ArrowLeft);
    assert!(!h.shows(Role::Button, "Sloop"));
    key(&mut h, NamedKey::ArrowRight);
    for k in [
        NamedKey::ArrowDown,
        NamedKey::ArrowDown,
        NamedKey::ArrowUp,
        NamedKey::Enter,
    ] {
        key(&mut h, k);
    }
    assert_eq!(
        h.app().asked,
        [Asked("Sloop".into())],
        "Down skips locked brig; Up returns to Sloop"
    );
    assert!(!h.shows(Role::Button, "Quit"));
}
#[test]
fn disabled_click_does_nothing_and_outside_click_dismisses_both_levels() {
    let mut h = Handle::new(TestApp::new(), (360.0, 240.0), view);
    h.click(Role::Button, "Voyage");
    h.click(Role::Button, "Save");
    assert!(h.shows(Role::Button, "Ship"));
    assert!(h.app().asked.is_empty());
    h.click(Role::Button, "Ship");
    h.click_at(Point::new(350.0, 230.0));
    assert!(!h.shows(Role::Button, "Ship"));
    assert!(!h.shows(Role::Button, "Sloop"));
    assert!(h.app().asked.is_empty());
}
#[test]
fn context_selection_and_escape_dismiss_without_a_menu_button() {
    let mut h = Handle::new(TestApp::new(), (360.0, 240.0), context);
    assert!(!h.shows(Role::Button, "Voyage"));
    h.click(Role::Button, "New voyage");
    assert_eq!(h.app().asked, [Asked("New voyage".into())]);
    assert!(!h.shows(Role::Button, "New voyage"));
    let mut h = Handle::new(TestApp::new(), (360.0, 240.0), context);
    key(&mut h, NamedKey::Escape);
    assert!(!h.shows(Role::Button, "New voyage"));
    assert!(h.app().asked.is_empty());
}
