//! DOM event payloads -> kit `InputEvent`s. Every function takes plain values (a `KeyboardEvent.key` string, a pointer's
//! numbers), never `web_sys` types, so it is tested natively; the wasm host reads the numbers off the event and calls it.
//!
//! Units: the kit works in logical px, and the host lays the canvas out at its CSS size (device scale `dpr`), so a
//! position relative to the canvas in CSS px is already a logical position. Nothing here divides by the scale.
use kit_core::Viewport;
use kit_core::kurbo::{Point, Size, Vec2};
use kit_input::{
    DeltaUnit, InputEvent, Key, KeyState, Modifiers, NamedKey, Phase, Platform, PointerButton,
    PointerEvent, PointerId, PointerKind,
};

/// The mouse's pointer id: the same as `kit_winit`'s. Touches and pens use the DOM's `pointerId`, which is unique among
/// live pointers and small, so none can share the mouse's.
pub const MOUSE: PointerId = PointerId(u64::MAX);

/// The kit key a `KeyboardEvent.key` names. `None` is the meaning: a key the kit never binds (function keys, dead keys,
/// an IME's `Process`, `Unidentified`) is not the kit's to route. Letters are one key in both cases (`'M'` is `'m'`) and
/// Space is the named key, as in `kit_winit`.
pub fn key_from_dom(dom_key: &str) -> Option<Key> {
    let mut chars = dom_key.chars();
    match (chars.next(), chars.next()) {
        (Some(' '), None) => Some(Key::Named(NamedKey::Space)),
        (Some(c), None) => Some(Key::character(c)),
        (Some(_), Some(_)) => named_key(dom_key).map(Key::Named),
        (None, _) => None,
    }
}

fn named_key(dom_key: &str) -> Option<NamedKey> {
    Some(match dom_key {
        "Escape" => NamedKey::Escape,
        "Enter" => NamedKey::Enter,
        "Tab" => NamedKey::Tab,
        "Backspace" => NamedKey::Backspace,
        "Delete" => NamedKey::Delete,
        "ArrowLeft" => NamedKey::ArrowLeft,
        "ArrowRight" => NamedKey::ArrowRight,
        "ArrowUp" => NamedKey::ArrowUp,
        "ArrowDown" => NamedKey::ArrowDown,
        "Home" => NamedKey::Home,
        "End" => NamedKey::End,
        // The DOM names about 250 keys; the kit names the eleven above (Space is a single character here).
        _ => return None,
    })
}

/// One key transition as the kit's `Key` event, or nothing when the kit has no name for the key. Unlike `kit_winit`
/// this never adds a `Text` event: typed text reaches a browser app through the hidden textarea (see [`crate::text`]),
/// which is also where IME and dead keys are resolved.
pub fn key_event(dom_key: &str, state: KeyState, mods: Modifiers) -> Option<InputEvent> {
    key_from_dom(dom_key).map(|key| InputEvent::Key { key, state, mods })
}

/// The DOM's `PointerEvent.pointerType`, as the three devices the kit knows. The DOM allows the empty string and
/// user-agent-defined types for a device it cannot name: those are the mouse, as in `kit_winit`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DomDevice {
    Mouse,
    Touch,
    Pen,
}

pub fn pointer_device(pointer_type: &str) -> DomDevice {
    match pointer_type {
        "touch" => DomDevice::Touch,
        "pen" => DomDevice::Pen,
        _unknown_or_mouse => DomDevice::Mouse,
    }
}

/// `MouseEvent.button`, named. The kit's pointer has three buttons; back and forward (the mouse's side buttons) are
/// here so a host that reads the DOM itself can decide what they mean. [`pointer`] drops them, as it always has.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DomButton {
    /// -1: no button changed (a move).
    Unchanged,
    Primary,
    Middle,
    Secondary,
    Back,
    Forward,
    /// Any other number: the fifth-and-up buttons, and a pen's eraser (5). Carried as the DOM sent it, not guessed at.
    Other(i16),
}

