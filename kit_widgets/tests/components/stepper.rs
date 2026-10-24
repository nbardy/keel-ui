//! Stepper goldens and the hold-repeat, driven like a host: real router presses, `Handle::tick` frames that fire the
//! kit wake-ups (`kit_state::wake_due`) the stepper schedules.
use crate::support::{Handle, TestApp};
use kit_core::Redraw;
use kit_core::kurbo::Point;
use kit_core::{Key, NamedKey, Theme, Time};
use kit_input::{InputEvent, Phase, PointerEvent, PointerId, PointerKind};
use kit_state::{Cx, Host, Ident, Intent, Life};
use kit_view::{Node, Role, SemanticState};
use kit_widgets::stepper::{Stepper, stepper};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Id;
impl<A> Ident<A> for Id {
    fn life(&self, _: &A) -> Life {
        Life::Live
    }
}
fn view<A: Host>(cx: &Cx<'_, A>, th: &Theme, initial: i64) -> Node<Intent<A>> {
    let p = Stepper {
        theme: th,
        key: Id,
        label: "Crew",
        initial,
        min: 0,
        max: 20,
        step: 1,
    };
    stepper(cx, p).pad(th.spacing.sm)
}
#[test]
fn golden_idle_minimum_and_maximum() {
    for (initial, name) in [
        (0, "stepper_min"),
        (10, "stepper_idle"),
        (20, "stepper_max"),
    ] {
        Handle::new(TestApp::new(), (200.0, 48.0), move |cx, th| {
            view(cx, th, initial)
        })
        .assert_golden(name);
    }
}

type H = Handle<TestApp, fn(&Cx<'_, TestApp>, &Theme) -> Node<Intent<TestApp>>>;
fn handle() -> H {
    Handle::new(TestApp::new(), (200.0, 48.0), |cx, th| view(cx, th, 0))
}
fn at(h: &mut H, ms: u64) {
    h.tick(|app| app.now = Time(Duration::from_millis(ms)));
}
fn pointer(h: &mut H, phase: Phase, label: &str, ms: u64) {
    at(h, ms);
    let laid_at: Point = h.center(Role::Button, label);
    h.input(InputEvent::Pointer(PointerEvent {
        id: PointerId(1),
        kind: PointerKind::Mouse,
        phase,
        at: laid_at,
        pressure: 1.0,
        button: kit_input::PointerButton::Primary,
    }));
}
fn value(h: &H) -> i64 {
    let shown = h
        .texts()
        .into_iter()
        .find_map(|t| t.strip_prefix("Crew: ").map(str::to_owned));
    shown
        .expect("the value is shown")
        .parse()
        .expect("numeric value")
}

#[test]
fn golden_held() {
    let mut h = handle();
    pointer(&mut h, Phase::Down, "Crew increase", 0);
    at(&mut h, 500);
    assert_eq!(value(&h), 2, "the press and the 400 ms repeat");
    h.assert_golden("stepper_held");
}

#[test]
fn holding_plus_accelerates_and_stops_at_max_without_an_extra_release_step() {
    let mut h = handle();
    pointer(&mut h, Phase::Down, "Crew increase", 0);
    assert_eq!(value(&h), 1);
    assert_eq!(
        h.store().next_due(),
        Redraw::At(Time(Duration::from_millis(400))),
        "the host may sleep until the repeat"
    );
    at(&mut h, 399);
    assert_eq!(value(&h), 1, "initial delay");
    at(&mut h, 400);
    assert_eq!(value(&h), 2);
    at(&mut h, 1000);
    let slow_start = value(&h);
    at(&mut h, 1300);
    let slow = value(&h) - slow_start;
    at(&mut h, 1600);
    let fast_start = value(&h);
    at(&mut h, 1900);
    assert!(
        value(&h) - fast_start > slow,
        "later equal time window repeats faster"
    );
    at(&mut h, 10_000);
    assert_eq!(value(&h), 20, "sparse frames catch up and clamp");
    assert_eq!(
        h.store().next_due(),
        Redraw::Idle,
        "holding at max schedules nothing more"
    );
    at(&mut h, 20_000);
    assert_eq!(value(&h), 20, "holding at max stops");
    assert_eq!(
        h.state_of(Role::Button, "Crew increase"),
        SemanticState::Disabled,
        "plus is disabled at max"
    );
    pointer(&mut h, Phase::Up, "Crew increase", 20_001);
    assert_eq!(value(&h), 20);
    pointer(&mut h, Phase::Down, "Crew decrease", 20_010);
    pointer(&mut h, Phase::Up, "Crew decrease", 20_011);
    assert_eq!(value(&h), 19, "release adds no extra step after a press");
    assert_eq!(
        h.store().next_due(),
        Redraw::Idle,
        "a release cancels the pending repeat"
    );
    at(&mut h, 30_000);
    assert_eq!(value(&h), 19, "released holds stop");
    h.press_key(Key::Named(NamedKey::Tab));
    h.press_key(Key::Named(NamedKey::Enter));
    assert_eq!(value(&h), 18, "Enter on the focused minus steps once");
    at(&mut h, 40_000);
    assert_eq!(value(&h), 18, "and does not repeat");
}
