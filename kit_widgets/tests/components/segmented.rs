//! Segmented selection: real routed input and host-clock-driven background movement.
use kit_core::{Key, MotionPref, NamedKey, Theme, Time};
use kit_state::{Cx, Host, Ident, Intent, Life};
use kit_test::Handle;
use kit_view::{Node, Placement, Role, Toggle};
use kit_widgets::segmented::{Segmented, segmented};
use std::{cell::Cell, time::Duration};

struct App {
    now: Cell<Time>,
    indicator_x: Cell<f64>,
    selected: usize,
    motion: MotionPref,
}
impl Host for App {
    type Ask = usize;
    fn now(&self) -> Time {
        self.now.get()
    }
    fn motion(&self) -> MotionPref {
        self.motion
    }
    fn ask(&mut self, selected: usize) {
        self.selected = selected;
    }
}
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Id;
impl Ident<App> for Id {
    fn life(&self, _: &App) -> Life {
        Life::Live
    }
}
const OPTIONS: [&str; 3] = ["Map", "Crew", "Cargo"];
fn view(cx: &Cx<'_, App>, theme: &Theme) -> Node<Intent<App>> {
    let choose = Intent::Ask;
    let node = segmented(
        cx,
        Segmented {
            theme,
            key: Id,
            options: &OPTIONS,
            selected: cx.app().selected,
            segment_width: theme.controls.hero.0,
            choose: &choose,
        },
    );
    // Observe the actual style sent to layout without driving the animation.
    let Node::Panel { children, .. } = &node else {
        panic!("segmented root panel");
    };
    let Node::Panel { style, .. } = &children[0] else {
        panic!("selection background");
    };
    let Placement::Anchored(anchor) = &style.placement else {
        panic!("moving background anchor");
    };
    cx.app().indicator_x.set(anchor.offset.x);
    node.pad(8.0)
}
fn handle(motion: MotionPref) -> Handle<App, impl Fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>> {
    let app = App {
        now: Cell::new(Time::default()),
        indicator_x: Cell::new(0.0),
        selected: 0,
        motion,
    };
    Handle::new(app, (240.0, 64.0), view)
}
fn key(h: &mut Handle<App, impl Fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>>, key: NamedKey) {
    h.press_key(Key::Named(key));
}
fn advance(h: &mut Handle<App, impl Fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>>, ms: u64) {
    h.app().now.set(Time(Duration::from_millis(ms)));
    h.press_key(Key::character('~')); // A host frame, with no control binding.
}

#[test]
fn golden_idle_moving_and_selected() {
    let mut h = handle(MotionPref::Full);
    h.assert_golden("segmented_idle");
    h.click(Role::Button, "Cargo");
    advance(&mut h, 30);
    h.assert_golden("segmented_moving");
    advance(&mut h, 120);
    h.assert_golden("segmented_selected");
}

#[test]
fn indicator_travels_from_old_segment_and_arrows_never_wrap_at_ends() {
    let mut h = handle(MotionPref::Full);
    key(&mut h, NamedKey::ArrowRight);
    assert_eq!(h.app().selected, 0, "unfocused arrows stay with the game");
    key(&mut h, NamedKey::Tab);
    key(&mut h, NamedKey::ArrowLeft);
    assert_eq!(h.app().selected, 0, "Left stops at the first segment");
    key(&mut h, NamedKey::ArrowRight);
    assert_eq!(h.app().selected, 1);
    assert_eq!(
        h.app().indicator_x.get(),
        0.0,
        "selection changes before the background moves"
    );
    advance(&mut h, 60);
    let middle = h.app().indicator_x.get();
    assert!(
        middle > 0.0 && middle < 68.0,
        "background must be between old/new segments: {middle}"
    );
    advance(&mut h, 120);
    assert_eq!(h.app().indicator_x.get(), 68.0);
    key(&mut h, NamedKey::ArrowRight);
    key(&mut h, NamedKey::ArrowRight);
    assert_eq!(h.app().selected, 2, "Right stops at the last segment");
    assert_eq!(h.toggle_of(Role::Button, "Cargo"), Toggle::On);
    key(&mut h, NamedKey::ArrowLeft);
    assert_eq!(
        h.app().selected,
        1,
        "Left still works after reaching the end"
    );
}

#[test]
fn reduced_motion_moves_the_background_immediately() {
    let mut h = handle(MotionPref::Reduced);
    h.click(Role::Button, "Cargo");
    assert_eq!(h.app().indicator_x.get(), 136.0);
    assert_eq!(h.toggle_of(Role::Button, "Map"), Toggle::Off);
    assert_eq!(h.toggle_of(Role::Button, "Cargo"), Toggle::On);
}
