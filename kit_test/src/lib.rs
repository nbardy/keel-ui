//! Drive a Keel UI component the way a player does, and check what it draws.
//!
//! `Handle` owns a real app, a real `Store`, the real Inter font and the real router. `click`, `press_key` and
//! `type_text` route a frame’s batch against the retained tree, dispatch every event’s intents in order, then rebuild
//! the next frame’s view. Controls are addressed by (Role, accessible label), never by number, and a control that is
//! not on screen (or scrolled out of its scroll area) panics with its name, so a typo cannot pass as a no-op. A click
//! lands where the control is drawn, through every enclosing scroll offset.
//!
//! The loop is a host’s: the view reads the router’s memory (`Cx::routed`, so `cx.hovered/pressed/focused` and
//! `cx.scroll` are live), and every layout is followed by `Router::frame` (scroll measurement, reveal, stick) and
//! `UiMemory::end_frame`. The router uses `Platform::Other` (Ctrl is the command key) on every OS, so a test that
//! presses Ctrl+V means the same thing on a Mac as on CI.
//!
//! Use it from an app's own tests, the same way the kit tests its components:
//!
//! ```
//! use kit_core::{Theme, WidgetId};
//! use kit_state::{Cx, Host, Intent};
//! use kit_test::{Asked, Handle, TestApp};
//! use kit_view::{A11y, Bindings, Node, Role, TextStyle, Toggle, interactive, text};
//!
//! fn view(_cx: &Cx<'_, TestApp>, theme: &Theme) -> Node<Intent<TestApp>> {
//!     let face = text("Save", TextStyle::new(14.0, theme.colors.text));
//!     let a11y = A11y::Labeled { role: Role::Button, label: "Save".into(), toggle: Toggle::NotToggle };
//!     let on_press = Bindings::activate(Intent::Ask(Asked("saved".into())));
//!     interactive(WidgetId::of(&"save"), on_press, a11y, face)
//! }
//!
//! let mut h = Handle::new(TestApp::new(), (160.0, 60.0), view);
//! h.click(Role::Button, "Save");
//! assert_eq!(h.app().asked, [Asked("saved".into())]);
//! ```
//!
//! Goldens are PNGs in `tests/golden/` of the package under test (`cargo test` runs there). `KIT_BLESS=1` writes
//! them; look at a new picture before blessing it. Use `Handle::with_golden_dir` to put them elsewhere.
use kit_core::kurbo::Point;
use kit_core::kurbo::Size;
use kit_core::peniko::Color;
use kit_core::{ColorVision, Key, MotionPref, Theme, Time, Viewport};
use kit_input::{ClipboardRequest, InputEvent, KeyState, Modifiers, Phase, Platform};
use kit_input::{PointerEvent, PointerId, PointerKind, Router, UiMemory, visible_rect};
use kit_layout::TaffyLayout;
use kit_paint::paint;
use kit_raster::RasterUi;
use kit_raster::golden::{self, Mode, Tolerance};
use kit_raster::tiny_skia::{self, Pixmap};
use kit_state::{Cx, Host, Intent, Store, dispatch, wake_due};
use kit_view::testing::{control, text_system};
use kit_view::{CosmicText, LaidKind, LaidOut, NoWorld, Node, Role, Semantic, SemanticState};
use kit_view::{Toggle, semantics};
use std::path::PathBuf;

pub fn theme() -> Theme {
    Theme::new(ColorVision::Normal, MotionPref::Full)
}

/// What a component asked the host for, as text. A test whose widget events must convert into its ask
/// (`impl From<ItsEvent> for Asked`) cannot use this type: the orphan rule forbids that impl. It defines its own ask
/// type and host instead.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Asked(pub String);

/// The smallest host: a clock and what its components asked for.
#[derive(Default)]
pub struct TestApp {
    pub now: Time,
    pub asked: Vec<Asked>,
}
impl TestApp {
    pub fn new() -> Self {
        Self::default()
    }
}
impl Host for TestApp {
    type Ask = Asked;
    fn now(&self) -> Time {
        self.now
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, ask: Asked) {
        self.asked.push(ask);
    }
}

/// A component under test: `view` is called once after a frame’s batch of events, with a fresh `Cx`.
pub struct Handle<A: Host, V> {
    app: A,
    store: Store<A>,
    view: V,
    theme: Theme,
    viewport: Viewport,
    text: CosmicText,
    laid: LaidOut<Intent<A>>,
    memory: UiMemory,
    router: Router,
    view_redraw: kit_core::Redraw,
    golden_dir: PathBuf,
}

