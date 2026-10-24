//! Context menus and focus restoration, driven through the real router: a secondary click opens an item's menu and
//! never its primary action, and whatever closes the menu hands focus back to the item that opened it.
use crate::support::{Asked, Handle, TestApp};
use kit_core::kurbo::Point;
use kit_core::{Key, NamedKey, Theme, WidgetId};
use kit_input::Focus;
use kit_state::{Act, Cx, Ident, Intent, Life};
use kit_view::{A11y, Bindings, Node, Role, TextStyle, Toggle, column, interactive, nothing, text};
use kit_widgets::Press;
use kit_widgets::menu::{Menu, MenuEntry, MenuItem, MenuKind, menu};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Slot;
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Drop;
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Gone;
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Gesture;
/// A fresh key per context gesture, as `MenuKind::Context` asks.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Popup(u32);
macro_rules! live {
    ($($k:ty),*) => {$(impl Ident<TestApp> for $k {
        fn life(&self, _: &TestApp) -> Life { Life::Live }
    })*};
}
live!(Slot, Drop, Gone, Gesture, Popup);

#[derive(Clone, Copy, PartialEq, Debug)]
struct Opened {
    n: u32,
    at: Point,
}

fn label(theme: &Theme, name: &str) -> Node<Intent<TestApp>> {
    let style = TextStyle::new(theme.type_scale.body as f32, theme.colors.text);
    text(name, style).pad(theme.spacing.md)
}
fn named(name: &str) -> A11y {
    A11y::Labeled {
        role: Role::Button,
        label: name.into(),
        toggle: Toggle::NotToggle,
    }
}
fn ask(what: &str) -> Intent<TestApp> {
    Intent::Ask(Asked(what.into()))
}

/// An inventory slot that uses its item on a left click and opens a menu on a right click; a "Drop" button beside it
/// (hotkey `d`) removes the slot from the screen.
fn view(cx: &Cx<'_, TestApp>, theme: &Theme) -> Node<Intent<TestApp>> {
    let gone = cx.state(
        &Gone,
        || false,
        |s: &mut bool, _: &Gone, e: bool, _: &TestApp| {
            *s = e;
            Act::none()
        },
    );
    let opened = cx.state(
        &Gesture,
        || Opened {
            n: 0,
            at: Point::ZERO,
        },
        |s: &mut Opened, _: &Gesture, e: Opened, _: &TestApp| {
            *s = e;
            Act::none()
        },
    );
    let sender = opened.sender();
    let n = opened.n;
    let slot = match *gone {
        true => nothing(),
        false => interactive(
            WidgetId::of(&Slot),
            Bindings::activate(ask("use sword"))
                .on_context(move |at| sender.send(Opened { n: n + 1, at })),
            named("Sword"),
            label(theme, "Sword"),
        ),
    };
    let drop = interactive(
        WidgetId::of(&Drop),
        Bindings::activate(gone.send(true)).with_shortcut(Key::Character('d')),
        named("Drop"),
        label(theme, "Drop"),
    );
    let entries = [
        MenuEntry::Item(MenuItem::Item("Inspect", Press::Sends(ask("inspect")))),
        MenuEntry::Item(MenuItem::Item("Equip", Press::Sends(ask("equip")))),
    ];
    let menu = match opened.n {
        0 => nothing(),
        n => menu(
            cx,
            Menu {
                theme,
                key: Popup(n),
                label: "Sword menu",
                kind: MenuKind::Context {
                    at: opened.at,
                    opener: WidgetId::of(&Slot),
                },
                entries: &entries,
            },
        ),
    };
    column([slot, menu, drop])
}

