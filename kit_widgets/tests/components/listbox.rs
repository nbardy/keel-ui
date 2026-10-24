//! Keyed ranges, goldens, and the native keyboard/modifier path (real keys, shift/cmd-click through the router).
use crate::support::Handle;
use kit_core::{Key, MotionPref, NamedKey, ScrollView, Theme, Time};
use kit_input::{InputEvent, Modifiers};
use kit_state::{Cx, Host, Ident, Intent, Life};
use kit_view::{A11y, Bindings, Node, Role, Toggle, column, interactive, row, text};
use kit_widgets::listbox::{Gesture, Input, Listbox, Mode, Selection, listbox};
use std::{cell::Cell, rc::Rc, time::Duration};

const ITEMS: [(u8, &str); 5] = [
    (10, "Sloop"),
    (20, "Brig"),
    (30, "Bark"),
    (40, "Cutter"),
    (50, "Dhow"),
];
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Id;
impl Ident<App> for Id {
    fn life(&self, _: &App) -> Life {
        Life::Live
    }
}
#[derive(Clone, Debug)]
enum Ask {
    Pick(Input<u8>, Gesture),
    Reorder,
    Tick,
}
struct App {
    items: Vec<(u8, &'static str)>,
    selection: Selection<u8>,
    mode: Mode,
    now: Time,
    scroll: Cell<ScrollView>,
}
impl App {
    fn new(mode: Mode) -> Self {
        Self {
            items: ITEMS.to_vec(),
            selection: Selection::default(),
            mode,
            now: Time::default(),
            scroll: Cell::new(ScrollView::Unseen),
        }
    }
}
impl Host for App {
    type Ask = Ask;
    fn now(&self) -> Time {
        self.now
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, ask: Ask) {
        match ask {
            Ask::Pick(input, gesture) => {
                self.selection
                    .input(&self.items, self.mode, gesture, input, self.now)
            }
            Ask::Reorder => self.items.swap(0, 1),
            Ask::Tick => self.now.0 += Duration::from_secs(2),
        }
    }
}
fn view(cx: &Cx<'_, App>, theme: &Theme) -> Node<Intent<App>> {
    let app = cx.app();
    app.scroll.set(cx.scroll(&Id));
    listbox(
        cx,
        Listbox {
            theme,
            key: Id,
            label: "Ships",
            items: &app.items,
            selection: &app.selection,
            choose: Rc::new(|input, gesture| Intent::Ask(Ask::Pick(input, gesture))),
            height: 140.0,
        },
    )
    .pad(theme.spacing.sm)
}
/// Fixture controls change the app outside the list (reorder items, advance the clock, select the end).
fn fixture(cx: &Cx<'_, App>, theme: &Theme) -> Node<Intent<App>> {
    let button = |name: &'static str, ask| {
        interactive(
            kit_core::WidgetId::of(&name),
            Bindings::activate(Intent::Ask(ask)),
            A11y::Labeled {
                role: Role::Button,
                label: name.into(),
                toggle: Toggle::NotToggle,
            },
            text(
                name,
                kit_view::TextStyle::new(theme.type_scale.label as f32, theme.colors.text),
            )
            .pad(theme.spacing.sm),
        )
    };
    column([
        view(cx, theme),
        row([button("Reorder", Ask::Reorder)]),
        row([
            button("End", Ask::Pick(Input::End, Gesture::Replace)),
            button("Tick", Ask::Tick),
        ]),
    ])
}
#[test]
fn golden_idle_and_selected() {
    let mut h = Handle::new(App::new(Mode::Single), (220.0, 180.0), view);
    h.assert_golden("listbox_idle");
    h.click(Role::Option, "Brig");
    assert_eq!(h.app().selection.selected(), [20]);
    h.assert_golden("listbox_selected");
}
#[test]
fn shift_range_tracks_the_data_anchor_after_reordering() {
    let mut h = Handle::new(App::new(Mode::Multiple), (300.0, 260.0), fixture);
    h.click(Role::Option, "Brig");
    h.click(Role::Button, "Reorder"); // Brig moved from index 1 to index 0.
    let held = |shift, ctrl| {
        InputEvent::Modifiers(Modifiers {
            shift,
            ctrl,
            ..Modifiers::default()
        })
    };
    h.input(held(true, false));
    h.click(Role::Option, "Cutter");
    assert_eq!(
        h.app().selection.selected(),
        [20, 10, 30, 40],
        "range must start at Brig's new position"
    );
    assert_eq!(h.toggle_of(Role::Option, "Sloop"), Toggle::On);
    h.click(Role::Option, "Sloop");
    assert_eq!(
        h.app().selection.selected(),
        [20, 10],
        "contracting preserves the original anchor"
    );
    h.input(held(false, true));
    h.click(Role::Option, "Bark");
    h.click(Role::Option, "Brig");
    assert_eq!(h.app().selection.selected(), [10, 30]);
}
#[test]
fn golden_multiple_selection() {
    let mut app = App::new(Mode::Multiple);
    app.ask(Ask::Pick(Input::Pick(20), Gesture::Replace));
    let mut h = Handle::new(app, (220.0, 180.0), view);
    h.input(InputEvent::Modifiers(Modifiers {
        shift: true,
        ..Modifiers::default()
    }));
    h.click(Role::Option, "Cutter");
    h.assert_golden("listbox_multiple");
}
#[test]
fn arrows_home_end_clamp_and_single_mode_ignores_multi_gestures() {
    let mut app = App::new(Mode::Single);
    for (input, expected) in [
        (Input::Down, 10),
        (Input::Down, 20),
        (Input::Up, 10),
        (Input::Up, 10),
        (Input::End, 50),
        (Input::Down, 50),
        (Input::Home, 10),
    ] {
        app.ask(Ask::Pick(input, Gesture::Extend));
        assert_eq!(app.selection.selected(), [expected]);
    }
    app.ask(Ask::Pick(Input::Pick(30), Gesture::Toggle));
    app.ask(Ask::Pick(Input::Pick(30), Gesture::Toggle));
    assert_eq!(app.selection.selected(), [30]);
}
#[test]
fn type_ahead_extends_cycles_and_expires_using_host_time() {
    let mut app = App::new(Mode::Single);
    let type_text =
        |app: &mut App, s: &str| app.ask(Ask::Pick(Input::Text(s.into()), Gesture::Replace));
    type_text(&mut app, "B");
    assert_eq!(app.selection.selected(), [20]);
    type_text(&mut app, "b");
    assert_eq!(
        app.selection.selected(),
        [30],
        "repeated initial cycles matching names"
    );
    type_text(&mut app, "a");
    assert_eq!(
        app.selection.selected(),
        [30],
        "prefix continuation includes current row"
    );
    app.ask(Ask::Tick);
    type_text(&mut app, "d");
    assert_eq!(
        app.selection.selected(),
        [50],
        "expired prefix starts new search"
    );
    type_text(&mut app, "z");
    assert_eq!(
        app.selection.selected(),
        [50],
        "unmatched prefix preserves selection"
    );
}
#[test]
fn additive_range_retains_disjoint_selection_and_removed_anchor_restarts() {
    let mut app = App::new(Mode::Multiple);
    for (input, gesture) in [
        (Input::Pick(10), Gesture::Replace),
        (Input::Pick(40), Gesture::Toggle),
        (Input::Pick(50), Gesture::AddRange),
    ] {
        app.ask(Ask::Pick(input, gesture));
    }
    assert_eq!(app.selection.selected(), [10, 40, 50]);
    app.items.retain(|(id, _)| *id != 40);
    app.ask(Ask::Pick(Input::Pick(20), Gesture::Extend));
    assert_eq!(app.selection.selected(), [20]);
    app.items.clear();
    app.ask(Ask::Pick(Input::Down, Gesture::Extend));
    assert!(app.selection.selected().is_empty());
}
/// The arrows reach the focused row's key binding and move the selection; focus follows the cursor (a focus
/// claim), so Enter then picks the row the arrows reached, not the one Tab first focused. Typed letters search.
#[test]
fn arrows_move_the_selection_and_focus_follows_it() {
    let mut h = Handle::new(App::new(Mode::Multiple), (220.0, 180.0), view);
    h.press_key(Key::Named(NamedKey::Tab));
    h.press_key(Key::Named(NamedKey::ArrowDown));
    h.press_key(Key::Named(NamedKey::ArrowDown));
    assert_eq!(h.app().selection.selected(), [20]);
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    h.press_key_with(Key::Named(NamedKey::Space), ctrl);
    assert!(
        h.app().selection.selected().is_empty(),
        "Ctrl+Space toggles the focused row, Brig, off"
    );
    h.press_key(Key::character('c'));
    assert_eq!(
        h.app().selection.selected(),
        [40],
        "type-ahead from the keyboard"
    );
}
#[test]
fn selected_row_is_revealed_by_the_scroll_core() {
    let mut app = App::new(Mode::Single);
    app.items.extend((60..90).map(|id| (id, "Other ship")));
    let mut h = Handle::new(app, (300.0, 260.0), fixture);
    h.click(Role::Button, "End");
    for _ in 0..4 {
        h.click(Role::Button, "Tick");
    }
    h.input(InputEvent::Focus(true));
    match h.app().scroll.get() {
        ScrollView::Seen {
            offset,
            viewport,
            content,
        } => {
            assert!(offset.y > 0.0);
            assert!(
                (offset.y + viewport.height - content.height).abs() < 1.0,
                "last selection must be visible"
            );
        }
        ScrollView::Unseen => panic!("scroll core was never measured"),
    }
}
#[test]
#[should_panic(expected = "listbox: duplicate item keys")]
fn duplicate_data_keys_are_rejected() {
    Selection::default().input(
        &[(1, "First"), (1, "Second")],
        Mode::Single,
        Gesture::Replace,
        Input::Home,
        Time::default(),
    );
}
#[test]
#[should_panic(expected = "listbox: Pick key is absent")]
fn picking_an_absent_key_is_rejected() {
    Selection::default().input(
        &ITEMS,
        Mode::Single,
        Gesture::Replace,
        Input::Pick(99),
        Time::default(),
    );
}

#[test]
fn pointer_selection_resets_type_ahead() {
    let mut app = App::new(Mode::Single);
    app.ask(Ask::Pick(Input::Text("c".into()), Gesture::Replace));
    app.ask(Ask::Pick(Input::Pick(10), Gesture::Replace));
    app.ask(Ask::Pick(Input::Text("b".into()), Gesture::Replace));
    assert_eq!(
        app.selection.selected(),
        [20],
        "a click must end the old search prefix"
    );
}
#[test]
#[should_panic(expected = "listbox: Pick key is absent")]
fn empty_lists_reject_picks() {
    Selection::<u8>::default().input(
        &[],
        Mode::Single,
        Gesture::Replace,
        Input::Pick(1),
        Time::default(),
    );
}