impl DomButton {
    pub fn of(dom_button: i16) -> Self {
        match dom_button {
            -1 => DomButton::Unchanged,
            0 => DomButton::Primary,
            1 => DomButton::Middle,
            2 => DomButton::Secondary,
            3 => DomButton::Back,
            4 => DomButton::Forward,
            other => DomButton::Other(other),
        }
    }

    /// The kit's button, for the three it has. Back, forward and the rest are the host's to handle.
    pub fn kit(self) -> Option<PointerButton> {
        match self {
            DomButton::Primary => Some(PointerButton::Primary),
            DomButton::Middle => Some(PointerButton::Middle),
            DomButton::Secondary => Some(PointerButton::Secondary),
            DomButton::Unchanged | DomButton::Back | DomButton::Forward | DomButton::Other(_) => {
                None
            }
        }
    }
}

/// The numbers of one DOM `PointerEvent`.
#[derive(Copy, Clone, Debug)]
pub struct DomPointer<'a> {
    /// `PointerEvent.pointerType`: `"mouse"`, `"pen"` or `"touch"` (see [`pointer_device`]).
    pub pointer_type: &'a str,
    pub id: i32,
    /// `MouseEvent.button`: which button changed on a down or up (0 left, 1 middle, 2 right, 3 back, 4 forward). On a
    /// move it is -1 and unread.
    pub button: i16,
    /// CSS px from the canvas's top-left corner.
    pub at: Point,
    pub pressure: f32,
    pub mods: Modifiers,
}

/// The kit events for one pointer phase. A press or release is preceded by `Modifiers` (the DOM puts them on the pointer
/// event, and a press binding such as shift-click reads them from the router). A mouse side button (back, forward) is
/// not one of the kit's three and gives nothing, on press and release alike.
pub fn pointer(phase: Phase, dom: &DomPointer<'_>) -> Vec<InputEvent> {
    let (kind, id) = match pointer_device(dom.pointer_type) {
        DomDevice::Touch => (
            PointerKind::Touch,
            PointerId(u64::from(dom.id.unsigned_abs())),
        ),
        DomDevice::Pen => (
            PointerKind::Pen,
            PointerId(u64::from(dom.id.unsigned_abs())),
        ),
        DomDevice::Mouse => (PointerKind::Mouse, MOUSE),
    };
    let pressure = match kind {
        // The DOM reports 0.5 for a pressed mouse and 0 otherwise: not a force. `kit_winit` also sends 1.
        PointerKind::Mouse => 1.0,
        PointerKind::Touch | PointerKind::Pen => dom.pressure,
    };
    let button = match (phase, kind) {
        (Phase::Down | Phase::Up, PointerKind::Mouse) => DomButton::of(dom.button).kit(),
        // Touch and pen contact is the primary button whatever `button` says (a pen's barrel and eraser are not presses).
        (Phase::Down | Phase::Up, PointerKind::Touch | PointerKind::Pen)
        | (Phase::Move | Phase::Cancel | Phase::Leave, _) => Some(PointerButton::Primary),
    };
    let event = |button| {
        InputEvent::Pointer(PointerEvent {
            id,
            kind,
            phase,
            at: dom.at,
            pressure,
            button,
        })
    };
    match (phase, button) {
        (_, None) => Vec::new(),
        (Phase::Down | Phase::Up, Some(button)) => {
            vec![InputEvent::Modifiers(dom.mods), event(button)]
        }
        (Phase::Move | Phase::Cancel | Phase::Leave, Some(button)) => vec![event(button)],
    }
}

/// A host's own identity for a DOM pointer: `kit_winit`'s mouse id is [`MOUSE`], the game's is its own scheme. The host
/// implements this and [`identify_pointer`] reads the device and button for it. `&mut self` lets a host keep state
/// (a slot per live touch, say); a stateless one ignores it.
pub trait PointerIdentity {
    type Id: Copy + Eq + std::fmt::Debug;
    fn identify(&mut self, device: DomDevice, pointer_id: i32) -> Self::Id;
}