fn handle() -> Handle<TestApp, impl Fn(&Cx<'_, TestApp>, &Theme) -> Node<Intent<TestApp>>> {
    Handle::new(TestApp::new(), (360.0, 240.0), view)
}
fn on_slot() -> Focus {
    Focus::On(WidgetId::of(&Slot))
}
fn key(
    h: &mut Handle<TestApp, impl Fn(&Cx<'_, TestApp>, &Theme) -> Node<Intent<TestApp>>>,
    k: NamedKey,
) {
    h.press_key(Key::Named(k));
}

/// A secondary click opens the menu and must not use the item; a primary click uses it and opens no menu.
#[test]
fn right_click_opens_the_menu_without_the_primary_action() {
    let mut h = handle();
    h.right_click(Role::Button, "Sword");
    assert!(h.shows(Role::Button, "Inspect"), "the context menu opened");
    assert!(h.app().asked.is_empty(), "a right-click never activates");
    assert_eq!(h.focus(), on_slot(), "the pressed item holds focus");

    let mut h = handle();
    h.click(Role::Button, "Sword");
    assert_eq!(h.app().asked, [Asked("use sword".into())]);
    assert!(
        !h.shows(Role::Button, "Inspect"),
        "a left-click opens no menu"
    );
}

/// Regression guard: closing a menu used to leave focus on nothing, so the next arrow key started from the top.
#[test]
fn escape_returns_focus_to_the_opener() {
    let mut h = handle();
    h.right_click(Role::Button, "Sword");
    key(&mut h, NamedKey::ArrowDown);
    assert_ne!(h.focus(), on_slot(), "focus walked into the menu");
    assert_ne!(h.focus(), Focus::Unfocused);
    key(&mut h, NamedKey::Escape);
    assert!(!h.shows(Role::Button, "Inspect"));
    assert_eq!(h.focus(), on_slot());
    assert_eq!(h.restore_fallbacks(), 0);
}

#[test]
fn choosing_an_item_fires_it_and_returns_focus() {
    let mut h = handle();
    h.right_click(Role::Button, "Sword");
    h.click(Role::Button, "Equip");
    assert_eq!(h.app().asked, [Asked("equip".into())]);
    assert!(!h.shows(Role::Button, "Inspect"));
    assert_eq!(h.focus(), on_slot());
}

#[test]
fn clicking_outside_returns_focus_to_the_opener() {
    let mut h = handle();
    h.right_click(Role::Button, "Sword");
    // Focus must have left the opener, or a press outside restores nothing and the test could not fail.
    key(&mut h, NamedKey::ArrowDown);
    assert_ne!(h.focus(), on_slot());
    h.click_at(Point::new(340.0, 220.0));
    assert!(!h.shows(Role::Button, "Inspect"));
    assert_eq!(h.focus(), on_slot());
    assert!(h.app().asked.is_empty());
}

/// The fallback rule: when the opener left the screen while its menu was open, focus goes to the first focusable
/// control of the scope (here "Drop") and the miss is counted, rather than vanishing.
#[test]
fn opener_removed_while_the_menu_is_open_falls_back_to_the_first_control() {
    let mut h = handle();
    h.right_click(Role::Button, "Sword");
    h.press_key(Key::Character('d'));
    assert!(!h.shows(Role::Button, "Sword"), "the opener is gone");
    assert!(h.shows(Role::Button, "Inspect"), "its menu is still open");
    key(&mut h, NamedKey::Escape);
    assert!(!h.shows(Role::Button, "Inspect"));
    assert_eq!(h.focus(), Focus::On(WidgetId::of(&Drop)));
    assert_eq!(h.restore_fallbacks(), 1);
}

/// A press outside that lands on another focusable control keeps that control's focus: the restore only fills a hole.
#[test]
fn restoration_does_not_steal_focus_from_where_the_player_went() {
    let mut h = handle();
    h.right_click(Role::Button, "Sword");
    key(&mut h, NamedKey::ArrowDown);
    // Tab order: Sword, the menu's items, Drop. Walk to Drop, then dismiss with Escape.
    for _ in 0..2 {
        key(&mut h, NamedKey::ArrowDown);
    }
    assert_eq!(h.focus(), Focus::On(WidgetId::of(&Drop)));
    key(&mut h, NamedKey::Escape);
    assert!(!h.shows(Role::Button, "Inspect"));
    assert_eq!(h.focus(), Focus::On(WidgetId::of(&Drop)));
}
