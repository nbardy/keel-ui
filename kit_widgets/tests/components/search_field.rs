//! Search behaviour through real text routing; the debounce is a kit wake-up the test host fires per frame.
use crate::support::{Asked, Handle};
use kit_core::{Key, MotionPref, NamedKey, Theme, Time};
use kit_state::{Cx, Host, Ident, Intent, Life};
use kit_view::{Node, Role};
use kit_widgets::search_field::{SearchChanged, SearchField, search_field};
use std::cell::Cell;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Id;
impl Ident<App> for Id {
    fn life(&self, _: &App) -> Life {
        Life::Live
    }
}
impl From<SearchChanged<Id>> for Asked {
    fn from(change: SearchChanged<Id>) -> Self {
        Asked(change.text)
    }
}
struct App {
    now: Cell<Time>,
    asked: Vec<Asked>,
}
impl App {
    fn new() -> Self {
        Self {
            now: Cell::new(Time::default()),
            asked: Vec::new(),
        }
    }
    fn at(&self, millis: u64) {
        self.now.set(Time(Duration::from_millis(millis)));
    }
}
impl Host for App {
    type Ask = Asked;
    fn now(&self) -> Time {
        self.now.get()
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, ask: Asked) {
        self.asked.push(ask);
    }
}
fn view(cx: &Cx<'_, App>, theme: &Theme) -> Node<Intent<App>> {
    let quiet = Duration::from_millis(300);
    let p = SearchField {
        theme,
        key: Id,
        label: "Search ships",
        placeholder: "Name or class",
        quiet,
    };
    search_field(cx, p).pad(theme.spacing.sm)
}
/// One host frame at `millis`: due wake-ups fire (`kit_state::wake_due`), the view is rebuilt.
fn frame<V: Fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>>(h: &mut Handle<App, V>, millis: u64) {
    h.app().at(millis);
    h.tick(|_| {});
}
#[test]
fn golden_empty_and_typing() {
    let mut h = Handle::new(App::new(), (310.0, 90.0), view);
    h.assert_golden("search_field_empty");
    h.click(Role::TextBox, "Search ships");
    h.type_text("Kestrel");
    h.assert_golden("search_field_typing");
}
#[test]
fn rapid_typing_emits_one_debounced_intent_with_the_final_text() {
    let mut h = Handle::new(App::new(), (310.0, 110.0), view);
    h.click(Role::TextBox, "Search ships");
    for (at, value) in [(0, "K"), (100, "es"), (200, "trel")] {
        h.app().at(at);
        h.type_text(value);
        assert!(
            h.app().asked.is_empty(),
            "typing must wait for a quiet interval"
        );
    }
    frame(&mut h, 499);
    assert!(
        h.app().asked.is_empty(),
        "deadline is relative to the last change"
    );
    frame(&mut h, 500);
    assert_eq!(h.app().asked, [Asked("Kestrel".into())]);
    frame(&mut h, 1000);
    assert_eq!(
        h.app().asked,
        [Asked("Kestrel".into())],
        "a settled value emits only once"
    );
}
#[test]
fn selection_does_not_postpone_search_and_clear_debounces_the_empty_value() {
    let mut h = Handle::new(App::new(), (310.0, 110.0), view);
    assert!(!h.shows(Role::Button, "Clear Search ships"));
    h.click(Role::TextBox, "Search ships");
    h.type_text("Brig");
    h.app().at(299);
    h.press_key(Key::Named(NamedKey::ArrowLeft));
    frame(&mut h, 300);
    assert_eq!(h.app().asked, [Asked("Brig".into())]);
    h.click(Role::Button, "Clear Search ships");
    assert!(!h.shows(Role::Button, "Clear Search ships"));
    frame(&mut h, 599);
    assert_eq!(h.app().asked.len(), 1);
    frame(&mut h, 600);
    assert_eq!(h.app().asked, [Asked("Brig".into()), Asked(String::new())]);
}
/// Escape in the focused field empties it without host help (the field hears `Edit::Escape`), and the empty value
/// is asked for after the quiet interval like any other change. Before field events the router swallowed Escape.
#[test]
fn escape_in_the_field_clears_it() {
    let mut h = Handle::new(App::new(), (310.0, 110.0), view);
    h.click(Role::TextBox, "Search ships");
    h.type_text("Sloop");
    frame(&mut h, 300);
    h.press_key(Key::Named(NamedKey::Escape));
    assert!(
        !h.shows(Role::Button, "Clear Search ships"),
        "the text is gone"
    );
    frame(&mut h, 600);
    assert_eq!(h.app().asked, [Asked("Sloop".into()), Asked(String::new())]);
}
