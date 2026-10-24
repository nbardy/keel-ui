//! Lazy bodies, data identity across reorder, and real intrinsic-height/opacity transitions.
use crate::support::{Handle, theme};
use kit_core::kurbo::Size;
use kit_core::{Key, MotionPref, NamedKey, Theme, Time, Viewport, WidgetId};
use kit_input::InputEvent;
use kit_layout::TaffyLayout;
use kit_state::{Cx, Host, Ident, Intent, Life, Store, dispatch};
use kit_view::testing::{control, text_system};
use kit_view::{A11y, Bindings, LaidKind, LaidOut, NoWorld, Node, Role, TextStyle, Toggle};
use kit_view::{column, interactive, text};
use kit_widgets::collapsing::{Collapsing, collapsing};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

struct App {
    now: Rc<Cell<Time>>,
    builds: [Cell<u32>; 2],
    reversed: bool,
    motion: MotionPref,
}
impl App {
    fn new() -> Self {
        Self {
            now: Rc::new(Cell::new(Time::default())),
            builds: [Cell::new(0), Cell::new(0)],
            reversed: false,
            motion: MotionPref::Full,
        }
    }
    fn at(&self, millis: u64) {
        self.now.set(Time(Duration::from_millis(millis)));
    }
}
impl Host for App {
    type Ask = ();
    fn now(&self) -> Time {
        self.now.get()
    }
    fn motion(&self) -> MotionPref {
        self.motion
    }
    fn ask(&mut self, (): ()) {
        self.reversed = !self.reversed;
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
    let order = match cx.app().reversed {
        false => [0, 1],
        true => [1, 0],
    };
    let headers = order.map(|i| {
        let (label, body) = [
            ("Crew", "Helm ready\nBilge ready\nGuns ready"),
            ("Rival", "No ships sighted"),
        ][i];
        collapsing(
            cx,
            Collapsing {
                theme,
                key: Id(label),
                label,
                default_open: false,
                body: || {
                    let count = &cx.app().builds[i];
                    count.set(count.get() + 1);
                    text(
                        body,
                        TextStyle::new(theme.type_scale.body as f32, theme.colors.text),
                    )
                },
            },
        )
    });
    let swap = interactive(
        WidgetId::of(&Id("swap")),
        Bindings::activate(Intent::Ask(())),
        A11y::Labeled {
            role: Role::Button,
            label: "Swap siblings".into(),
            toggle: Toggle::NotToggle,
        },
        text(
            "Swap siblings",
            TextStyle::new(theme.type_scale.label as f32, theme.colors.text_muted),
        ),
    );
    column(headers.into_iter().chain([swap]))
        .gap(theme.spacing.sm)
        .pad(theme.spacing.md)
}

#[test]
fn golden_closed_opening_and_open() {
    let app = App::new();
    let now = app.now.clone();
    let mut h = Handle::new(app, (220.0, 180.0), view);
    h.assert_golden("collapsing_closed");
    h.click(Role::Button, "Crew");
    now.set(Time(Duration::from_millis(55)));
    h.input(InputEvent::Focus(true));
    h.assert_golden("collapsing_opening");
    now.set(Time(Duration::from_millis(220)));
    h.input(InputEvent::Focus(true));
    h.assert_golden("collapsing_open");
}

#[test]
fn closed_bodies_cost_nothing_and_open_state_follows_data_after_sibling_reorder() {
    let app = App::new();
    let now = app.now.clone();
    let mut h = Handle::new(app, (220.0, 180.0), view);
    assert_eq!(h.app().builds.each_ref().map(Cell::get), [0, 0]);
    h.press_key(Key::Named(NamedKey::Space));
    assert_eq!(h.app().builds.each_ref().map(Cell::get), [0, 0]);
    h.click(Role::Button, "Crew");
    now.set(Time(Duration::from_millis(220)));
    h.input(InputEvent::Focus(true));
    assert!(h.app().builds[0].get() > 0, "opening actually builds Crew");
    assert_eq!(h.app().builds[1].get(), 0, "closed Rival never builds");
    h.click(Role::Button, "Swap siblings");
    assert!(h.app().reversed);
    assert_eq!(h.toggle_of(Role::Button, "Crew"), Toggle::On);
    assert_eq!(h.toggle_of(Role::Button, "Rival"), Toggle::Off);
    assert_eq!(h.app().builds[1].get(), 0);
    h.click(Role::Button, "Crew");
    now.set(Time(Duration::from_millis(440)));
    h.input(InputEvent::Focus(true));
    let closed_count = h.app().builds[0].get();
    h.input(InputEvent::Focus(true));
    h.click(Role::Button, "Swap siblings");
    assert_eq!(
        h.app().builds[0].get(),
        closed_count,
        "settled closed body stops building"
    );
    assert_eq!(h.toggle_of(Role::Button, "Crew"), Toggle::Off);
}

fn layout(app: &App, store: &Store<App>) -> LaidOut<Intent<App>> {
    let node = view(&Cx::new(app, store), &theme());
    TaffyLayout::new()
        .layout(
            node,
            Viewport::new(Size::new(220.0, 180.0), 1.0),
            &mut text_system(),
            &NoWorld,
        )
        .expect("collapsing lays out")
}
fn toggle(app: &mut App, store: &mut Store<App>) {
    let laid = layout(app, store);
    let LaidKind::Interactive { on, .. } = &control(&laid, Role::Button, "Crew").unwrap().kind
    else {
        panic!("header is interactive");
    };
    dispatch(app, store, on.activate.clone().expect("header toggle"));
}
fn rival_y(laid: &LaidOut<Intent<App>>) -> f64 {
    control(laid, Role::Button, "Rival").unwrap().rect.y0
}
fn opacity(laid: &LaidOut<Intent<App>>) -> f32 {
    laid.root
        .descendants()
        .into_iter()
        .find_map(|n| {
            if let LaidKind::Layer { opacity, .. } = &n.kind
                && *opacity < 1.0
            {
                Some(*opacity)
            } else {
                None
            }
        })
        .expect("a partially transparent body layer")
}

#[test]
fn intrinsic_body_height_and_opacity_animate_and_reversing_does_not_jump() {
    let (mut app, mut store) = (App::new(), Store::new());
    let closed = rival_y(&layout(&app, &store));
    toggle(&mut app, &mut store);
    assert_eq!(rival_y(&layout(&app, &store)), closed);
    app.at(55);
    let partial = layout(&app, &store);
    let middle = rival_y(&partial);
    let alpha = opacity(&partial);
    assert!(alpha > 0.0 && alpha < 1.0);
    app.at(220);
    let full = rival_y(&layout(&app, &store));
    assert!(
        closed < middle && middle < full,
        "height must grow: {closed} < {middle} < {full}"
    );
    toggle(&mut app, &mut store);
    let started = rival_y(&layout(&app, &store));
    assert_eq!(started, full, "closing starts at full height");
    app.at(275);
    let shrinking = rival_y(&layout(&app, &store));
    assert!(closed < shrinking && shrinking < full);
    toggle(&mut app, &mut store);
    assert_eq!(
        rival_y(&layout(&app, &store)),
        shrinking,
        "reversal retains current height"
    );
    app.at(495);
    assert_eq!(rival_y(&layout(&app, &store)), full);
}

#[test]
fn reduced_motion_snaps_and_keyboard_activation_is_focused() {
    let mut app = App::new();
    app.motion = MotionPref::Reduced;
    let mut h = Handle::new(app, (220.0, 180.0), view);
    h.press_key(Key::Named(NamedKey::Tab));
    h.press_key(Key::Named(NamedKey::Enter));
    assert_eq!(h.toggle_of(Role::Button, "Crew"), Toggle::On);
    assert!(
        h.app().builds[0].get() > 0,
        "reduced motion builds open body without a clock tick"
    );
    assert_eq!(h.app().builds[1].get(), 0);
    h.press_key(Key::Named(NamedKey::Space));
    assert_eq!(h.toggle_of(Role::Button, "Crew"), Toggle::Off);
    let builds = h.app().builds[0].get();
    h.input(InputEvent::Focus(true));
    assert_eq!(h.app().builds[0].get(), builds);
}
