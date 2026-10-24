//! Behaviour tests for kit_state. One App model, real Store, real dispatch, real kit_view trees.

use kit_core::{Identity, MotionPref, Time};
use kit_state::{
    Act, Cx, Disclosure, DisclosureEv, Easing, Host, Ident, Intent, LateWhy, Life, Phase, Presence,
    Since, Store, dispatch,
};
use kit_view::{A11y, Bindings, Node, Role, Toggle, column, interactive, nothing};
use std::collections::HashSet;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Ask {
    Retire(u32),
}

#[derive(Debug)]
struct App {
    now: Time,
    motion: MotionPref,
    gone: HashSet<u32>,
    leaving: HashSet<u32>,
    asked: Vec<Ask>,
}
impl Default for App {
    fn default() -> Self {
        Self {
            now: Time::default(),
            motion: MotionPref::Full,
            gone: HashSet::new(),
            leaving: HashSet::new(),
            asked: Vec::new(),
        }
    }
}
impl Host for App {
    type Ask = Ask;
    fn now(&self) -> Time {
        self.now
    }
    fn motion(&self) -> MotionPref {
        self.motion
    }
    fn ask(&mut self, ask: Ask) {
        match ask {
            Ask::Retire(id) => {
                self.gone.insert(id);
            }
        }
        self.asked.push(ask);
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Row(u32);
impl Ident<App> for Row {
    fn life(&self, app: &App) -> Life {
        match (app.gone.contains(&self.0), app.leaving.contains(&self.0)) {
            (true, _) => Life::Over,
            (false, true) => Life::Leaving(Time(Duration::from_secs(9))),
            (false, false) => Life::Live,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Ev {
    Add(i32),
    Double,
    Retire,
}

/// A component: state, reducer and UI in one function. Returns the count and the intent that adds one.
fn counter(cx: &Cx<'_, App>, id: u32) -> (i32, Intent<App>) {
    let n = cx.state(
        &Row(id),
        || 0_i32,
        |n: &mut i32, key: &Row, ev: Ev, _: &App| match ev {
            Ev::Add(k) => {
                *n += k;
                Act::none()
            }
            Ev::Double => {
                *n *= 2;
                Act::none()
            }
            Ev::Retire => Act::ask(Ask::Retire(key.0)),
        },
    );
    (*n, n.send(Ev::Add(1)))
}

fn run(app: &mut App, store: &mut Store<App>, intents: impl IntoIterator<Item = Intent<App>>) {
    for intent in intents {
        dispatch(app, store, intent);
    }
}

#[test]
fn two_keys_of_one_declaration_are_independent() {
    let (mut app, mut store) = (App::default(), Store::new());
    let (_, add_a) = counter(&Cx::new(&app, &store), 1);
    let (_, _add_b) = counter(&Cx::new(&app, &store), 2);
    run(&mut app, &mut store, [add_a.clone(), add_a]);
    let cx = Cx::new(&app, &store);
    assert_eq!((counter(&cx, 1).0, counter(&cx, 2).0), (2, 0));
}

#[test]
fn one_closure_in_a_loop_is_one_declaration_and_equal_bodies_are_two() {
    let (app, store) = (App::default(), Store::new());
    let cx = Cx::new(&app, &store);
    for id in 0..5 {
        let _ = counter(&cx, id); // one `cx.state` call site, five keys
    }
    assert_eq!(store.len(), 5);
    // The declaration is the closure TYPE: a textually identical closure elsewhere does not share state.
    let twin = cx.state(
        &Row(0),
        || 7_i32,
        |n: &mut i32, _: &Row, _: (), _: &App| {
            *n += 1;
            Act::none()
        },
    );
    assert_eq!((*twin, store.len()), (7, 6));
}

#[test]
fn over_collects_state_and_late_intents_are_refused_and_counted() {
    let (mut app, mut store) = (App::default(), Store::new());
    let (_, add) = counter(&Cx::new(&app, &store), 1);
    app.gone.insert(1);
    store.collect(&app);
    assert_eq!(store.len(), 0);
    dispatch(&mut app, &mut store, add);
    assert_eq!(store.late().len(), 1);
    assert_eq!(store.late()[0].why, LateWhy::Gone);
    // Re-rendering a dead key shows the initial value and must not resurrect it in the store.
    assert_eq!(counter(&Cx::new(&app, &store), 1).0, 0);
    assert_eq!(store.len(), 0);
}

#[test]
fn a_step_whose_ask_ends_the_key_collects_it_in_the_same_dispatch() {
    let (mut app, mut store) = (App::default(), Store::new());
    let cx = Cx::new(&app, &store);
    let n = cx.state(
        &Row(3),
        || 5_i32,
        |n: &mut i32, key: &Row, _: Ev, _: &App| {
            *n += 1;
            Act::ask(Ask::Retire(key.0))
        },
    );
    let retire = n.send(Ev::Retire);
    dispatch(&mut app, &mut store, retire);
    assert_eq!((app.asked.clone(), store.len()), (vec![Ask::Retire(3)], 0));
}

/// A fn item is a ZST reducer too, and is one declaration however many places name it.
fn reducer(n: &mut i32, _: &Row, ev: Ev, _: &App) -> Act<Ask, Ev> {
    match ev {
        Ev::Add(k) => *n += k,
        Ev::Double | Ev::Retire => *n *= 2,
    }
    Act::none()
}
fn value(app: &App, store: &Store<App>) -> i32 {
    *Cx::new(app, store).state(&Row(1), || 0_i32, reducer)
}

#[test]
fn events_in_one_frame_apply_in_order_and_the_second_sees_the_first() {
    let (mut app, mut store) = (App::default(), Store::new());
    let n = Cx::new(&app, &store).state(&Row(1), || 0_i32, reducer);
    let add_then_double = Intent::All(vec![n.send(Ev::Add(3)), n.send(Ev::Double)]);
    let double_then_add = Intent::All(vec![n.send(Ev::Double), n.send(Ev::Add(3))]);
    dispatch(&mut app, &mut store, add_then_double);
    assert_eq!(value(&app, &store), 6); // (0 + 3) * 2
    dispatch(&mut app, &mut store, double_then_add);
    assert_eq!(value(&app, &store), 15); // 6 * 2 + 3
}

#[test]
fn leaving_keeps_state_inert_and_refuses_intents() {
    let (mut app, mut store) = (App::default(), Store::new());
    let (_, add) = counter(&Cx::new(&app, &store), 1);
    dispatch(&mut app, &mut store, add.clone());
    app.leaving.insert(1);
    dispatch(&mut app, &mut store, add);
    assert_eq!(
        store.late().iter().map(|l| l.why).collect::<Vec<_>>(),
        vec![LateWhy::Leaving]
    );
    assert_eq!((store.len(), counter(&Cx::new(&app, &store), 1).0), (1, 1));
}

#[test]
fn zero_duration_presence_is_shown_and_gone_immediately_and_never_nan() {
    let instant = Presence {
        enter: Duration::ZERO,
        exit: Duration::ZERO,
    };
    let t = |ms| Time(Duration::from_millis(ms));
    let opened_now = Disclosure::Open {
        since: Since::At(t(100)),
    };
    let closed_now = Disclosure::Closed {
        since: Since::At(t(100)),
    };
    // Regression (kit_widgets float.rs): "finished" must mean "started and elapsed", not "zero length".
    assert_eq!(opened_now.phase(instant, t(100)), Phase::Shown);
    assert_eq!(closed_now.phase(instant, t(100)), Phase::Gone);
    // Asked about a moment before the change (clock skew between host and model): still no NaN.
    assert_eq!(opened_now.phase(instant, t(50)), Phase::Shown);
    let slow = Presence {
        enter: Duration::from_millis(200),
        exit: Duration::from_millis(200),
    };
    assert_eq!(opened_now.phase(slow, t(100)), Phase::Entering(0.0));
    assert_eq!(opened_now.phase(slow, t(200)), Phase::Entering(0.5));
    assert_eq!(opened_now.phase(slow, t(300)), Phase::Shown);
    assert_eq!(closed_now.phase(slow, t(150)), Phase::Exiting(0.25));
    assert_eq!(closed_now.phase(slow, t(300)), Phase::Gone);
    assert_eq!(Disclosure::CLOSED.phase(slow, t(0)), Phase::Gone);
}

#[test]
fn toggle_twice_in_one_frame_is_two_transitions() {
    let mut d = Disclosure::CLOSED;
    let now = Time(Duration::from_millis(10));
    d.step(DisclosureEv::Toggle, now);
    d.step(DisclosureEv::Toggle, now);
    assert_eq!(
        d,
        Disclosure::Closed {
            since: Since::At(now)
        }
    );
    d.step(DisclosureEv::Close, Time(Duration::from_millis(99)));
    assert_eq!(
        d,
        Disclosure::Closed {
            since: Since::At(now)
        },
        "redundant Close keeps the original since"
    );
}

#[test]
fn easings_hit_their_endpoints_and_clamp() {
    for e in [Easing::Linear, Easing::OutQuad, Easing::OutCubic] {
        assert_eq!(
            (e.at(0.0), e.at(1.0), e.at(-3.0), e.at(7.0)),
            (0.0, 1.0, 0.0, 1.0)
        );
    }
    assert!(Easing::OutCubic.at(0.5) > Easing::OutQuad.at(0.5));
}

// ---- end to end: a todo list built from components and kit_view nodes ----

#[derive(Clone, Debug, PartialEq)]
struct Toggle1;

fn todo_item(cx: &Cx<'_, App>, id: u32, title: &str) -> Node<Intent<App>> {
    let done = cx.state(
        &Row(id),
        || false,
        |done: &mut bool, _: &Row, _: Toggle1, _: &App| {
            *done = !*done;
            Act::none()
        },
    );
    let mark = if *done { "x" } else { " " };
    interactive(
        Identity::Positional,
        Bindings::activate(done.send(Toggle1)),
        A11y::Labeled {
            role: Role::Button,
            label: format!("[{mark}] {title}"),
            toggle: Toggle::NotToggle,
        },
        nothing(),
    )
}

fn todo_list(cx: &Cx<'_, App>, titles: &[&str]) -> Node<Intent<App>> {
    column(
        titles
            .iter()
            .zip(0..)
            .map(|(title, id)| todo_item(cx, id, title)),
    )
}

fn labels(node: &Node<Intent<App>>, out: &mut Vec<String>) {
    if let Node::Interactive {
        a11y: A11y::Labeled { label, .. },
        ..
    } = node
    {
        out.push(label.clone());
    }
    node.children().iter().for_each(|child| labels(child, out));
}
fn press(node: &Node<Intent<App>>, wanted: &str) -> Option<Intent<App>> {
    match node {
        Node::Interactive {
            a11y: A11y::Labeled { label, .. },
            on,
            ..
        } if label == wanted => on.activate.clone(),
        _ => node
            .children()
            .iter()
            .find_map(|child| press(child, wanted)),
    }
}

#[test]
fn todo_component_renders_dispatches_and_rerenders() {
    let (mut app, mut store) = (App::default(), Store::new());
    let titles = ["milk", "eggs"];
    let frame = |app: &App, store: &Store<App>| {
        let node = todo_list(&Cx::new(app, store), &titles);
        let mut out = Vec::new();
        labels(&node, &mut out);
        (node, out)
    };
    let (node, seen) = frame(&app, &store);
    assert_eq!(seen, ["[ ] milk", "[ ] eggs"]);
    let toggle_eggs = press(&node, "[ ] eggs").expect("eggs is pressable");
    dispatch(&mut app, &mut store, toggle_eggs);
    assert_eq!(frame(&app, &store).1, ["[ ] milk", "[x] eggs"]);
    // The node tree from before the event is stale data: pressing it again toggles back (state is by key, not by node).
    dispatch(
        &mut app,
        &mut store,
        press(&node, "[ ] eggs").expect("stale node still addresses eggs"),
    );
    assert_eq!(frame(&app, &store).1, ["[ ] milk", "[ ] eggs"]);
}

// ---- late ring, lazy init, zoom ----

#[test]
fn late_log_is_a_bounded_ring_and_the_total_keeps_counting() {
    let (mut app, mut store) = (App::default(), Store::new());
    let (_, add) = counter(&Cx::new(&app, &store), 1);
    app.gone.insert(1);
    store.collect(&app);
    let sent = kit_state::LATE_LOG as u64 + 10;
    for _ in 0..sent {
        dispatch(&mut app, &mut store, add.clone());
    }
    assert_eq!(
        (store.late().len(), store.late_total()),
        (kit_state::LATE_LOG, sent)
    );
}

#[test]
fn init_runs_once_per_new_key_not_every_frame() {
    use std::cell::Cell;
    let (app, store) = (App::default(), Store::new());
    let cx = Cx::new(&app, &store);
    let calls = Cell::new(0);
    for _ in 0..3 {
        let _ = cx.state(
            &Row(1),
            || {
                calls.set(calls.get() + 1);
                0_i32
            },
            |_: &mut i32, _: &Row, _: (), _: &App| Act::none(),
        );
    }
    // The same closure site is one declaration, so only the first frame builds the value.
    assert_eq!(calls.get(), 1);
}

#[derive(Clone, Debug)]
struct Pair {
    left: i32,
    right: i32,
}
#[derive(Clone, Debug, PartialEq)]
enum PairEv {
    Left(i32),
    Right(i32),
}

#[test]
fn zoom_reads_a_part_and_routes_its_events_through_the_whole_reducer() {
    let (mut app, mut store) = (App::default(), Store::new());
    let frame = |app: &App, store: &Store<App>| {
        let cx = Cx::new(app, store);
        let pair = cx.state(
            &Row(1),
            || Pair { left: 0, right: 0 },
            |p: &mut Pair, _: &Row, ev: PairEv, _: &App| {
                match ev {
                    PairEv::Left(k) => p.left += k,
                    PairEv::Right(k) => p.right += k,
                }
                Act::none()
            },
        );
        let right = pair.zoom(|p: &Pair| &p.right, PairEv::Right);
        (*right, right.send(5), pair.send(PairEv::Left(1)))
    };
    let (_, add_right, add_left) = frame(&app, &store);
    dispatch(&mut app, &mut store, add_right);
    dispatch(&mut app, &mut store, add_left);
    assert_eq!(frame(&app, &store).0, 5); // the zoom sees only `right`; the left event did not leak into it
}
