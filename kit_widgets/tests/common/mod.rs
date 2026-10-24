use kit_core::kurbo::{Point, Size};
use kit_core::{ColorVision, MotionPref, Theme, Time, Viewport};
use kit_input::{InputEvent, Phase, PointerEvent, PointerId, PointerKind, Router, UiMemory};
use kit_layout::TaffyLayout;
use kit_view::testing::text_system;
use kit_view::{LaidOut, NoWorld, Node};
use kit_widgets::{Icon, Ink, PathEl};

pub fn theme() -> Theme {
    Theme::new(ColorVision::Normal, MotionPref::Full)
}

pub struct Scene<Msg> {
    pub laid: LaidOut<Msg>,
    pub memory: UiMemory,
}

pub fn scene<Msg>(root: Node<Msg>, w: f64, h: f64) -> Scene<Msg> {
    let mut text = text_system();
    let viewport = Viewport::new(Size::new(w, h), 1.0);
    let laid = TaffyLayout::new()
        .layout(root, viewport, &mut text, &NoWorld)
        .expect("widget tree lays out");
    Scene {
        laid,
        memory: UiMemory::new(viewport),
    }
}

impl<Msg: Clone> Scene<Msg> {
    pub fn pointer(&mut self, phase: Phase, at: Point) -> Vec<Msg> {
        let event = PointerEvent {
            id: PointerId(1),
            kind: PointerKind::Mouse,
            phase,
            at,
            pressure: 1.0,
        };
        Router::default()
            .route(
                &self.laid,
                &mut self.memory,
                InputEvent::Pointer(event),
                Time::default(),
            )
            .messages()
    }
    /// Press and release at `at`; every message the gesture produced.
    pub fn click(&mut self, at: Point) -> Vec<Msg> {
        let mut out = self.pointer(Phase::Down, at);
        out.extend(self.pointer(Phase::Up, at));
        out
    }
    /// Press at `from`, drag past the slop to `to`, release; every message produced.
    pub fn drag(&mut self, from: Point, to: Point) -> Vec<Msg> {
        let mut out = self.pointer(Phase::Down, from);
        out.extend(self.pointer(Phase::Move, to));
        out.extend(self.pointer(Phase::Up, to));
        out
    }
    /// Press the key `c` (any case) and release it; every message it produced.
    pub fn key(&mut self, c: char) -> Vec<Msg> {
        Router::default()
            .route(
                &self.laid,
                &mut self.memory,
                InputEvent::Key {
                    key: kit_core::Key::character(c),
                    state: kit_input::KeyState::Down,
                    mods: kit_input::Modifiers::default(),
                },
                Time::default(),
            )
            .messages()
    }
    /// Centre of the button a screen reader would announce as `label`.
    pub fn center_of_label(&self, label: &str) -> Point {
        kit_view::testing::control(&self.laid, kit_view::Role::Button, label)
            .unwrap_or_else(|| panic!("no button named {label:?}"))
            .rect
            .center()
    }
}

// An icon on the kit's 32x32 canvas, written as `PathEl` ops.
const ARROW: &[PathEl] = &[
    PathEl::Move([16.0, 3.0]),
    PathEl::Line([28.0, 28.0]),
    PathEl::Line([16.0, 22.0]),
    PathEl::Line([4.0, 28.0]),
    PathEl::Close,
];

pub fn arrow() -> Icon {
    Icon::new(ARROW, Ink::Fill)
}
