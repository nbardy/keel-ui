//! Tabs: wrapped navigation, caller-owned selection and animated indicators.
use crate::support::Handle;
use kit_core::{Key, MotionPref, NamedKey, Theme, Time};
use kit_input::InputEvent;
use kit_state::{Cx, Host, Ident, Intent, Life};
use kit_view::{Node, Role, Toggle};
use kit_widgets::tabs::{Tabs, tabs};
use std::{cell::Cell, rc::Rc, time::Duration};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct Id;
impl Ident<App> for Id {
    fn life(&self, _: &App) -> Life {
        Life::Live
    }
}
#[derive(Clone, Debug)]
enum Ev {
    Choose(usize),
    Close(usize),
}
struct App {
    clock: Rc<Cell<Time>>,
    selected: usize,
    closed: Vec<usize>,
}
impl App {
    fn new(clock: Rc<Cell<Time>>) -> Self {
        Self {
            clock,
            selected: 0,
            closed: Vec::new(),
        }
    }
}
impl Host for App {
    type Ask = Ev;
    fn now(&self) -> Time {
        self.clock.get()
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, ev: Ev) {
        match ev {
            Ev::Choose(i) => self.selected = i,
            Ev::Close(i) => self.closed.push(i),
        }
    }
}
fn bar(cx: &Cx<'_, App>, theme: &Theme, labels: &[&str], closable: bool) -> Node<Intent<App>> {
    let choose = |i| Intent::Ask(Ev::Choose(i));
    let close = |i| Intent::Ask(Ev::Close(i));
    tabs(
        cx,
        Tabs {
            theme,
            key: Id,
            labels,
            selected: cx.app().selected,
            choose: &choose,
            close: closable.then_some(&close),
        },
    )
    .pad(theme.spacing.md)
}
fn view(cx: &Cx<'_, App>, theme: &Theme) -> Node<Intent<App>> {
    bar(cx, theme, &["Ship", "Crew", "Log"], true)
}
fn handle() -> Handle<App, impl Fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>> {
    Handle::new(App::new(Rc::default()), (320.0, 54.0), view)
}
fn selected(h: &Handle<App, impl Fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>>, label: &str) {
    for name in ["Ship", "Crew", "Log"] {
        assert_eq!(
            h.toggle_of(Role::Button, name),
            if name == label {
                Toggle::On
            } else {
                Toggle::Off
            }
        );
    }
}

#[test]
fn golden_idle_transition_and_selected() {
    let clock = Rc::new(Cell::new(Time::default()));
    let mut h = Handle::new(App::new(clock.clone()), (320.0, 54.0), view);
    h.assert_golden("tabs_idle");
    h.click(Role::Button, "Crew");
    clock.set(Time(Duration::from_millis(30)));
    h.input(InputEvent::Focus(true));
    h.assert_golden("tabs_transition");
    clock.set(Time(Duration::from_millis(120)));
    h.input(InputEvent::Focus(true));
    h.assert_golden("tabs_selected");
    // At the same settled time, the same value must produce exactly the same pixels.
    h.input(InputEvent::Focus(true));
    h.assert_golden("tabs_selected");
}

#[test]
fn keyboard_wraps_and_closing_selected_emits_only_close_intent() {
    let mut h = handle();
    h.press_key(Key::Named(NamedKey::ArrowLeft));
    selected(&h, "Ship");
    h.press_key(Key::Named(NamedKey::Tab));
    h.press_key(Key::Named(NamedKey::ArrowLeft));
    selected(&h, "Log");
    h.press_key(Key::Named(NamedKey::ArrowRight));
    selected(&h, "Ship");
    h.press_key(Key::Named(NamedKey::ArrowRight));
    selected(&h, "Crew");
    h.click(Role::Button, "Close Crew");
    assert_eq!(h.app().closed, [1]);
    assert_eq!(h.app().selected, 1);
    selected(&h, "Crew");
    assert!(
        h.shows(Role::Button, "Crew"),
        "the caller decides whether to remove the closed tab"
    );
    h.click(Role::Button, "Close Log");
    selected(&h, "Crew");
    assert_eq!(h.app().closed, [1, 2]);
    h.click(Role::Button, "Log");
    selected(&h, "Log");
    h.press_key(Key::Named(NamedKey::ArrowRight));
    selected(&h, "Log"); // Pointer activation relinquishes keyboard focus.
    h.press_key(Key::Named(NamedKey::Tab));
    h.press_key(Key::Named(NamedKey::ArrowRight));
    selected(&h, "Ship");
}

#[test]
fn one_and_two_tabs_wrap_without_close_buttons() {
    for labels in [&["Ship"][..], &["Ship", "Crew"][..]] {
        let mut h = Handle::new(App::new(Rc::default()), (320.0, 54.0), |cx, theme| {
            bar(cx, theme, labels, false)
        });
        h.press_key(Key::Named(NamedKey::Tab));
        h.press_key(Key::Named(NamedKey::ArrowLeft));
        assert_eq!(
            h.toggle_of(Role::Button, labels[labels.len() - 1]),
            Toggle::On
        );
        h.press_key(Key::Named(NamedKey::ArrowRight));
        assert_eq!(h.toggle_of(Role::Button, "Ship"), Toggle::On);
        assert!(!h.shows(Role::Button, "Close Ship"));
    }
}

#[test]
#[should_panic(expected = "tabs: invalid selection")]
fn empty_bar_is_rejected() {
    Handle::new(App::new(Rc::default()), (320.0, 54.0), |cx, theme| {
        bar(cx, theme, &[], false)
    });
}
#[test]
#[should_panic(expected = "tabs: invalid selection")]
fn invalid_selection_is_rejected() {
    let mut app = App::new(Rc::default());
    app.selected = 3;
    Handle::new(app, (320.0, 54.0), view);
}

#[test]
fn long_bar_keys_do_not_collide_with_navigation_keys() {
    let labels = [
        "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten",
    ];
    let mut h = Handle::new(App::new(Rc::default()), (1000.0, 54.0), |cx, theme| {
        bar(cx, theme, &labels, true)
    });
    h.press_key(Key::Named(NamedKey::Tab));
    h.press_key(Key::Named(NamedKey::ArrowLeft));
    assert_eq!(h.app().selected, 9);
    h.click(Role::Button, "Close Ten");
    assert_eq!(h.app().closed, [9]);
    assert_eq!(h.app().selected, 9);
}
