//! winit `WindowEvent` -> kit `InputEvent`. [`Input::convert`] only selects a function; each conversion is a pure
//! function of its payload (plus the window's scale, cursor and modifiers, which winit spreads over several events).
//!
//! Units: the kit works in logical px. winit reports positions, sizes and pixel wheel deltas in physical px, so all
//! of them are divided by the scale factor here; line deltas are not pixels and pass through.
use kit_core::Viewport;
use kit_core::kurbo::{Point, Size, Vec2};
use kit_input::{
    DeltaUnit, ImeEvent, InputEvent, Key, KeyState, Modifiers, NamedKey, Phase, PointerButton,
    PointerEvent, PointerId, PointerKind,
};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{
    ElementState, Force, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey as WinitNamed};

/// The mouse's pointer id. Touches use winit's touch id; one at `u64::MAX` would share the mouse's.
pub const MOUSE: PointerId = PointerId(u64::MAX);

/// One window's conversion state: what winit reports once and the kit wants on every event.
#[derive(Clone, Debug)]
pub struct Input {
    size: PhysicalSize<u32>,
    scale: f64,
    /// Logical px. winit's button events carry no position: a press lands where the cursor last moved.
    cursor: Point,
    /// winit's key events carry no modifiers: they come in `ModifiersChanged`.
    mods: Modifiers,
}

impl Input {
    /// `window.inner_size()` and `window.scale_factor()` at creation.
    pub fn new(size: PhysicalSize<u32>, scale_factor: f64) -> Self {
        Self {
            size,
            scale: scale_factor,
            cursor: Point::ZERO,
            mods: Modifiers::default(),
        }
    }

    /// The window as the kit lays out and draws it (logical size, device scale).
    pub fn viewport(&self) -> Viewport {
        viewport(self.size, self.scale)
    }

    /// The window's size in physical px (the surface size).
    pub fn physical_size(&self) -> PhysicalSize<u32> {
        self.size
    }

    /// The kit events for one window event, in order (a key press can be a key and its text). Events that are not
    /// input (redraw, close, move, theme, file drops, gestures the kit has no event for) give none.
    pub fn convert(&mut self, event: &WindowEvent) -> Vec<InputEvent> {
        match event {
            WindowEvent::Resized(size) => vec![self.resized(*size)],
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                vec![self.rescaled(*scale_factor)]
            }
            WindowEvent::Focused(focused) => vec![InputEvent::Focus(*focused)],
            WindowEvent::ModifiersChanged(state) => vec![self.modifiers_changed(state.state())],
            WindowEvent::KeyboardInput { event, .. } => key_events(
                &event.logical_key,
                event.state,
                event.text.as_deref(),
                self.mods,
            ),
            WindowEvent::Ime(ime) => ime_event(ime).map(InputEvent::Ime).into_iter().collect(),
            WindowEvent::CursorMoved { position, .. } => vec![self.cursor_moved(*position)],
            WindowEvent::CursorLeft { .. } => {
                vec![self.mouse(Phase::Leave, PointerButton::Primary)]
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => vec![self.button(*state, PointerButton::Primary)],
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Right,
                ..
            } => vec![self.button(*state, PointerButton::Secondary)],
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Middle,
                ..
            } => vec![self.button(*state, PointerButton::Middle)],
            WindowEvent::MouseWheel { delta, .. } => vec![wheel(*delta, self.cursor, self.scale)],
            WindowEvent::Touch(t) => vec![touch(t.id, t.phase, t.location, t.force, self.scale)],
            // The kit names three buttons (primary, secondary, middle); the side buttons have no meaning to it.
            WindowEvent::MouseInput {
                button: MouseButton::Back | MouseButton::Forward | MouseButton::Other(_),
                ..
            }
            | WindowEvent::ActivationTokenDone { .. }
            | WindowEvent::Moved(_)
            | WindowEvent::CloseRequested
            | WindowEvent::Destroyed
            | WindowEvent::DroppedFile(_)
            | WindowEvent::HoveredFile(_)
            | WindowEvent::HoveredFileCancelled
            | WindowEvent::CursorEntered { .. }
            | WindowEvent::PinchGesture { .. }
            | WindowEvent::PanGesture { .. }
            | WindowEvent::DoubleTapGesture { .. }
            | WindowEvent::RotationGesture { .. }
            | WindowEvent::TouchpadPressure { .. }
            | WindowEvent::AxisMotion { .. }
            | WindowEvent::ThemeChanged(_)
            | WindowEvent::Occluded(_)
            | WindowEvent::RedrawRequested => Vec::new(),
        }
    }

    fn resized(&mut self, size: PhysicalSize<u32>) -> InputEvent {
        self.size = size;
        InputEvent::Resize(self.viewport())
    }
    /// A new scale is a new logical size even when the physical one is unchanged: the whole viewport is resent.
    fn rescaled(&mut self, scale: f64) -> InputEvent {
        self.scale = scale;
        InputEvent::Resize(self.viewport())
    }
    fn modifiers_changed(&mut self, state: ModifiersState) -> InputEvent {
        self.mods = modifiers(state);
        InputEvent::Modifiers(self.mods)
    }
    fn cursor_moved(&mut self, position: PhysicalPosition<f64>) -> InputEvent {
        self.cursor = logical(position, self.scale);
        self.mouse(Phase::Move, PointerButton::Primary)
    }
    fn button(&self, state: ElementState, button: PointerButton) -> InputEvent {
        self.mouse(
            match state {
                ElementState::Pressed => Phase::Down,
                ElementState::Released => Phase::Up,
            },
            button,
        )
    }
    fn mouse(&self, phase: Phase, button: PointerButton) -> InputEvent {
        InputEvent::Pointer(PointerEvent {
            id: MOUSE,
            kind: PointerKind::Mouse,
            phase,
            at: self.cursor,
            pressure: 1.0,
            button,
        })
    }
}

