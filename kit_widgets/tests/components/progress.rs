//! Progress rejects invalid values; spinner geometry follows host time unless reduced motion is requested.
use kit_core::kurbo::BezPath;
use kit_core::{MotionPref, ShapeSpec, Theme, Time};
use kit_state::{Cx, Host, Intent, Store};
use kit_test::{Handle, TestApp, theme};
use kit_view::{Node, column};
use kit_widgets::progress::{ProgressBar, ProgressError, Spinner, progress, spinner};
use std::time::Duration;

fn bar<Msg>(theme: &Theme, value: f64, label: Option<&str>) -> Node<Msg> {
    let mut props = ProgressBar::new(theme, value).expect("valid progress");
    props.label = label;
    progress(props)
}

fn determinate(_: &Cx<'_, TestApp>, theme: &Theme) -> Node<Intent<TestApp>> {
    column([
        bar(theme, 0.0, Some("Waiting")),
        bar(theme, 0.5, Some("Loading 50%")),
        bar(theme, 1.0, Some("Complete")),
        bar(theme, 0.75, None),
    ])
    .gap(8.0)
    .pad(8.0)
    .fill_width()
}

#[test]
fn golden_empty_partial_complete_and_unlabeled() {
    Handle::new(TestApp::new(), (220.0, 120.0), determinate).assert_golden("progress_bars");
}

#[test]
fn out_of_range_and_nonfinite_progress_are_errors_instead_of_silent_clamps() {
    let theme = theme();
    for value in [-0.01, 1.01, f64::NEG_INFINITY, f64::INFINITY, f64::NAN] {
        assert!(
            matches!(ProgressBar::new(&theme, value), Err(ProgressError)),
            "invalid progress {value:?} must not become an apparently valid bar"
        );
    }
    for value in [0.0, 0.5, 1.0] {
        assert!(
            ProgressBar::new(&theme, value).is_ok(),
            "valid endpoint or fraction {value}"
        );
    }
}

struct Clock {
    now: Time,
    motion: MotionPref,
}
impl Host for Clock {
    type Ask = ();
    fn now(&self) -> Time {
        self.now
    }
    fn motion(&self) -> MotionPref {
        self.motion
    }
    fn ask(&mut self, (): ()) {}
}
fn clock(millis: u64, motion: MotionPref) -> Clock {
    Clock {
        now: Time(Duration::from_millis(millis)),
        motion,
    }
}
fn spinner_path(app: &Clock) -> BezPath {
    let store = Store::new();
    let node = spinner(&Cx::new(app, &store), Spinner { theme: &theme() });
    let Node::Shape {
        shape: ShapeSpec::Path { path, .. },
        ..
    } = node
    else {
        panic!("spinner must draw an arc");
    };
    path
}

#[test]
fn reduced_motion_freezes_a_visible_spinner_while_full_motion_changes_with_time() {
    let first = spinner_path(&clock(500, MotionPref::Reduced));
    let later = spinner_path(&clock(1700, MotionPref::Reduced));
    assert_eq!(
        first, later,
        "reduced motion must ignore advancing host time"
    );
    assert!(
        first.elements().len() > 1,
        "the static spinner must contain an arc, not just a point"
    );
    assert_ne!(
        spinner_path(&clock(500, MotionPref::Full)),
        spinner_path(&clock(1700, MotionPref::Full)),
        "full motion must actually animate"
    );
}

fn spinning(cx: &Cx<'_, Clock>, theme: &Theme) -> Node<Intent<Clock>> {
    column([spinner(cx, Spinner { theme })]).pad(8.0)
}

#[test]
fn golden_spinner_two_phases_and_reduced_motion() {
    Handle::new(clock(500, MotionPref::Full), (40.0, 40.0), spinning)
        .assert_golden("progress_spinner_first");
    Handle::new(clock(1700, MotionPref::Full), (40.0, 40.0), spinning)
        .assert_golden("progress_spinner_later");
    Handle::new(clock(500, MotionPref::Reduced), (40.0, 40.0), spinning)
        .assert_golden("progress_spinner_reduced");
}
