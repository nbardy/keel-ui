//! Timers as data: a reducer returns `Act::wake(at, ev)`, the store keeps one wake-up per key, the host fires due
//! ones with `wake_due` and sleeps until `Store::next_due`.
use kit_core::Redraw;
use kit_core::{MotionPref, Time};
use kit_state::{Act, Cx, Host, Ident, Life, Store, dispatch, wake_due};
use std::collections::HashSet;
use std::time::Duration;

struct App {
    now: Time,
    gone: HashSet<u32>,
    fired: Vec<(u32, Time)>,
}
impl Host for App {
    type Ask = (u32, Time);
    fn now(&self) -> Time {
        self.now
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, ask: (u32, Time)) {
        self.fired.push(ask);
    }
}
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Row(u32);
impl Ident<App> for Row {
    fn life(&self, app: &App) -> Life {
        match app.gone.contains(&self.0) {
            true => Life::Over,
            false => Life::Live,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
enum Ev {
    /// Wake after this long.
    Arm(Duration),
    Rang,
}
fn ms(n: u64) -> Time {
    Time(Duration::from_millis(n))
}
/// Arm `row` for `after` from now (an alarm: the reducer schedules its own wake-up and reports when it rings).
fn arm(app: &mut App, store: &mut Store<App>, row: u32, after: u64) {
    let intent = {
        let cx = Cx::new(app, store);
        let alarm = cx.state(
            &Row(row),
            || (),
            |_: &mut (), key: &Row, ev: Ev, app: &App| match ev {
                Ev::Arm(after) => Act::wake(Time(app.now().0 + after), Ev::Rang),
                Ev::Rang => Act::ask((key.0, app.now())),
            },
        );
        alarm.send(Ev::Arm(Duration::from_millis(after)))
    };
    dispatch(app, store, intent);
}
fn at(app: &mut App, store: &mut Store<App>, now: u64) {
    app.now = ms(now);
    wake_due(app, store);
}
fn app() -> App {
    App {
        now: Time::default(),
        gone: HashSet::new(),
        fired: Vec::new(),
    }
}

/// The wake-up fires at its time and only once: firing removes it, so later frames (and a host calling `wake_due`
/// every frame) never deliver it twice; before its time nothing fires.
#[test]
fn a_timer_fires_once_at_its_time() {
    let (mut app, mut store) = (app(), Store::new());
    arm(&mut app, &mut store, 1, 300);
    at(&mut app, &mut store, 299);
    assert!(app.fired.is_empty(), "not before its time");
    at(&mut app, &mut store, 340);
    at(&mut app, &mut store, 400);
    assert_eq!(app.fired, [(1, ms(340))]);
    assert_eq!(store.next_due(), Redraw::Idle);
}

/// The host sleeps until the earliest wake-up across every key; re-arming a key replaces its wake-up rather than
/// adding a second one (a debounce restarts).
#[test]
fn the_earliest_due_across_keys_is_reported_and_rearming_replaces() {
    let (mut app, mut store) = (app(), Store::new());
    assert_eq!(store.next_due(), Redraw::Idle);
    arm(&mut app, &mut store, 1, 500);
    arm(&mut app, &mut store, 2, 200);
    assert_eq!(store.next_due(), Redraw::At(ms(200)));
    arm(&mut app, &mut store, 2, 800);
    assert_eq!(
        store.next_due(),
        Redraw::At(ms(500)),
        "row 2's 200 ms wake-up was replaced"
    );
    at(&mut app, &mut store, 1000);
    assert_eq!(
        app.fired,
        [(1, ms(1000)), (2, ms(1000))],
        "earliest first, each once"
    );
}

/// A wake-up belongs to its key: when the key's life ends the wake-up is collected with its state, so it neither
/// fires (as a `Late` refusal) nor keeps the host awake.
#[test]
fn a_timer_is_cancelled_when_its_key_ends() {
    let (mut app, mut store) = (app(), Store::new());
    arm(&mut app, &mut store, 1, 100);
    arm(&mut app, &mut store, 2, 300);
    app.gone.insert(1);
    store.collect(&app);
    assert_eq!(store.next_due(), Redraw::At(ms(300)));
    at(&mut app, &mut store, 1000);
    assert_eq!(app.fired, [(2, ms(1000))]);
    assert_eq!(
        store.late_total(),
        0,
        "a collected wake-up is not a refused intent"
    );
}