/// A physical window position in the kit's logical px.
pub fn logical(position: PhysicalPosition<f64>, scale: f64) -> Point {
    Point::new(position.x / scale, position.y / scale)
}

/// A physical window size and its scale as the kit's viewport.
pub fn viewport(size: PhysicalSize<u32>, scale: f64) -> Viewport {
    Viewport::new(
        Size::new(
            f64::from(size.width) / scale,
            f64::from(size.height) / scale,
        ),
        scale,
    )
}

/// winit's `super` is the kit's `meta` (Cmd on macOS, the Windows key elsewhere).
pub fn modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        ctrl: state.control_key(),
        alt: state.alt_key(),
        meta: state.super_key(),
    }
}

/// The kit key for winit's logical key (so a binding follows the player's keyboard layout). `None`: the kit has no
/// name for it (function keys, dead keys, a multi-character composition).
pub fn key(logical: &WinitKey) -> Option<Key> {
    match logical {
        WinitKey::Named(named) => named_key(*named).map(Key::Named),
        WinitKey::Character(text) => {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Some(Key::character(c)),
                (Some(_), Some(_)) | (None, _) => None,
            }
        }
        WinitKey::Unidentified(_) | WinitKey::Dead(_) => None,
    }
}

fn named_key(named: WinitNamed) -> Option<NamedKey> {
    Some(match named {
        WinitNamed::Escape => NamedKey::Escape,
        WinitNamed::Enter => NamedKey::Enter,
        WinitNamed::Tab => NamedKey::Tab,
        WinitNamed::Space => NamedKey::Space,
        WinitNamed::Backspace => NamedKey::Backspace,
        WinitNamed::Delete => NamedKey::Delete,
        WinitNamed::ArrowLeft => NamedKey::ArrowLeft,
        WinitNamed::ArrowRight => NamedKey::ArrowRight,
        WinitNamed::ArrowUp => NamedKey::ArrowUp,
        WinitNamed::ArrowDown => NamedKey::ArrowDown,
        WinitNamed::Home => NamedKey::Home,
        WinitNamed::End => NamedKey::End,
        // winit's `NamedKey` is non-exhaustive (about 300 keys); the kit names the twelve above.
        _ => return None,
    })
}

/// One key transition: the key (when the kit names it), then on a press the text it typed. A focused field takes
/// character and Space keys without acting and inserts the `Text`, so both are always sent and the router decides.
/// No text while Ctrl or Cmd is held (that is a shortcut) or when winit's text is a control character (Enter gives
/// "\r", Backspace "\u{8}": those act through their keys).
pub fn key_events(
    logical: &WinitKey,
    state: ElementState,
    text: Option<&str>,
    mods: Modifiers,
) -> Vec<InputEvent> {
    let key_state = match state {
        ElementState::Pressed => KeyState::Down,
        ElementState::Released => KeyState::Up,
    };
    let typed = match (key_state, text) {
        (KeyState::Down, Some(text))
            if !mods.ctrl && !mods.meta && !text.chars().any(char::is_control) =>
        {
            Some(InputEvent::Text(text.to_owned()))
        }
        (KeyState::Down | KeyState::Up, _) => None,
    };
    let key = key(logical).map(|key| InputEvent::Key {
        key,
        state: key_state,
        mods,
    });
    key.into_iter().chain(typed).collect()
}