impl<A: Host, V: Fn(&Cx<'_, A>, &Theme) -> Node<Intent<A>>> Handle<A, V> {
    pub fn new(app: A, size: (f64, f64), view: V) -> Self {
        let viewport = Viewport::new(Size::new(size.0, size.1), 1.0);
        let (store, theme, mut text) = (Store::new(), theme(), text_system());
        let mut router = Router::default();
        router.config.platform = Platform::Other;
        let mut memory = UiMemory::new(viewport);
        let (laid, view_redraw) =
            lay_out(&app, &store, &view, &theme, viewport, &mut text, &memory);
        router.frame(&laid, &mut memory, app.now());
        Self {
            app,
            store,
            view,
            theme,
            viewport,
            text,
            laid,
            memory,
            router,
            view_redraw,
            golden_dir: PathBuf::from("tests/golden"),
        }
    }

    /// Where `assert_golden` reads and writes PNGs. The default, `tests/golden`, is relative to the working
    /// directory, which `cargo test` sets to the package root.
    pub fn with_golden_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.golden_dir = dir.into();
        self
    }

    pub fn app(&self) -> &A {
        &self.app
    }
    /// Refused intents (`late`) and reducer faults are counted here.
    pub fn store(&self) -> &Store<A> {
        &self.store
    }
    /// Copy and paste requests the components made (answer a `Read` with `input(InputEvent::Paste { .. })`).
    pub fn take_clipboard(&mut self) -> Vec<ClipboardRequest> {
        self.memory.take_clipboard()
    }

    /// A single-event frame. Use input_frame for a host batch: every event hits the
    /// retained layout and each event's intents dispatch before the next event routes.
    pub fn input(&mut self, event: InputEvent) {
        self.input_frame([event]);
    }
    pub fn input_frame(&mut self, events: impl IntoIterator<Item = InputEvent>) {
        for event in events {
            let routed = self
                .router
                .route(&self.laid, &mut self.memory, event, self.app.now());
            for intent in routed.messages() {
                dispatch(&mut self.app, &mut self.store, intent);
            }
        }
        self.frame();
    }
    /// The independent sources and their one host-facing combination.
    pub fn redraw_sources(&self) -> [kit_core::Redraw; 4] {
        [
            self.view_redraw,
            self.store.redraw(&self.app),
            self.memory.redraw(&self.laid),
            self.draw().redraw,
        ]
    }
    pub fn draw(&self) -> kit_paint::DrawList {
        paint(&self.laid, &self.memory, &self.theme, self.app.now())
    }
    pub fn redraw(&self) -> kit_core::Redraw {
        self.draw().frame_redraw(
            self.view_redraw,
            self.store.redraw(&self.app),
            self.memory.redraw(&self.laid),
        )
    }

    /// One host frame with no input: `change` the app (advance its clock), fire due wake-ups, rebuild the view.
    pub fn tick(&mut self, change: impl FnOnce(&mut A)) {
        change(&mut self.app);
        self.frame();
    }

    /// The host's per-frame step: collect ended keys, wake-ups due now (`kit_state::wake_due`) before the view, then layout,
    /// `Router::frame` (scroll physics, focus claims) and `UiMemory::end_frame`.
    fn frame(&mut self) {
        // The app may have changed outside `dispatch` (a `tick`, i.e. server data): ask every key its life, as
        // `game_client::Client::frame` does, so a test sees state collected when its key ends.
        self.store.collect(&self.app);
        wake_due(&mut self.app, &mut self.store);
        (self.laid, self.view_redraw) = lay_out(
            &self.app,
            &self.store,
            &self.view,
            &self.theme,
            self.viewport,
            &mut self.text,
            &self.memory,
        );
        self.router
            .frame(&self.laid, &mut self.memory, self.app.now());
        self.memory.end_frame();
    }

    /// Press and release the centre of the visible part of the control a screen reader announces as `label` with `role`.
    /// Panics (naming the control) when the screen shows no such control: a typo must not pass as a no-op.
    pub fn click(&mut self, role: Role, label: &str) {
        let at = self.center(role, label);
        self.click_at(at);
    }

    /// The centre of the control `label` with `role`, where it is on screen (to press, drag or release over it).
    /// Inside a scroll area that is the laid rect less the area's offset, and of the part that is in view.
    /// Panics when no part of it is in view: a click there would land on something else.
    pub fn center(&self, role: Role, label: &str) -> Point {
        let node = control(&self.laid, role, label)
            .unwrap_or_else(|| panic!("no {role:?} named {label:?}"));
        visible_rect(&self.laid, &self.memory, node)
            .unwrap_or_else(|| {
                panic!("{role:?} named {label:?} is scrolled out of view: no part of it is in its scroll area")
            })
            .center()
    }

    /// Whether the control `label` with `role` is enabled, disabled or an inert tile, as announced.
    pub fn state_of(&self, role: Role, label: &str) -> SemanticState {
        self.semantic(role, label).state
    }

    /// Everything the control `label` with `role` announces.
    pub fn semantic(&self, role: Role, label: &str) -> Semantic {
        semantics(&self.laid)
            .into_iter()
            .find(|s| s.role == role && s.label == label)
            .unwrap_or_else(|| panic!("no {role:?} named {label:?}"))
    }

    /// Press and release at a point (a click on empty space, outside a popup).
    pub fn click_at(&mut self, at: Point) {
        self.input_frame([Phase::Down, Phase::Up].map(|phase| {
            InputEvent::Pointer(PointerEvent {
                id: PointerId(1),
                kind: PointerKind::Mouse,
                phase,
                at,
                pressure: 1.0,
            })
        }));
    }

    pub fn press_key(&mut self, key: Key) {
        self.press_key_with(key, Modifiers::default());
    }
    pub fn press_key_with(&mut self, key: Key, mods: Modifiers) {
        self.input_frame([KeyState::Down, KeyState::Up].map(|state| InputEvent::Key {
            key,
            state,
            mods,
        }));
    }

    /// Committed text for whatever is focused (click a field first).
    pub fn type_text(&mut self, text: &str) {
        self.input(InputEvent::Text(text.to_string()));
    }

    /// Every text node's lines as shown, in tree order (a value printed beside the controls).
    pub fn texts(&self) -> Vec<String> {
        self.laid
            .root
            .descendants()
            .into_iter()
            .filter_map(|node| match &node.kind {
                LaidKind::Text { layout, .. } => self.text.buffer(layout.id),
                LaidKind::Stack { .. }
                | LaidKind::Panel { .. }
                | LaidKind::Image { .. }
                | LaidKind::Shape { .. }
                | LaidKind::Interactive { .. }
                | LaidKind::Scroll { .. }
                | LaidKind::TextField { .. }
                | LaidKind::Layer { .. }
                | LaidKind::World { .. }
                | LaidKind::Portal { .. } => None,
            })
            .flat_map(|buffer| buffer.lines.iter().map(|line| line.text().to_owned()))
            .collect()
    }

    /// Whether the control `label` with `role` is on screen (a popup's options exist only while it is open).
    pub fn shows(&self, role: Role, label: &str) -> bool {
        control(&self.laid, role, label).is_some()
    }

    /// The toggle state the control announces (`aria-pressed`).
    pub fn toggle_of(&self, role: Role, label: &str) -> Toggle {
        self.semantic(role, label).toggle
    }

    /// The component as the CPU backend draws it over the theme's background; compare with `tests/golden/<name>.png`
    /// (`KIT_BLESS=1` writes it). Rule: look at the picture before blessing it.
    pub fn assert_golden(&mut self, name: &str) {
        let list = paint(&self.laid, &self.memory, &self.theme, self.app.now());
        let size = self.viewport.size;
        let mut pixmap = Pixmap::new(size.width as u32, size.height as u32).expect("non-zero size");
        let bg: Color = self.theme.colors.surface;
        let [r, g, b, _] = bg.to_rgba8().to_u8_array();
        pixmap.fill(tiny_skia::Color::from_rgba8(r, g, b, 255));
        RasterUi::new()
            .render(&list, &mut self.text, &mut pixmap)
            .expect("draw list renders");
        if let Err(e) = golden::compare(
            &self.golden_dir,
            name,
            &pixmap,
            Tolerance::ROUNDING,
            Mode::from_env(),
        ) {
            panic!("golden `{name}`: {e}");
        }
    }
}

fn lay_out<A: Host>(
    app: &A,
    store: &Store<A>,
    view: &impl Fn(&Cx<'_, A>, &Theme) -> Node<Intent<A>>,
    theme: &Theme,
    viewport: Viewport,
    text: &mut CosmicText,
    memory: &UiMemory,
) -> (LaidOut<Intent<A>>, kit_core::Redraw) {
    let cx = Cx::routed(app, store, memory);
    let root = view(&cx, theme);
    let redraw = cx.redraw();
    let laid = TaffyLayout::new()
        .layout(root, viewport, text, &NoWorld)
        .expect("component tree lays out");
    (laid, redraw)
}
