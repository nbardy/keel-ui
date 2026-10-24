//! Switch goldens, keyboard/disabled rules, and real laid-out knob positions over host time.
use crate::support::{Handle, theme};
use core::cell::Cell;
use kit_core::kurbo::{Rect, Size};
use kit_core::{Key, MotionPref, NamedKey, ShapeSpec, Theme, Time, Viewport};
use kit_input::InputEvent;
use kit_layout::TaffyLayout;
use kit_state::{Cx, Host, Ident, Intent, Life};
use kit_view::testing::{control, text_system};
use kit_view::{LaidKind, NoWorld, Node, Role, Toggle, column};
use kit_widgets::Press;
use kit_widgets::switch::{Switch, switch};

struct App {
    now: Cell<Time>,
    motion: MotionPref,
    flags: [bool; 2],
}
impl App {
    fn new(motion: MotionPref) -> Self {
        Self {
            now: Cell::new(Time::default()),
            motion,
            flags: [false; 2],
        }
    }
}
impl Host for App {
    type Ask = usize;
    fn now(&self) -> Time {
        self.now.get()
    }
    fn motion(&self) -> MotionPref {
        self.motion
    }
    fn ask(&mut self, index: usize) {
        self.flags[index] = !self.flags[index];
    }
}
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Id(&'static str);
impl Ident<App> for Id {
    fn life(&self, _: &App) -> Life {
        Life::Live
    }
}
fn view(cx: &Cx<'_, App>, theme: &Theme) -> Node<Intent<App>> {
    let control = |label, on, press| {
        switch(
            cx,
            Switch {
                theme,
                key: Id(label),
                label,
                on,
                press,
            },
        )
    };
    column([
        control("Sound", cx.app().flags[0], Press::Sends(Intent::Ask(0))),
        control("Music", cx.app().flags[1], Press::Sends(Intent::Ask(1))),
        control("Locked", true, Press::Disabled),
    ])
    .pad(theme.spacing.md)
    .gap(theme.spacing.sm)
}
fn handle(motion: MotionPref) -> Handle<App, impl Fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>> {
    Handle::new(App::new(motion), (180.0, 110.0), view)
}

/// Handle intentionally has no layout/time mutator. Rebuild against its real app/store and inspect geometry,
/// selecting the accessible control and its ellipse rather than relying on a node index.
fn knob(h: &Handle<App, impl Fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>>) -> Rect {
    let tree = view(&Cx::new(h.app(), h.store()), &theme());
    let viewport = Viewport::new(Size::new(180.0, 110.0), 1.0);
    let laid = TaffyLayout::new()
        .layout(tree, viewport, &mut text_system(), &NoWorld)
        .expect("switch layout");
    let rects: Vec<_> = control(&laid, Role::CheckBox, "Sound")
        .expect("Sound switch")
        .descendants()
        .into_iter()
        .filter_map(|n| match &n.kind {
            LaidKind::Shape {
                shape: ShapeSpec::Ellipse,
                ..
            } => Some(n.rect),
            LaidKind::Shape {
                shape: ShapeSpec::Rect | ShapeSpec::RoundRect(_) | ShapeSpec::Path { .. },
                ..
            }
            | LaidKind::Stack { .. }
            | LaidKind::Panel { .. }
            | LaidKind::Image { .. }
            | LaidKind::Text { .. }
            | LaidKind::Interactive { .. }
            | LaidKind::Scroll { .. }
            | LaidKind::TextField { .. }
            | LaidKind::Layer { .. }
            | LaidKind::World { .. }
            | LaidKind::Portal { .. } => None,
        })
        .collect();
    assert_eq!(rects.len(), 1, "one actual knob");
    rects[0]
}

#[test]
fn golden_off_on_and_disabled() {
    let mut h = handle(MotionPref::Full);
    h.assert_golden("switch_off");
    h.click(Role::CheckBox, "Sound");
    h.app().now.set(Time(theme().motion.fast()));
    h.input(InputEvent::Focus(false));
    h.assert_golden("switch_on");
}

#[test]
fn knob_animates_reverses_without_jumping_and_reduced_motion_snaps() {
    let mut h = handle(MotionPref::Full);
    let left = knob(&h);
    h.click(Role::CheckBox, "Sound");
    assert_eq!(knob(&h), left, "activation starts at the old position");
    h.app().now.set(Time(theme().motion.fast() / 2));
    let middle = knob(&h);
    assert!(middle.x0 > left.x0, "the knob moves before the deadline");
    h.app().now.set(Time(theme().motion.fast()));
    let right = knob(&h);
    assert!(
        middle.x0 < right.x0,
        "the halfway frame must not snap to the target"
    );
    assert_eq!(
        left.origin().distance(right.origin()),
        theme().controls.mini.1
    );
    assert_eq!(
        left.size(),
        right.size(),
        "the knob moves without stretching"
    );
    h.click(Role::CheckBox, "Sound");
    h.app()
        .now
        .set(Time(theme().motion.fast() + theme().motion.fast() / 2));
    let reversing = knob(&h);
    assert!(left.x0 < reversing.x0 && reversing.x0 < right.x0);
    h.click(Role::CheckBox, "Sound");
    assert_eq!(knob(&h), reversing, "a mid-flight reversal never jumps");
    h.app().now.set(Time(theme().motion.fast() * 3));
    assert_eq!(knob(&h), right);

    let mut reduced = handle(MotionPref::Reduced);
    assert_eq!(knob(&reduced), left);
    reduced.click(Role::CheckBox, "Sound");
    assert_eq!(
        knob(&reduced),
        right,
        "reduced motion snaps without a clock tick"
    );
    reduced.click(Role::CheckBox, "Sound");
    assert_eq!(knob(&reduced), left, "reduced motion also snaps off");
}

#[test]
fn space_needs_focus_and_disabled_switch_never_activates() {
    let mut h = handle(MotionPref::Full);
    let key = |key| Key::Named(key);
    h.press_key(key(NamedKey::Space));
    assert_eq!(h.app().flags, [false, false]);
    h.click(Role::CheckBox, "Locked");
    assert_eq!(
        h.app().flags,
        [false, false],
        "disabled pointer press is inert"
    );
    assert_eq!(h.toggle_of(Role::CheckBox, "Locked"), Toggle::On);
    h.press_key(key(NamedKey::Tab));
    h.press_key(key(NamedKey::Space));
    assert_eq!(h.app().flags, [true, false]);
    h.press_key(key(NamedKey::Tab));
    h.press_key(key(NamedKey::Space));
    assert_eq!(h.app().flags, [true, true]);
    h.press_key(key(NamedKey::Tab));
    h.press_key(key(NamedKey::Space));
    assert_eq!(
        h.app().flags,
        [false, true],
        "Tab skips the disabled switch"
    );
}