/// An input-method event. An empty preedit is winit's "composition cleared" (the kit's `Cancel`); enable/disable
/// carry nothing for a field (the host turns IME on and off: `TextEntry`).
pub fn ime_event(ime: &Ime) -> Option<ImeEvent> {
    match ime {
        Ime::Preedit(text, _) if text.is_empty() => Some(ImeEvent::Cancel),
        Ime::Preedit(text, _) => Some(ImeEvent::Preedit(text.clone())),
        Ime::Commit(text) => Some(ImeEvent::Commit(text.clone())),
        Ime::Enabled | Ime::Disabled => None,
    }
}

/// A wheel turn at `at` (logical px). winit's deltas are positive when content should move right/down (the wheel
/// turned up); the kit's, like the browser's, are positive when the view scrolls down, so the sign flips. Pixel
/// deltas are physical px and are divided by the scale; line deltas are lines.
pub fn wheel(delta: MouseScrollDelta, at: Point, scale: f64) -> InputEvent {
    let (delta, unit) = match delta {
        MouseScrollDelta::LineDelta(x, y) => {
            (Vec2::new(-f64::from(x), -f64::from(y)), DeltaUnit::Lines)
        }
        MouseScrollDelta::PixelDelta(p) => {
            (Vec2::new(-p.x / scale, -p.y / scale), DeltaUnit::Pixels)
        }
    };
    InputEvent::Wheel { at, delta, unit }
}