/// One DOM pointer as the host names it: its identity, the device, and the button (side buttons included).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IdentifiedPointer<I> {
    pub id: I,
    pub device: DomDevice,
    pub button: DomButton,
}

/// The host's identity for a DOM pointer (`pointerId`, `pointerType`, `button`), with the device and the button read
/// the same way [`pointer`] reads them, so the two paths cannot disagree about which device an event came from.
pub fn identify_pointer<P: PointerIdentity>(
    identity: &mut P,
    pointer_type: &str,
    pointer_id: i32,
    button: i16,
) -> IdentifiedPointer<P::Id> {
    let device = pointer_device(pointer_type);
    IdentifiedPointer {
        id: identity.identify(device, pointer_id),
        device,
        button: DomButton::of(button),
    }
}

/// A `WheelEvent.deltaMode` the spec does not define. Named, so a host can log it rather than guess a unit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UnknownDeltaMode(pub u32);

impl std::fmt::Display for UnknownDeltaMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WheelEvent.deltaMode {} is not in the spec", self.0)
    }
}

/// The kit's unit for a `WheelEvent.deltaMode`: 0 pixels (CSS px, already logical), 1 lines, 2 pages.
pub fn delta_unit(delta_mode: u32) -> Result<DeltaUnit, UnknownDeltaMode> {
    match delta_mode {
        0 => Ok(DeltaUnit::Pixels),
        1 => Ok(DeltaUnit::Lines),
        2 => Ok(DeltaUnit::Pages),
        other => Err(UnknownDeltaMode(other)),
    }
}

/// A wheel turn at `at` (CSS px from the canvas). The DOM's deltas are positive when the view scrolls down or right, as
/// the kit's are, so no sign flips (winit's are the other way round). A `delta_mode` the spec does not define is an error
/// the host reports; no turn is made up for it.
pub fn wheel(
    delta_x: f64,
    delta_y: f64,
    delta_mode: u32,
    at: Point,
) -> Result<InputEvent, UnknownDeltaMode> {
    Ok(InputEvent::Wheel {
        at,
        delta: Vec2::new(delta_x, delta_y),
        unit: delta_unit(delta_mode)?,
    })
}

/// The canvas's drawing-buffer size for `css` px at `dpr` device px per CSS px: whole pixels, at least one (a zero-sized
/// WebGPU surface is an error, and a hidden canvas measures 0).
pub fn backing_size(css: (f64, f64), dpr: f64) -> (u32, u32) {
    let px = |css: f64| (css * dpr).round().max(1.0) as u32;
    (px(css.0), px(css.1))
}

/// The viewport for a canvas of `css` px drawn into a `backing_size` buffer. The scale is the buffer's actual ratio, not
/// `dpr`: at a fractional ratio the buffer is rounded to whole pixels, and a scale from `dpr` would draw the UI slightly
/// larger or smaller than its buffer (and put every edge off the pixel grid). A zero CSS size lays out as 1 px.
pub fn viewport(css: (f64, f64), dpr: f64) -> Viewport {
    let (width, _height) = backing_size(css, dpr);
    let css_width = css.0.max(1.0);
    let scale = f64::from(width) / css_width;
    Viewport::new(Size::new(css_width, css.1.max(1.0)), scale)
}

