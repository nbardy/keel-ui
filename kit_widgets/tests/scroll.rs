//! Virtual list and scroll-state collection through the whole loop: view -> taffy layout -> router frame -> input.
use kit_core::kurbo::{Point, Size, Vec2};
use kit_core::peniko::{Brush, Color};
use kit_core::{MotionPref, ScrollView, Time, Viewport, WidgetId};
use kit_input::{
    DeltaUnit, InputEvent, Phase, PointerEvent, PointerId, PointerKind, Router, UiMemory,
};
use kit_layout::TaffyLayout;
use kit_state::{Cx, Host, Ident, Life, Store};
use kit_view::testing::text_system;
use kit_view::{
    A11y, Bindings, CosmicText, LaidOut, NoWorld, Node, Style, column, interactive, rect,
};
use kit_widgets::virtual_rows;

struct App {
    now: Time,
    feed_open: bool,
}
impl Host for App {
    type Ask = ();
    fn now(&self) -> Time {
        self.now
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, _: ()) {}
}
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Feed;
impl Ident<App> for Feed {
    fn life(&self, app: &App) -> Life {
        match app.feed_open {
            true => Life::Live,
            false => Life::Over,
        }
    }
}

const ROW: f64 = 20.0;
fn row_id(i: usize) -> WidgetId {
    WidgetId::of(&("row", i))
}
fn view(cx: &Cx<'_, App>) -> Node<usize> {
    let ink = Brush::Solid(Color::from_rgba8(40, 40, 40, 255));
    let list = virtual_rows(cx, &Feed, 1000, ROW, |i| {
        let face = rect(Style::column().sized(100.0, ROW), ink.clone());
        interactive(row_id(i), Bindings::activate(i), A11y::Hidden, face)
    });
    column(vec![list.size(100.0, 100.0)])
}

struct Loop {
    app: App,
    store: Store<App>,
    memory: UiMemory,
    text: CosmicText,
    laid: LaidOut<usize>,
}
impl Loop {
    fn new() -> Self {
        let viewport = Viewport::new(Size::new(400.0, 400.0), 1.0);
        let (app, store, mut text) = (
            App {
                now: Time::default(),
                feed_open: true,
            },
            Store::new(),
            text_system(),
        );
        let memory = UiMemory::new(viewport);
        let laid = TaffyLayout::new()
            .layout(
                column(Vec::<Node<usize>>::new()),
                viewport,
                &mut text,
                &NoWorld,
            )
            .expect("empty tree");
        let mut l = Self {
            app,
            store,
            memory,
            text,
            laid,
        };
        l.frame();
        l
    }
    /// One host frame: view with the router's reads, lay out, let the router measure.
    fn frame(&mut self) {
        let root = view(&Cx::routed(&self.app, &self.store, &self.memory));
        self.laid = TaffyLayout::new()
            .layout(root, self.memory.viewport(), &mut self.text, &NoWorld)
            .expect("list lays out");
        Router::default().frame(&self.laid, &mut self.memory, self.app.now);
    }
    fn send(&mut self, event: InputEvent) -> Vec<usize> {
        Router::default()
            .route(&self.laid, &mut self.memory, event, self.app.now)
            .messages()
    }
    fn built(&self) -> Vec<usize> {
        (0..1000)
            .filter(|i| self.laid.find(row_id(*i)).is_some())
            .collect()
    }
}
fn wheel(dy: f64) -> InputEvent {
    InputEvent::Wheel {
        at: Point::new(50.0, 50.0),
        delta: Vec2::new(0.0, dy),
        unit: DeltaUnit::Pixels,
    }
}
fn mouse(phase: Phase, y: f64) -> InputEvent {
    InputEvent::Pointer(PointerEvent {
        id: PointerId(0),
        kind: PointerKind::Mouse,
        phase,
        at: Point::new(50.0, y),
        pressure: 1.0,
        button: kit_input::PointerButton::Primary,
    })
}

#[test]
fn virtual_rows_builds_only_the_window_and_keeps_row_identity_while_scrolling() {
    let mut l = Loop::new();
    l.frame();
    // 100 px viewport / 20 px rows = 5 rows, plus 3 of overscan below (none above row 0).
    assert_eq!(l.built(), (0..8).collect::<Vec<_>>());

    l.send(wheel(410.0));
    l.frame();
    // Offset 410: rows 20..26 intersect the viewport; overscan widens it to 17..29.
    assert_eq!(l.built(), (17..29).collect::<Vec<_>>());
    let row = l.laid.find(row_id(21)).expect("row 21 is built").rect;
    assert_eq!(
        row.y0, 420.0,
        "row i sits at i * row height in content space"
    );

    // Press row 21 (on screen at 10..30, then 5..25), scroll 5 px while held, rebuild, release: the press survives
    // the rebuild only if row 21 has the same id in the new window.
    assert_eq!(l.send(mouse(Phase::Down, 20.0)), vec![]);
    l.send(wheel(5.0));
    l.frame();
    assert_eq!(l.built(), (17..29).collect::<Vec<_>>());
    assert_eq!(l.send(mouse(Phase::Up, 20.0)), vec![21]);
}

#[test]
fn a_scroll_offset_is_collected_when_its_key_ends() {
    let mut l = Loop::new();
    l.send(wheel(300.0));
    l.frame();
    let area = WidgetId::of(&Feed);
    assert!(
        matches!(l.memory.scroll_view(area), ScrollView::Seen { offset, .. } if offset.y == 300.0)
    );

    // The feed closes: the store collects its key and hands the area's id back to the router's memory.
    l.app.feed_open = false;
    l.store.collect(&l.app);
    let released = l.store.take_released();
    assert_eq!(released, vec![area]);
    l.memory.forget_scrolls(&released);
    assert_eq!(l.memory.scroll_view(area), ScrollView::Unseen);

    // Reopened, it starts at the top instead of the stale 300 px.
    l.app.feed_open = true;
    l.frame();
    assert_eq!(l.memory.scroll_offset(area), Vec2::ZERO);
    assert!(l.store.take_released().is_empty(), "released once");
}
