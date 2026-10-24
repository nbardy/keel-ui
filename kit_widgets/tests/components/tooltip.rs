//! Tooltip timing, rich content, and viewport-edge placement through the real router.
use core::time::Duration;
use kit_core::kurbo::Point;
use kit_core::{MotionPref, Theme, Time, WidgetId};
use kit_input::{InputEvent, Phase, PointerEvent, PointerId, PointerKind};
use kit_state::{Act, Cx, Host, Ident, Intent, Life};
use kit_test::Handle;
use kit_view::{A11y, Bindings, Node, Role, TextStyle, Toggle, Weight, column, disabled, text};
use kit_widgets::tooltip::{Tooltip, tooltip};
use std::cell::Cell;

struct App {
    now: Cell<Time>,
    entered: usize,
    left: usize,
    activated: usize,
}
impl Host for App {
    type Ask = u8;
    fn now(&self) -> Time {
        self.now.get()
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, ask: u8) {
        match ask {
            0 => self.entered += 1,
            1 => self.left += 1,
            2 => self.activated += 1,
            _ => panic!("unknown test ask"),
        }
    }
}
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Id;
impl Ident<App> for Id {
    fn life(&self, _: &App) -> Life {
        Life::Live
    }
}

fn label(role: Role, name: &str) -> A11y {
    A11y::Labeled {
        role,
        label: name.into(),
        toggle: Toggle::NotToggle,
    }
}
fn view(cx: &Cx<'_, App>, theme: &Theme, top: f64) -> Node<Intent<App>> {
    let events = cx.state(
        &Id,
        || (),
        |_: &mut (), _: &Id, ask: u8, _: &App| Act::ask(ask),
    );
    let title =
        TextStyle::new(theme.type_scale.label as f32, theme.colors.text).weight(Weight::BOLD);
    let detail = TextStyle::new(theme.type_scale.body as f32, theme.colors.text_muted);
    let rich = column([
        text("Cannon", title.clone()),
        text("Fires all mounted guns.", detail),
    ])
    .gap(theme.spacing.xs);
    let content = disabled(
        WidgetId::of(&"help"),
        label(Role::Label, "Cannon help"),
        rich,
    );
    let on = Bindings::activate(events.send(2)).with_hover(events.send(0), events.send(1));
    let tip = tooltip(
        cx,
        Tooltip {
            theme,
            key: Id,
            delay: Duration::from_millis(500),
            trigger: text("Inspect cannon", title).pad(theme.spacing.sm),
            on,
            a11y: label(Role::Button, "Inspect cannon"),
            content,
        },
    );
    column([
        kit_view::stack(kit_view::Style::column().sized(0.0, top), vec![]),
        tip,
    ])
    .pad(theme.spacing.sm)
}
fn pointer(x: f64, y: f64, phase: Phase) -> InputEvent {
    InputEvent::Pointer(PointerEvent {
        id: PointerId(1),
        kind: PointerKind::Mouse,
        phase,
        at: Point::new(x, y),
        pressure: 0.0,
    })
}
fn handle(top: f64) -> Handle<App, impl Fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>> {
    let app = App {
        now: Cell::new(Time(Duration::from_secs(10))),
        entered: 0,
        left: 0,
        activated: 0,
    };
    Handle::new(app, (240.0, 160.0), move |cx, theme| view(cx, theme, top))
}

#[test]
fn appears_only_after_delay_and_leaving_restarts_the_wait() {
    let mut h = handle(0.0);
    assert!(!h.shows(Role::Label, "Cannon help"));
    h.input(pointer(20.0, 20.0, Phase::Move));
    assert!(
        !h.shows(Role::Label, "Cannon help"),
        "entering does not show help immediately"
    );
    h.app().now.set(Time(Duration::from_millis(10_499)));
    h.input(InputEvent::ScaleFactor(1.0));
    assert!(
        !h.shows(Role::Label, "Cannon help"),
        "still waiting one millisecond before deadline"
    );
    h.app().now.set(Time(Duration::from_millis(10_500)));
    h.input(InputEvent::ScaleFactor(1.0));
    assert!(
        h.shows(Role::Label, "Cannon help"),
        "appears exactly at the deadline without another pointer move"
    );
    h.input(pointer(230.0, 150.0, Phase::Move));
    assert!(
        !h.shows(Role::Label, "Cannon help"),
        "leaving hides immediately"
    );
    h.input(pointer(20.0, 20.0, Phase::Move));
    assert!(
        !h.shows(Role::Label, "Cannon help"),
        "re-entry must start a fresh delay"
    );
    h.app().now.set(Time(Duration::from_secs(11)));
    h.input(InputEvent::ScaleFactor(1.0));
    assert!(h.shows(Role::Label, "Cannon help"));
    h.input(pointer(20.0, 20.0, Phase::Leave));
    assert!(
        !h.shows(Role::Label, "Cannon help"),
        "leaving the surface hides too"
    );
    assert_eq!(
        (h.app().entered, h.app().left),
        (2, 2),
        "caller hover bindings survive"
    );
    h.click(Role::Button, "Inspect cannon");
    assert_eq!(h.app().activated, 1, "caller activation survives");
}

#[test]
fn golden_idle_rich_below_and_flipped_above() {
    let mut h = handle(0.0);
    h.assert_golden("tooltip_idle");
    h.input(pointer(20.0, 20.0, Phase::Move));
    h.app().now.set(Time(Duration::from_millis(10_500)));
    h.input(InputEvent::ScaleFactor(1.0));
    h.assert_golden("tooltip_below");
    let mut edge = handle(110.0);
    edge.input(pointer(20.0, 130.0, Phase::Move));
    edge.app().now.set(Time(Duration::from_millis(10_500)));
    edge.input(InputEvent::ScaleFactor(1.0));
    assert!(edge.shows(Role::Label, "Cannon help"));
    edge.assert_golden("tooltip_flipped");
}