/// One finger. Pressure is winit's normalised force, or 1 where the platform reports none.
pub fn touch(
    id: u64,
    phase: TouchPhase,
    location: PhysicalPosition<f64>,
    force: Option<Force>,
    scale: f64,
) -> InputEvent {
    InputEvent::Pointer(PointerEvent {
        id: PointerId(id),
        kind: PointerKind::Touch,
        phase: match phase {
            TouchPhase::Started => Phase::Down,
            TouchPhase::Moved => Phase::Move,
            TouchPhase::Ended => Phase::Up,
            TouchPhase::Cancelled => Phase::Cancel,
        },
        at: logical(location, scale),
        pressure: force.map_or(1.0, |force| force.normalized() as f32),
        button: PointerButton::Primary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer(event: &InputEvent) -> (Phase, Point) {
        match event {
            InputEvent::Pointer(p) => (p.phase, p.at),
            other => panic!("expected a pointer event, got {other:?}"),
        }
    }

    /// winit reports physical px: at scale 2 a physical (300, 120) is the kit's (150, 60), and a press (which
    /// carries no position) lands there too. A missed division hits controls at twice their position; a press
    /// at the stale cursor of another scale misses them.
    #[test]
    fn pointer_positions_and_the_viewport_are_logical() {
        let mut input = Input::new(PhysicalSize::new(1600, 1200), 2.0);
        assert_eq!(input.viewport().size, Size::new(800.0, 600.0));
        let moved = input.cursor_moved(PhysicalPosition::new(300.0, 120.0));
        assert_eq!(pointer(&moved), (Phase::Move, Point::new(150.0, 60.0)));
        assert_eq!(
            pointer(&input.mouse(Phase::Down, PointerButton::Primary)),
            (Phase::Down, Point::new(150.0, 60.0))
        );
        let finger = touch(
            7,
            TouchPhase::Ended,
            PhysicalPosition::new(300.0, 120.0),
            None,
            2.0,
        );
        assert_eq!(pointer(&finger), (Phase::Up, Point::new(150.0, 60.0)));
        // Dragged to a scale-1 display: the same physical size is now twice the logical size.
        let InputEvent::Resize(viewport) = input.rescaled(1.0) else {
            panic!("a scale change resends the viewport")
        };
        assert_eq!(
            (viewport.size, viewport.scale_factor),
            (Size::new(1600.0, 1200.0), 1.0)
        );
        let moved = input.cursor_moved(PhysicalPosition::new(300.0, 120.0));
        assert_eq!(pointer(&moved).1, Point::new(300.0, 120.0));
    }

    /// Left, right and middle reach the router as primary, secondary and middle. A swapped mapping makes a right-click
    /// fire a control's primary action (or a left-click open its context menu); the side buttons are dropped, not
    /// guessed at.
    #[test]
    fn mouse_buttons_map_to_the_kit_buttons() {
        let mut input = Input::new(PhysicalSize::new(800, 600), 1.0);
        let press = |input: &mut Input, button, state| {
            let event = WindowEvent::MouseInput {
                device_id: winit::event::DeviceId::dummy(),
                state,
                button,
            };
            input.convert(&event)
        };
        let button_of = |events: Vec<InputEvent>| match events.as_slice() {
            [InputEvent::Pointer(p)] => (p.phase, p.button),
            other => panic!("expected one pointer event, got {other:?}"),
        };
        for (winit_button, button) in [
            (MouseButton::Left, PointerButton::Primary),
            (MouseButton::Right, PointerButton::Secondary),
            (MouseButton::Middle, PointerButton::Middle),
        ] {
            assert_eq!(
                button_of(press(&mut input, winit_button, ElementState::Pressed)),
                (Phase::Down, button)
            );
            assert_eq!(
                button_of(press(&mut input, winit_button, ElementState::Released)),
                (Phase::Up, button)
            );
        }
        assert!(press(&mut input, MouseButton::Back, ElementState::Pressed).is_empty());
    }

    /// Lines stay lines; pixels are physical and halve at scale 2; both flip sign (winit: positive = wheel up,
    /// the kit: positive = scroll down).
    #[test]
    fn wheel_lines_pass_through_and_pixels_are_scaled() {
        let at = Point::new(5.0, 5.0);
        let lines = wheel(MouseScrollDelta::LineDelta(0.0, 3.0), at, 2.0);
        assert_eq!(
            lines,
            InputEvent::Wheel {
                at,
                delta: Vec2::new(0.0, -3.0),
                unit: DeltaUnit::Lines
            }
        );
        let pixels = wheel(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(10.0, -40.0)),
            at,
            2.0,
        );
        assert_eq!(
            pixels,
            InputEvent::Wheel {
                at,
                delta: Vec2::new(-5.0, 20.0),
                unit: DeltaUnit::Pixels
            }
        );
    }

    /// Cmd (winit's `super`) is the kit's `meta`, never `ctrl`: a swapped mapping turns Cmd+V into a plain V on a
    /// Mac and types a "v" into the field instead of pasting.
    #[test]
    fn super_is_meta_and_command_chords_type_nothing() {
        let mods = modifiers(ModifiersState::SUPER | ModifiersState::SHIFT);
        assert_eq!(
            mods,
            Modifiers {
                shift: true,
                ctrl: false,
                alt: false,
                meta: true
            }
        );
        let v = WinitKey::Character("v".into());
        assert_eq!(
            key_events(&v, ElementState::Pressed, Some("v"), mods),
            vec![InputEvent::Key {
                key: Key::character('v'),
                state: KeyState::Down,
                mods
            }]
        );
    }

    /// A typed capital is the key 'a' (bindings ignore case) plus the text "A"; Enter's "\r" and a release type
    /// nothing; Space is a key and a space.
    #[test]
    fn a_press_is_its_key_then_its_printable_text() {
        let shift = modifiers(ModifiersState::SHIFT);
        let a = WinitKey::Character("A".into());
        assert_eq!(
            key_events(&a, ElementState::Pressed, Some("A"), shift),
            vec![
                InputEvent::Key {
                    key: Key::Character('a'),
                    state: KeyState::Down,
                    mods: shift
                },
                InputEvent::Text("A".into()),
            ]
        );
        let none = Modifiers::default();
        let enter = WinitKey::Named(WinitNamed::Enter);
        assert_eq!(
            key_events(&enter, ElementState::Pressed, Some("\r"), none),
            vec![InputEvent::Key {
                key: Key::Named(NamedKey::Enter),
                state: KeyState::Down,
                mods: none
            }]
        );
        assert_eq!(
            key_events(&a, ElementState::Released, Some("A"), shift).len(),
            1,
            "a release types nothing"
        );
        let space = WinitKey::Named(WinitNamed::Space);
        assert_eq!(
            key_events(&space, ElementState::Pressed, Some(" "), none)[1],
            InputEvent::Text(" ".into())
        );
    }

    /// Preedit is shown, never inserted; only a commit inserts. An empty preedit is a cancelled composition, not an
    /// empty one (which would leave the field in composing mode).
    #[test]
    fn ime_preedit_commit_and_cancel() {
        assert_eq!(
            ime_event(&Ime::Preedit("ni".into(), Some((2, 2)))),
            Some(ImeEvent::Preedit("ni".into()))
        );
        assert_eq!(
            ime_event(&Ime::Commit("你".into())),
            Some(ImeEvent::Commit("你".into()))
        );
        assert_eq!(
            ime_event(&Ime::Preedit(String::new(), None)),
            Some(ImeEvent::Cancel)
        );
        assert_eq!(ime_event(&Ime::Enabled), None);
    }
}
