//! Slider goldens and the clamp/step rule under a real captured drag through the router.
use core::ops::RangeInclusive;
use kit_core::kurbo::Point;
use kit_core::{Key, MotionPref, NamedKey, Theme, Time};
use kit_input::{InputEvent, Phase, PointerEvent, PointerId, PointerKind};
use kit_state::{Cx, Host, Intent};
use kit_test::Handle;
use kit_view::{Node, RangeValue, Role, SemanticValue};
use kit_widgets::slider::Step;
use kit_widgets::{Slider, slider};

struct App {
    value: f64,
    range: RangeInclusive<f64>,
    step: f64,
}
impl Host for App {
    type Ask = f64;
    fn now(&self) -> Time {
        Time::default()
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, value: f64) {
        self.value = value;
    }
}
type View = fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>;
fn view(cx: &Cx<'_, App>, theme: &Theme) -> Node<Intent<App>> {
    let app = cx.app();
    let p = Slider {
        theme,
        key: "volume",
        label: "Volume",
        value: app.value,
        range: app.range.clone(),
        step: Step::By(app.step),
        on_change: Intent::Ask,
    };
    slider(p).pad(8.0)
}
fn handle(range: RangeInclusive<f64>, step: f64) -> Handle<App, View> {
    let app = App {
        value: 40.0,
        range,
        step,
    };
    Handle::new(app, (200.0, 40.0), view)
}
/// The track spans screen x 8..108; its 8 px handle radius leaves 84 px of travel from x 16. y 18 is on the track.
fn mouse(h: &mut Handle<App, View>, phase: Phase, x: f64, y: f64) {
    h.input(InputEvent::Pointer(PointerEvent {
        id: PointerId(1),
        kind: PointerKind::Mouse,
        phase,
        at: Point::new(x, y),
        pressure: 1.0,
    }));
}

#[test]
fn golden_idle_and_dragging() {
    let mut h = handle(0.0..=100.0, 5.0);
    h.assert_golden("slider_idle");
    mouse(&mut h, Phase::Down, 58.0, 18.0);
    mouse(&mut h, Phase::Move, 88.0, 18.0);
    h.assert_golden("slider_dragging");
}

#[test]
fn drag_beyond_the_track_clamps_to_the_range_and_respects_step() {
    let mut h = handle(0.0..=100.0, 5.0);
    mouse(&mut h, Phase::Down, 58.0, 18.0);
    assert_eq!(h.app().value, 50.0, "a press jumps to the pointer");
    // 37.3% of the travel is 37.3, which steps to 35.
    mouse(&mut h, Phase::Move, 16.0 + 84.0 * 0.373, 18.0);
    assert_eq!(h.app().value, 35.0);
    mouse(&mut h, Phase::Move, 500.0, 18.0);
    assert_eq!(
        h.app().value,
        100.0,
        "far right of the track clamps to the end"
    );
    mouse(&mut h, Phase::Move, -200.0, 300.0);
    assert_eq!(
        h.app().value,
        0.0,
        "far left (and below) clamps to the start"
    );
    mouse(&mut h, Phase::Up, 500.0, 300.0);
    assert_eq!(
        h.app().value,
        100.0,
        "the release outside the track still arrives"
    );
    mouse(&mut h, Phase::Move, 30.0, 18.0);
    assert_eq!(h.app().value, 100.0, "no capture after release");

    // Beyond the end of 0..=10 by 3 the value is the last step, 9: clamp first, then snap (egui's order).
    let mut h = handle(0.0..=10.0, 3.0);
    mouse(&mut h, Phase::Down, 58.0, 18.0);
    mouse(&mut h, Phase::Move, 400.0, 18.0);
    assert_eq!(h.app().value, 9.0);
    // 0..=10 by 4 snaps 10 to 12, which egui clamps back on its next frame: the range wins at once.
    let mut h = handle(0.0..=10.0, 4.0);
    mouse(&mut h, Phase::Down, 58.0, 18.0);
    mouse(&mut h, Phase::Move, 400.0, 18.0);
    assert_eq!(h.app().value, 10.0);
}

/// Arrow keys reach the slider only while it has focus (Tab gives it), move it one step as egui's `kb_step` (clamped
/// at the ends like a drag), and the semantics announce a `slider` with its value and bounds for the mirror.
#[test]
fn focused_arrows_step_the_value_and_clamp_at_the_ends() {
    let mut h = handle(0.0..=100.0, 5.0);
    h.press_key(Key::Named(NamedKey::ArrowRight));
    assert_eq!(h.app().value, 40.0, "unfocused: the arrows are the world's");
    h.press_key(Key::Named(NamedKey::Tab));
    h.press_key(Key::Named(NamedKey::ArrowRight));
    assert_eq!(h.app().value, 45.0);
    h.press_key(Key::Named(NamedKey::ArrowLeft));
    h.press_key(Key::Named(NamedKey::ArrowLeft));
    assert_eq!(h.app().value, 35.0);
    assert_eq!(
        h.semantic(Role::Slider, "Volume").value,
        SemanticValue::Range(RangeValue {
            now: 35.0,
            min: 0.0,
            max: 100.0
        })
    );
    let mut h = handle(0.0..=10.0, 4.0);
    h.press_key(Key::Named(NamedKey::Tab));
    for _ in 0..3 {
        h.press_key(Key::Named(NamedKey::ArrowRight));
    }
    assert_eq!(
        h.app().value,
        10.0,
        "40 clamps to 10; +4 snaps to 12 and clamps back to 10"
    );
}