/// Which modifier does what in text editing, from what the browser says about the machine: `navigator.userAgentData
/// .platform`, `navigator.platform` and `navigator.userAgent`, in any order, empty when absent. iPadOS reports `MacIntel`,
/// which is right: it has Cmd. A browser that says nothing is `Other` (a Ctrl chord is the safe default off Apple).
pub fn platform_of(witnesses: &[&str]) -> Platform {
    let apple = |text: &&str| {
        // `userAgentData.platform` says "macOS" and `navigator.platform` says "MacIntel": the case differs.
        let text = text.to_ascii_lowercase();
        ["mac", "iphone", "ipad", "ipod", "ios"]
            .iter()
            .any(|name| text.contains(name))
    };
    match witnesses.iter().any(apple) {
        true => Platform::Apple,
        false => Platform::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    };

    fn mouse(button: i16) -> DomPointer<'static> {
        DomPointer {
            pointer_type: "mouse",
            id: 1,
            button,
            at: Point::new(30.0, 40.0),
            pressure: 0.5,
            mods: NONE,
        }
    }

    fn buttons(events: &[InputEvent]) -> Vec<(Phase, PointerButton)> {
        events
            .iter()
            .filter_map(|event| match event {
                InputEvent::Pointer(p) => Some((p.phase, p.button)),
                _ => None,
            })
            .collect()
    }

    /// DOM numbers the buttons left 0, middle 1, right 2. A swapped table fires a right-click's context gesture on the
    /// left button (or the reverse); the side buttons are dropped, press and release alike, not guessed at.
    #[test]
    fn dom_mouse_buttons_map_to_the_kits_and_the_side_buttons_are_dropped() {
        for (dom, kit) in [
            (0, PointerButton::Primary),
            (1, PointerButton::Middle),
            (2, PointerButton::Secondary),
        ] {
            assert_eq!(
                buttons(&pointer(Phase::Down, &mouse(dom))),
                [(Phase::Down, kit)]
            );
            assert_eq!(
                buttons(&pointer(Phase::Up, &mouse(dom))),
                [(Phase::Up, kit)]
            );
        }
        assert!(pointer(Phase::Down, &mouse(3)).is_empty());
        assert!(pointer(Phase::Up, &mouse(4)).is_empty());
        // A move carries button -1 in the DOM: still delivered, as the primary button the router ignores.
        assert_eq!(
            buttons(&pointer(Phase::Move, &mouse(-1))),
            [(Phase::Move, PointerButton::Primary)]
        );
    }

    /// A shift-click binding reads the modifiers the router last saw, so the press must be preceded by them.
    #[test]
    fn a_press_carries_its_modifiers_first() {
        let shift = Modifiers {
            shift: true,
            ..NONE
        };
        let events = pointer(
            Phase::Down,
            &DomPointer {
                mods: shift,
                ..mouse(0)
            },
        );
        assert_eq!(events[0], InputEvent::Modifiers(shift));
        assert!(matches!(events[1], InputEvent::Pointer(_)));
        assert_eq!(events.len(), 2);
    }

    /// Fingers and the pen keep their DOM ids and kinds and always press the primary button (the DOM says 0 for touch
    /// contact, but a pen's barrel or eraser reports other numbers). Unknown pointer types are the mouse, on its id.
    #[test]
    fn touch_and_pen_are_primary_and_unknown_devices_are_the_mouse() {
        let touch = DomPointer {
            pointer_type: "touch",
            id: 7,
            button: 5,
            pressure: 0.25,
            ..mouse(0)
        };
        let [InputEvent::Modifiers(_), InputEvent::Pointer(p)] = pointer(Phase::Down, &touch)[..]
        else {
            panic!("a touch press is modifiers then a pointer")
        };
        assert_eq!(
            (p.id, p.kind, p.button, p.pressure),
            (
                PointerId(7),
                PointerKind::Touch,
                PointerButton::Primary,
                0.25
            )
        );
        let pen = DomPointer {
            pointer_type: "pen",
            id: 9,
            ..touch
        };
        assert!(matches!(
            pointer(Phase::Move, &pen)[..],
            [InputEvent::Pointer(PointerEvent {
                id: PointerId(9),
                kind: PointerKind::Pen,
                ..
            })]
        ));
        let unknown = DomPointer {
            pointer_type: "",
            id: 3,
            ..mouse(0)
        };
        let [_, InputEvent::Pointer(p)] = pointer(Phase::Down, &unknown)[..] else {
            panic!("a press is modifiers then a pointer")
        };
        assert_eq!((p.id, p.kind, p.pressure), (MOUSE, PointerKind::Mouse, 1.0));
    }

    /// Unlike winit, the DOM's wheel deltas already point the kit's way (positive = scroll down), and pixel deltas are
    /// CSS px, which are logical px. A flipped sign scrolls every list backwards; a scaled pixel delta scrolls it by
    /// the display density.
    #[test]
    fn wheel_deltas_pass_through_with_their_unit() {
        let at = Point::new(5.0, 6.0);
        assert_eq!(
            wheel(1.0, 53.0, 0, at),
            Ok(InputEvent::Wheel {
                at,
                delta: Vec2::new(1.0, 53.0),
                unit: DeltaUnit::Pixels
            })
        );
        assert!(matches!(
            wheel(0.0, 3.0, 1, at),
            Ok(InputEvent::Wheel {
                unit: DeltaUnit::Lines,
                ..
            })
        ));
        assert!(matches!(
            wheel(0.0, 1.0, 2, at),
            Ok(InputEvent::Wheel {
                unit: DeltaUnit::Pages,
                ..
            })
        ));
        // A made-up unit would scroll by the wrong amount: the mode is reported, and the turn is not sent.
        assert_eq!(wheel(0.0, 1.0, 3, at), Err(UnknownDeltaMode(3)));
    }

    /// The host that feeds its own input maps `deltaMode` without an event. Unknown modes are an error, not pixels.
    #[test]
    fn delta_modes_map_to_units_and_unknown_modes_are_reported() {
        assert_eq!(delta_unit(0), Ok(DeltaUnit::Pixels));
        assert_eq!(delta_unit(1), Ok(DeltaUnit::Lines));
        assert_eq!(delta_unit(2), Ok(DeltaUnit::Pages));
        assert_eq!(delta_unit(7), Err(UnknownDeltaMode(7)));
    }

    /// The side buttons reach a host that asks for them (back and forward are not the kit's, but they are presses a
    /// game may bind), while the kit's own path still sends nothing for them. A host that routed side buttons through
    /// `kit()` would lose them here.
    #[test]
    fn a_host_sees_the_side_buttons_the_kit_does_not() {
        struct Raw;
        impl PointerIdentity for Raw {
            type Id = i32;
            fn identify(&mut self, _device: DomDevice, pointer_id: i32) -> i32 {
                pointer_id
            }
        }
        assert_eq!(
            identify_pointer(&mut Raw, "mouse", 0, 3).button,
            DomButton::Back
        );
        assert_eq!(
            identify_pointer(&mut Raw, "mouse", 0, 4).button,
            DomButton::Forward
        );
        assert_eq!(
            identify_pointer(&mut Raw, "mouse", 0, 5).button,
            DomButton::Other(5)
        );
        assert!(pointer(Phase::Down, &mouse(3)).is_empty());
    }

    /// The host's identity gets the device the DOM named, with the same unknown-type rule `pointer` uses: an unnamed
    /// device is the mouse, never a touch. A mapping that read the type differently would split one pointer in two.
    #[test]
    fn identify_reads_the_device_the_way_pointer_does() {
        struct Echo;
        impl PointerIdentity for Echo {
            type Id = (DomDevice, i32);
            fn identify(&mut self, device: DomDevice, pointer_id: i32) -> (DomDevice, i32) {
                (device, pointer_id)
            }
        }
        assert_eq!(
            identify_pointer(&mut Echo, "", 3, 0).id,
            (DomDevice::Mouse, 3)
        );
        assert_eq!(
            identify_pointer(&mut Echo, "touch", 7, 0).id,
            (DomDevice::Touch, 7)
        );
        let pen = identify_pointer(&mut Echo, "pen", 9, 0);
        assert_eq!((pen.id, pen.device), ((DomDevice::Pen, 9), DomDevice::Pen));
        let [_, InputEvent::Pointer(p)] = pointer(
            Phase::Down,
            &DomPointer {
                pointer_type: "",
                id: 3,
                ..mouse(0)
            },
        )[..] else {
            panic!("a press is modifiers then a pointer")
        };
        assert_eq!(p.id, MOUSE);
    }

    /// `KeyboardEvent.key` names: case and Shift must not matter (a hotkey is `1`, not `!`... the key is whatever the
    /// layout typed, lower-cased), Space is the named key, and everything the kit has no name for is not the kit's.
    #[test]
    fn dom_keys_map_to_kit_keys_without_regard_to_case() {
        assert_eq!(key_from_dom("M"), key_from_dom("m"));
        assert_eq!(key_from_dom("m"), Some(Key::Character('m')));
        assert_eq!(key_from_dom(" "), Some(Key::Named(NamedKey::Space)));
        assert_eq!(
            key_from_dom("ArrowDown"),
            Some(Key::Named(NamedKey::ArrowDown))
        );
        assert_eq!(key_from_dom("Enter"), Some(Key::Named(NamedKey::Enter)));
        for unnamed in ["F5", "Unidentified", "Dead", "Process", "Shift", ""] {
            assert_eq!(key_from_dom(unnamed), None, "{unnamed:?}");
        }
        let ctrl = Modifiers { ctrl: true, ..NONE };
        assert_eq!(
            key_event("c", KeyState::Down, ctrl),
            Some(InputEvent::Key {
                key: Key::Character('c'),
                state: KeyState::Down,
                mods: ctrl
            })
        );
        assert_eq!(key_event("F5", KeyState::Down, NONE), None);
    }

    /// Regression guard for the scale: at 1.25 a 801 px canvas needs 1001.25 buffer px, which rounds to 1001, and the
    /// UI must be drawn at exactly 1001/801, not 1.25, or it is 0.25 px too big at the right edge.
    #[test]
    fn the_viewport_scale_is_the_buffers_actual_ratio() {
        assert_eq!(backing_size((801.0, 600.0), 1.25), (1001, 750));
        let viewport = viewport((801.0, 600.0), 1.25);
        assert_eq!(viewport.size, Size::new(801.0, 600.0));
        assert!((viewport.scale_factor * 801.0 - 1001.0).abs() < 1e-9);
        // A hidden canvas measures 0: one pixel, not a zero-sized (invalid) surface or a division by zero.
        assert_eq!(backing_size((0.0, 0.0), 2.0), (1, 1));
        assert!(viewport_is_finite((0.0, 0.0)));
    }

    fn viewport_is_finite(css: (f64, f64)) -> bool {
        let viewport = viewport(css, 2.0);
        viewport.scale_factor.is_finite() && viewport.size.width >= 1.0
    }

    /// Cmd or Ctrl in text editing follows the browser's own words. iPadOS says `MacIntel` and has Cmd; Windows, Linux and
    /// Android do not; a browser that says nothing gets Ctrl.
    #[test]
    fn platform_follows_the_browsers_witnesses() {
        assert_eq!(
            platform_of(&["", "MacIntel", "Mozilla/5.0 (Macintosh)"]),
            Platform::Apple
        );
        assert_eq!(platform_of(&["macOS", "", ""]), Platform::Apple);
        assert_eq!(
            platform_of(&["", "", "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0)"]),
            Platform::Apple
        );
        assert_eq!(
            platform_of(&["Windows", "Win32", "Mozilla/5.0 (Windows NT 10.0)"]),
            Platform::Other
        );
        assert_eq!(
            platform_of(&["", "Linux armv81", "Mozilla/5.0 (Linux; Android 14)"]),
            Platform::Other
        );
        assert_eq!(platform_of(&["", "", ""]), Platform::Other);
    }
}
