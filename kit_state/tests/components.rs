//! kit_state v2: intent equality, value animation, interaction reads and the TextValue model.

use kit_core::kurbo::Vec2;
use kit_core::{MotionPref, Time, WidgetId};
use kit_state::{
    Act, Applied, Cx, Dir, Drag, History, Host, Ident, Interaction, Life, Part, Selection, Store,
    TextValue,
};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

struct App {
    now: Time,
    motion: MotionPref,
}
impl Host for App {
    type Ask = ();
    fn now(&self) -> Time {
        self.now
    }
    fn motion(&self) -> MotionPref {
        self.motion
    }
    fn ask(&mut self, (): ()) {}
}
fn at(ms: u64) -> App {
    App {
        now: Time(Duration::from_millis(ms)),
        motion: MotionPref::Full,
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Key(u32);
impl Ident<App> for Key {
    fn life(&self, _: &App) -> Life {
        Life::Live
    }
}

// ---- Intent equality ----

/// An event whose Debug hides a field, as a hand-written Debug often does.
#[derive(Clone, PartialEq)]
struct Nudge {
    shown: u8,
    hidden: u8,
}
impl core::fmt::Debug for Nudge {
    fn fmt(&self, out: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(out, "Nudge({})", self.shown)
    }
}

#[test]
fn intents_compare_by_value_not_by_debug_text() {
    let (app, store) = (at(0), Store::new());
    let cx = Cx::new(&app, &store);
    let n = cx.state(
        &Key(1),
        || 0_u8,
        |n: &mut u8, _: &Key, ev: Nudge, _: &App| {
            *n += ev.hidden;
            Act::none()
        },
    );
    let one = |hidden| n.send(Nudge { shown: 1, hidden });
    assert_eq!(one(5), one(5));
    // Regression: equality by Debug text called these equal although they apply different steps.
    assert_ne!(one(5), one(6));
    // Same key and event, another declaration: not the same intent.
    let twin = cx.state(
        &Key(1),
        || 0_u8,
        |n: &mut u8, _: &Key, ev: Nudge, _: &App| {
            *n += ev.hidden;
            Act::none()
        },
    );
    assert_ne!(
        one(5),
        twin.send(Nudge {
            shown: 1,
            hidden: 5
        })
    );
}

// ---- animate ----

#[test]
fn animate_retargets_from_where_it_is_and_zero_or_reduced_motion_snaps() {
    let store = Store::new();
    let glow = |app: &App, target: f32, ms: u64| {
        Cx::new(app, &store).animate(&Key(1), target, Duration::from_millis(ms))
    };
    assert_eq!(glow(&at(0), 0.0, 100), 0.0, "first sight shows the target");
    assert_eq!(
        glow(&at(10), 1.0, 100),
        0.0,
        "a retarget starts where the value is"
    );
    let mid = glow(&at(60), 1.0, 100);
    assert!(
        mid > 0.5 && mid < 1.0,
        "OutCubic is past halfway at 50%: {mid}"
    );
    // Reversal mid-flight: continuous at the instant of the change (no jump back to 1 or to 0).
    assert!((glow(&at(60), 0.0, 100) - mid).abs() < 1e-6);
    assert_eq!(glow(&at(160), 0.0, 100), 0.0);
    // Regression (presence float.rs): a zero-length tween is at its target from its first instant, never 0/0.
    assert_eq!(glow(&at(200), 1.0, 0), 1.0);
    let reduced = App {
        motion: MotionPref::Reduced,
        ..at(300)
    };
    assert_eq!(glow(&reduced, 0.0, 100), 0.0);
    // Two values of one key through Part do not retarget each other every frame.
    let app = at(400);
    let cx = Cx::new(&app, &store);
    let knob = cx.animate(&Part(Key(2), "knob"), 4.0, Duration::from_millis(50));
    let track = cx.animate(&Part(Key(2), "track"), 9.0, Duration::from_millis(50));
    assert_eq!((knob, track), (4.0, 9.0));
}

// ---- interaction reads: a test double and a component written against them ----

/// The plain-data double of the router's memory.
#[derive(Default)]
struct Snapshot {
    hovered: BTreeSet<WidgetId>,
    drags: BTreeMap<WidgetId, Drag>,
}
impl Interaction for Snapshot {
    fn hovered(&self, id: WidgetId) -> bool {
        self.hovered.contains(&id)
    }
    fn pressed(&self, id: WidgetId) -> bool {
        self.drags.contains_key(&id)
    }
    fn focused(&self, _: WidgetId) -> bool {
        false
    }
    fn drag(&self, id: WidgetId) -> Drag {
        self.drags.get(&id).copied().unwrap_or(Drag::Idle)
    }
    fn scroll(&self, _: WidgetId) -> kit_core::ScrollView {
        kit_core::ScrollView::Unseen
    }
}

/// A slider in a few lines: stored value, live drag preview, hover glow. Returns (shown value, glow).
fn slider(cx: &Cx<'_, App>, key: &Key) -> (f32, f32) {
    let value = cx.state(
        key,
        || 0.5_f32,
        |v: &mut f32, _: &Key, to: u32, _: &App| {
            *v = to as f32 / 100.0;
            Act::none()
        },
    );
    let shown = match cx.drag(key) {
        Drag::Idle => *value,
        Drag::Held { total, .. } => (*value + total.x as f32 / 200.0).clamp(0.0, 1.0),
    };
    let glow = cx.animate_bool(
        key,
        cx.hovered(key) || cx.pressed(key),
        Duration::from_millis(80),
    );
    (shown, glow)
}

#[test]
fn a_component_reads_drag_and_hover_through_the_key_it_keyed_its_node_with() {
    let store = Store::new();
    let key = Key(7);
    let app = at(0);
    assert_eq!(slider(&Cx::new(&app, &store), &key), (0.5, 0.0));
    // The router saw a node keyed `.key(&key)`, whose id is `WidgetId::of(&key)`.
    let mut input = Snapshot::default();
    input.hovered.insert(WidgetId::of(&key));
    input.drags.insert(
        WidgetId::of(&key),
        Drag::Held {
            delta: Vec2::new(10.0, 0.0),
            total: Vec2::new(50.0, 3.0),
        },
    );
    let app = at(100);
    let (shown, glow) = slider(&Cx::routed(&app, &store, &input), &key);
    assert_eq!((shown, glow), (0.75, 0.0), "glow starts animating from 0");
    let app = at(300);
    assert_eq!(slider(&Cx::routed(&app, &store, &input), &key).1, 1.0);
}

// ---- TextValue ----

const THUMB: &str = "\u{1F44D}\u{1F3FD}"; // thumbs up + skin tone: 8 bytes, one grapheme
const E_ACUTE: &str = "e\u{301}"; // e + combining acute: 3 bytes, one grapheme

#[test]
fn offsets_inside_a_utf8_sequence_or_a_grapheme_are_refused_and_change_nothing() {
    let mut t = TextValue::new(format!("a{THUMB}{E_ACUTE}"));
    let rev = t.rev();
    assert_eq!(t.splice(rev, 2..3, "x"), Applied::Misplaced); // inside the emoji's UTF-8
    assert_eq!(t.splice(rev, 5..5, "x"), Applied::Misplaced); // between emoji and skin tone (char boundary, not grapheme)
    assert_eq!(t.select(rev, Selection::caret(10)), Applied::Misplaced); // between e and its accent
    assert_eq!(
        t.splice(rev, core::ops::Range { start: 9, end: 1 }, ""),
        Applied::Misplaced
    ); // reversed
    assert_eq!(
        (t.text(), t.rev()),
        (format!("a{THUMB}{E_ACUTE}").as_str(), rev)
    );
}

#[test]
fn backspace_and_delete_remove_whole_graphemes() {
    let mut t = TextValue::new(format!("a{THUMB}{E_ACUTE}"));
    t.delete(t.rev(), Dir::Back);
    assert_eq!(t.text(), format!("a{THUMB}"));
    t.select(t.rev(), Selection::caret(1));
    t.delete(t.rev(), Dir::Forward);
    assert_eq!((t.text(), t.selection()), ("a", Selection::caret(1)));
    // At the edge there is nothing to delete: same text, same revision.
    let rev = t.rev();
    t.select(rev, Selection::caret(0));
    assert_eq!(t.delete(rev, Dir::Back), Applied::Ok(rev));
}

#[test]
fn an_edit_that_merges_a_grapheme_leaves_the_caret_on_a_boundary() {
    // Typing `e` in front of a lone combining accent makes one cluster: the caret must not land between them.
    let mut t = TextValue::new("\u{301}x");
    t.select(t.rev(), Selection::caret(0));
    t.type_text(t.rev(), "e");
    assert_eq!(t.selection(), Selection::caret(3));
    let before = t.rev();
    t.delete(before, Dir::Back);
    assert_eq!((t.text(), t.rev() > before), ("x", true));
}

#[test]
fn a_splice_for_an_older_revision_is_refused() {
    let mut t = TextValue::new("hello");
    let old = t.rev();
    t.type_text(old, "!");
    assert_eq!(
        t.splice(old, 0..1, "J"),
        Applied::Stale { current: t.rev() }
    );
    assert_eq!(t.text(), "hello!");
}

#[test]
fn typing_coalesces_per_word_and_any_caret_move_or_other_edit_ends_the_run() {
    let mut t = TextValue::new("");
    for c in ["h", "i", " ", "y", "o"] {
        t.type_text(t.rev(), c);
    }
    t.undo();
    assert_eq!(t.text(), "hi ", "the word after the space is its own step");
    t.undo();
    assert_eq!(t.text(), "");
    assert_eq!(t.undo(), History::Empty);
    t.redo();
    assert_eq!(t.text(), "hi ");
    // A caret move ends the run even if the caret comes back to the same place.
    t.select(t.rev(), Selection::caret(1));
    t.select(t.rev(), Selection::caret(3));
    t.type_text(t.rev(), "x");
    t.type_text(t.rev(), "y");
    t.undo();
    assert_eq!(t.text(), "hi ");
    // A new edit forgets what was undone.
    t.type_text(t.rev(), "z");
    assert_eq!(t.redo(), History::Empty);
}

#[test]
fn a_splice_that_deletes_under_the_selection_clamps_it_into_the_new_text() {
    let mut t = TextValue::new("hello world");
    t.select(
        t.rev(),
        Selection {
            anchor: 6,
            head: 11,
        },
    );
    t.splice(t.rev(), 3..11, "");
    assert_eq!((t.text(), t.selection()), ("hel", Selection::caret(3)));
    assert_eq!(t.selected(), "");
    // Text after a splice shifts; text before it stays.
    let mut t = TextValue::new("ab cd");
    t.select(t.rev(), Selection { anchor: 3, head: 5 });
    t.splice(t.rev(), 0..2, "xyz");
    assert_eq!(t.selected(), "cd");
}

#[test]
fn select_word_takes_the_word_under_the_offset() {
    let mut t = TextValue::new(format!("say {E_ACUTE}t\u{e9} now"));
    t.select_word(t.rev(), 7); // the `t`
    assert_eq!(t.selected(), format!("{E_ACUTE}t\u{e9}"));
    t.select_all();
    assert_eq!(t.selected(), t.text());
}
