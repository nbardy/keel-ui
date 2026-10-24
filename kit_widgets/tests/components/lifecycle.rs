//! Host scheduling contract checked against complete rendered component frames.
use kit_core::kurbo::{Point, Size};
use kit_core::peniko::Brush;
use kit_core::{Axis, Key, MotionPref, NamedKey, Redraw, Theme, Time, WidgetId};
use kit_input::{InputEvent, Phase, PointerEvent, PointerId, PointerKind};
use kit_state::{Act, Cx, Host, Ident, Intent, Life};
use kit_test::Handle;
use kit_view::{A11y, Availability, Bindings, Node, Role, ScrollSpec, Style, TextStyle};
use kit_view::{column, interactive, layer, nothing, rect, scroll_area, text};
use kit_widgets::{Submitted, TextField, Toast, ToastStack, Tooltip, text_field, toast, tooltip};
use std::time::Duration;

fn at(ms: u64) -> Time {
    Time(Duration::from_millis(ms))
}
#[derive(Clone, Debug)]
enum Ask {
    Remove,
    Hit(u8),
}
struct App {
    now: Time,
    target: bool,
    removed: bool,
    hits: Vec<u8>,
    motion: MotionPref,
}
impl App {
    fn new() -> Self {
        Self {
            now: at(0),
            target: false,
            removed: false,
            hits: vec![],
            motion: MotionPref::Full,
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
            Ask::Remove => self.removed = true,
            Ask::Hit(i) => self.hits.push(i),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Id {
    Always,
    Row,
}
impl Ident<App> for Id {
    fn life(&self, app: &App) -> Life {
        match self {
            Self::Always => Life::Live,
            Self::Row if !app.removed => Life::Live,
            Self::Row if app.now < at(1000) => Life::Leaving(at(1000)),
            Self::Row => Life::Over,
        }
    }
}
impl From<Submitted<Id>> for Ask {
    fn from(_: Submitted<Id>) -> Self {
        Self::Hit(9)
    }
}
type View = fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>;
fn handle(view: View) -> Handle<App, View> {
    Handle::new(App::new(), (300., 300.), view)
}
fn label(theme: &Theme, value: &str) -> Node<Intent<App>> {
    text(value, TextStyle::new(16., theme.colors.text))
}
fn pointer(at: Point, phase: Phase, kind: PointerKind) -> InputEvent {
    InputEvent::Pointer(PointerEvent {
        id: PointerId(1),
        kind,
        phase,
        at,
        pressure: 1.,
        button: kit_input::PointerButton::Primary,
    })
}

/// Reusable scheduler oracle. For At, sample EVERY intervening millisecond and a
/// sub-millisecond instant just before the boundary. For Idle, include distant future
/// frames. Each sample runs the real view, reducer wakes, layout, physics and paint.
/// Compare the entire DrawList before a deadline; at a changing deadline compare ink
/// commands too, so a metadata-only change cannot satisfy the assertion.
fn check_sleep(h: &mut Handle<App, View>, changes_at_deadline: bool) {
    let now = h.app().now;
    let list = h.draw();
    let demand = h.redraw();
    let samples = match demand {
        Redraw::Now => return,
        Redraw::At(t) => {
            assert!(t > now);
            let mut samples: Vec<_> = ((now.0.as_millis() as u64 + 1)..t.0.as_millis() as u64)
                .map(at)
                .collect();
            samples.push(Time(t.0 - Duration::from_nanos(1)));
            samples
        }
        Redraw::Idle => [1, 17, 530, 2000, 86_400_000]
            .map(|ms| Time(now.0 + Duration::from_millis(ms)))
            .to_vec(),
    };
    for time in samples {
        h.tick(|app| app.now = time);
        assert_eq!(
            h.draw(),
            list,
            "demand {demand:?} allowed a change at {time:?}"
        );
    }
    if let Redraw::At(t) = demand {
        h.tick(|app| app.now = t);
        if changes_at_deadline {
            assert_ne!(
                h.draw().items,
                list.items,
                "deadline must change drawn output"
            );
        }
    }
}

#[test]
fn static_screen_is_idle_in_every_source_and_arbitrary_clock_reads_are_now() {
    let mut h = handle(|_, th| label(th, "Static screen"));
    assert_eq!(h.redraw_sources(), [Redraw::Idle; 4]);
    check_sleep(&mut h, false);
    let h = handle(|cx, th| label(th, &format!("{:?}", cx.now())));
    assert_eq!(h.redraw_sources()[0], Redraw::Now);
}

#[test]
fn focused_field_sleeps_exactly_until_each_caret_toggle_and_reduced_motion_is_idle() {
    let view: View = |cx, th| {
        text_field(
            cx,
            TextField {
                theme: th,
                key: Id::Always,
                label: "Name",
                placeholder: "Name",
                availability: Availability::Enabled,
            },
        )
    };
    let mut h = handle(view);
    h.press_key(Key::Named(NamedKey::Tab));
    h.tick(|app| app.now = at(0)); // consume the conservative focus follow-up
    let painter = h.redraw_sources()[3];
    check_sleep(&mut h, true);
    assert_eq!(painter, Redraw::At(at(530)));
    assert_eq!(h.redraw(), Redraw::At(at(1060)));
    check_sleep(&mut h, true);

    let mut text = kit_view::testing::text_system();
    let th = Theme::new(kit_core::ColorVision::Normal, MotionPref::Reduced);
    let viewport = kit_core::Viewport::new(Size::new(300., 300.), 1.);
    let store = kit_state::Store::new();
    let app = App::new();
    let root = view(&Cx::new(&app, &store), &th);
    let laid = kit_layout::TaffyLayout::new()
        .layout(root, viewport, &mut text, &kit_view::NoWorld)
        .unwrap();
    let mut memory = kit_input::UiMemory::new(viewport);
    let router = kit_input::Router::default();
    let id = kit_view::testing::control(&laid, Role::TextBox, "Name")
        .unwrap()
        .id()
        .unwrap();
    router.request_focus(&laid, &mut memory, id, at(0));
    let first = kit_paint::paint(&laid, &memory, &th, at(0));
    assert_eq!(first.redraw, Redraw::Idle);
    assert_eq!(first, kit_paint::paint(&laid, &memory, &th, at(10000)));
}

#[test]
fn hovered_tooltip_sleeps_until_its_delay_then_appears() {
    let mut h = handle(|cx, th| {
        tooltip(
            cx,
            Tooltip {
                theme: th,
                key: Id::Always,
                delay: Duration::from_millis(500),
                trigger: label(th, "Inspect"),
                on: Bindings::default(),
                a11y: A11y::named(Role::Button, "Inspect"),
                content: label(th, "Help"),
            },
        )
    });
    let center = h.center(Role::Button, "Inspect");
    h.input(pointer(center, Phase::Move, PointerKind::Mouse));
    assert_eq!(h.redraw(), Redraw::Now, "hover overlay is still animating");
    h.tick(|app| app.now = at(200));
    let sources = h.redraw_sources();
    check_sleep(&mut h, true);
    assert_eq!(
        sources,
        [
            Redraw::At(at(500)),
            Redraw::Idle,
            Redraw::Idle,
            Redraw::Idle
        ]
    );
    assert_eq!(h.redraw(), Redraw::Idle);
    check_sleep(&mut h, false);
}

#[test]
fn running_tween_and_leaving_row_redraw_until_they_finish() {
    let mut h = handle(|cx, th| {
        let alpha = cx.animate_bool(&Id::Row, cx.app().target, Duration::from_millis(300));
        layer(alpha, label(th, "Animated row"))
    });
    assert_eq!(h.redraw(), Redraw::Idle);
    h.tick(|app| app.target = true);
    check_sleep(&mut h, false);
    assert_eq!(h.redraw_sources()[1], Redraw::Now);
    let start = h.draw();
    h.tick(|app| app.now = at(150));
    assert_ne!(h.draw().items, start.items);
    assert_eq!(h.redraw(), Redraw::Now);
    h.tick(|app| app.now = at(300));
    assert_eq!(h.redraw(), Redraw::Idle);
    check_sleep(&mut h, false);

    let mut h = handle(|cx, th| {
        let _state = cx.state(
            &Id::Row,
            || (),
            |_: &mut (), _: &Id, (): (), _: &App| Act::none(),
        );
        match Id::Row.life(cx.app()) {
            Life::Live => label(th, "Row"),
            Life::Leaving(until) => layer(
                cx.app().now.since(at(0)).as_secs_f32() / until.0.as_secs_f32(),
                label(th, "Row"),
            ),
            Life::Over => nothing(),
        }
    });
    h.tick(|app| app.removed = true);
    check_sleep(&mut h, false);
    assert_eq!(h.redraw_sources()[1], Redraw::Now);
    let leaving = h.draw();
    h.tick(|app| app.now = at(500));
    assert_ne!(leaving.items, h.draw().items);
    assert_eq!(h.redraw(), Redraw::Now);
    h.tick(|app| app.now = at(1000));
    assert_eq!(h.redraw(), Redraw::Idle);
    check_sleep(&mut h, false);
}

#[test]
fn reducer_timer_sleeps_until_it_changes_the_view() {
    let mut h = handle(|cx, th| {
        let state = cx.state(
            &Id::Always,
            || false,
            |s: &mut bool, _: &Id, fire: bool, _: &App| {
                if fire {
                    *s = true;
                    Act::none()
                } else {
                    Act::wake(at(400), true)
                }
            },
        );
        interactive(
            WidgetId::of(&Id::Always),
            Bindings::activate(state.send(false)),
            A11y::named(Role::Button, "Start"),
            label(th, if *state { "Done" } else { "Wait" }),
        )
    });
    h.click(Role::Button, "Start");
    h.tick(|app| app.now = at(200)); // settle press/hover overlay
    assert_eq!(h.redraw(), Redraw::At(at(400)));
    check_sleep(&mut h, true);
    assert_eq!(h.redraw(), Redraw::Idle);
    check_sleep(&mut h, false);
}

#[test]
fn fling_is_now_then_idle_after_physics_and_bars_settle() {
    let mut h = handle(|cx, th| {
        cx.scroll(&Id::Always);
        if cx.app().removed {
            return label(th, "Hidden list");
        }
        scroll_area(
            WidgetId::of(&Id::Always),
            ScrollSpec::new(Axis::Vertical),
            Style::column().sized(150., 100.),
            rect(
                Style::column().sized(150., 3000.),
                Brush::Solid(th.colors.text),
            ),
        )
    });
    h.tick(|app| app.now = at(0)); // initial scroll measurement is now visible to the view
    assert_eq!(h.redraw_sources(), [Redraw::Idle; 4]);
    h.input(pointer(
        Point::new(50., 90.),
        Phase::Down,
        PointerKind::Touch,
    ));
    for i in 1..=5 {
        h.tick(|app| app.now = at(i * 10));
        h.input(pointer(
            Point::new(50., 90. - 20. * i as f64),
            Phase::Move,
            PointerKind::Touch,
        ));
    }
    h.input(pointer(
        Point::new(50., -10.),
        Phase::Up,
        PointerKind::Touch,
    ));
    check_sleep(&mut h, false);
    assert_eq!(h.redraw_sources()[2], Redraw::Now);
    let released = h.draw();
    h.tick(|app| app.now = at(100));
    assert_ne!(h.draw().items, released.items);
    h.tick(|app| app.removed = true);
    assert_eq!(
        h.redraw_sources(),
        [Redraw::Idle; 4],
        "retained momentum of an absent list does not keep drawing"
    );
    h.tick(|app| app.removed = false);
    assert_eq!(
        h.redraw_sources()[2],
        Redraw::Now,
        "the retained list may coast again when it returns"
    );
    for i in 1..=300 {
        h.tick(|app| app.now = at(100 + i * 16));
    }
    assert_eq!(h.redraw_sources(), [Redraw::Idle; 4]);
    check_sleep(&mut h, false);
}

#[test]
fn toast_sleeps_while_shown_and_redraws_during_its_exit() {
    let mut h = handle(|cx, th| {
        toast(
            cx,
            ToastStack {
                theme: th,
                items: &[Toast {
                    key: Id::Always,
                    label: "Saved",
                    born: at(0),
                    expires: at(1000),
                }],
                max_visible: 1,
            },
        )
    });
    assert_eq!(h.redraw(), Redraw::Now);
    h.tick(|app| app.now = at(200));
    assert_eq!(h.redraw(), Redraw::At(at(1000)));
    check_sleep(&mut h, false); // opacity begins at 1 exactly on the exit boundary
    assert_eq!(h.redraw(), Redraw::Now);
    h.tick(|app| app.now = at(1200));
    assert_eq!(h.redraw(), Redraw::Idle);
    check_sleep(&mut h, false);
}

#[test]
fn one_frame_keeps_its_hit_tree_but_dispatches_between_events_and_refuses_leaving_keys() {
    let mut h = handle(|cx, th| {
        let second = cx.state(
            &Id::Row,
            || (),
            |_: &mut (), _: &Id, (): (), _: &App| Act::ask(Ask::Hit(2)),
        );
        let button = |id, name: &str, intent| {
            interactive(
                id,
                Bindings::activate(intent),
                A11y::named(Role::Button, name),
                label(th, name),
            )
        };
        let first = button(WidgetId::of(&"remove"), "Remove", Intent::Ask(Ask::Remove));
        let row = if cx.app().removed {
            nothing()
        } else {
            button(WidgetId::of(&Id::Row), "Second", second.send(()))
        };
        let third = button(WidgetId::of(&"third"), "Third", Intent::Ask(Ask::Hit(3)));
        column([first, row, third])
    });
    let a = h.center(Role::Button, "Remove");
    let b = h.center(Role::Button, "Second");
    let c = h.center(Role::Button, "Third");
    let clicks = [a, b, c].into_iter().flat_map(|point| {
        [Phase::Down, Phase::Up].map(move |phase| pointer(point, phase, PointerKind::Mouse))
    });
    h.input_frame(clicks);
    assert_eq!(
        h.app().hits,
        [3],
        "third uses its old location; second's key is already Leaving"
    );
    assert_eq!(
        h.store().late_total(),
        1,
        "the second click routed, then the authority refused it"
    );
    assert!(!h.shows(Role::Button, "Second"));
    assert!(
        h.center(Role::Button, "Third").y < c.y,
        "state appears in the next frame layout"
    );
}

#[test]
fn scrollbar_linger_sleeps_then_fades_and_reveal_glides_redraw() {
    let view: View = |cx, th| {
        cx.scroll(&Id::Always);
        let spec = match cx.app().target {
            false => ScrollSpec::new(Axis::Vertical),
            true => ScrollSpec::new(Axis::Vertical).reveal(
                kit_view::RevealTarget::Content(kit_core::kurbo::Rect::new(0., 800., 100., 820.)),
                kit_view::RevealAlign::Nearest,
                &1u8,
            ),
        };
        scroll_area(
            WidgetId::of(&Id::Always),
            spec,
            Style::column().sized(150., 100.),
            rect(
                Style::column().sized(150., 3000.),
                Brush::Solid(th.colors.text),
            ),
        )
    };
    let mut h = handle(view);
    h.input(InputEvent::Wheel {
        at: Point::new(50., 50.),
        delta: kit_core::kurbo::Vec2::new(0., 100.),
        unit: kit_input::DeltaUnit::Pixels,
    });
    assert_eq!(
        h.redraw_sources()[2],
        Redraw::Idle,
        "wheel is immediate, not a coast"
    );
    assert_eq!(h.redraw(), Redraw::At(at(800)), "bars linger after moving");
    check_sleep(&mut h, false);
    assert_eq!(h.redraw(), Redraw::Now, "fade has begun");
    let start = h.draw();
    h.tick(|app| app.now = at(950));
    assert_ne!(h.draw().items, start.items);
    h.tick(|app| app.now = at(1100));
    assert_eq!(h.redraw(), Redraw::Idle);
    check_sleep(&mut h, false);

    let mut h = handle(view);
    h.tick(|app| app.target = true);
    check_sleep(&mut h, false);
    assert_eq!(h.redraw_sources()[2], Redraw::Now);
    let start = h.draw();
    h.tick(|app| app.now = at(16));
    assert_ne!(h.draw().items, start.items);
    for i in 2..=200 {
        h.tick(|app| app.now = at(i * 16));
    }
    assert_eq!(h.redraw(), Redraw::Idle);
    check_sleep(&mut h, false);
}

#[test]
fn precise_deadlines_combine_with_unrestricted_clock_reads_and_spinner_motion() {
    let mut h = handle(|cx, th| {
        let early = cx.redraw_at(at(300));
        cx.redraw_at(at(600));
        label(th, if early < at(300) { "Before" } else { "After" })
    });
    assert_eq!(h.redraw(), Redraw::At(at(300)));
    check_sleep(&mut h, true);
    assert_eq!(h.redraw(), Redraw::At(at(600)));
    check_sleep(&mut h, false);
    let h = handle(|cx, th| {
        cx.now();
        cx.redraw_at(at(600));
        label(th, "Mixed clock readers")
    });
    assert_eq!(h.redraw(), Redraw::Now);
    let view: View = |cx, th| kit_widgets::spinner(cx, kit_widgets::Spinner { theme: th });
    let mut h = handle(view);
    assert_eq!(h.redraw(), Redraw::Now);
    let first = h.draw();
    h.tick(|app| app.now = at(50));
    assert_ne!(h.draw().items, first.items);
    h.tick(|app| app.motion = MotionPref::Reduced);
    assert_eq!(h.redraw_sources(), [Redraw::Idle; 4]);
    check_sleep(&mut h, false);
}

#[test]
fn frame_focus_claims_and_drag_delta_resets_request_a_followup_view() {
    let mut h = handle(|cx, th| {
        interactive(
            WidgetId::of(&Id::Always),
            Bindings::activate(Intent::Ask(Ask::Hit(1))),
            A11y::named(Role::Button, "Focus"),
            label(
                th,
                if cx.focused(&Id::Always) {
                    "Focused"
                } else {
                    "Unfocused"
                },
            ),
        )
        .claim_focus(&1u8)
    });
    check_sleep(&mut h, false);
    assert_eq!(
        h.redraw_sources()[2],
        Redraw::Now,
        "claim was applied after the view read focus"
    );
    h.tick(|app| app.now = at(200));
    assert_eq!(h.redraw(), Redraw::Idle);
    check_sleep(&mut h, false);

    let mut h = handle(|cx, th| {
        interactive(
            WidgetId::of(&Id::Always),
            Bindings::default().on_pointer(|_| Intent::Ask(Ask::Hit(1))),
            A11y::named(Role::Button, "Drag"),
            label(th, &format!("{:?}", cx.drag(&Id::Always))),
        )
    });
    let center = h.center(Role::Button, "Drag");
    h.input(pointer(center, Phase::Down, PointerKind::Touch));
    h.tick(|app| app.now = at(200)); // settle the press overlay
    h.input(pointer(
        center + kit_core::kurbo::Vec2::new(10., 0.),
        Phase::Move,
        PointerKind::Touch,
    ));
    check_sleep(&mut h, false);
    assert_eq!(
        h.redraw_sources()[2],
        Redraw::Now,
        "end_frame reset the delta read by the view"
    );
    h.tick(|app| app.now = at(400));
    assert_eq!(h.redraw(), Redraw::Idle);
    check_sleep(&mut h, false);
}

#[test]
fn first_scroll_measurement_and_the_final_glide_snap_request_a_followup_view() {
    let mut h = handle(|cx, th| {
        let value = format!("{:?}", cx.scroll(&Id::Always));
        column([
            label(th, &value),
            scroll_area(
                WidgetId::of(&Id::Always),
                ScrollSpec::new(Axis::Vertical),
                Style::column().sized(150., 100.),
                rect(
                    Style::column().sized(150., 3000.),
                    Brush::Solid(th.colors.text),
                ),
            ),
        ])
    });
    check_sleep(&mut h, false);
    assert_eq!(
        h.redraw_sources()[2],
        Redraw::Now,
        "Unseen became a measured scroll view"
    );
    h.tick(|app| app.now = at(16));
    assert_eq!(h.redraw(), Redraw::Idle);
    check_sleep(&mut h, false);

    let mut h = handle(|cx, th| {
        let value = format!("{:?}", cx.scroll(&Id::Always));
        let spec = ScrollSpec::new(Axis::Vertical).reveal(
            kit_view::RevealTarget::Content(kit_core::kurbo::Rect::new(0., 800., 100., 820.)),
            kit_view::RevealAlign::Nearest,
            &1u8,
        );
        column([
            label(th, &value),
            scroll_area(
                WidgetId::of(&Id::Always),
                spec,
                Style::column().sized(150., 100.),
                rect(
                    Style::column().sized(150., 3000.),
                    Brush::Solid(th.colors.text),
                ),
            ),
        ])
    });
    assert_eq!(h.redraw_sources()[2], Redraw::Now);
    for i in 1..=200 {
        h.tick(|app| app.now = at(i * 16));
        if h.redraw_sources()[2] == Redraw::Idle {
            check_sleep(&mut h, false);
            return;
        }
    }
    panic!("glide never settled");
}
