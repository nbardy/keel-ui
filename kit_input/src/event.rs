use crate::PasteTicket;
use kit_core::Viewport;
use kit_core::kurbo::{Insets, Point, Vec2};
pub use kit_core::{Key, Modifiers, NamedKey};

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct PointerId(pub u64);
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum PointerKind {
    Mouse,
    Touch,
    Pen,
}
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Phase {
    Down,
    Move,
    Up,
    /// The platform took the pointer away (touch cancelled, alt-tab).
    Cancel,
    /// Left the window / surface without a button transition.
    Leave,
}
/// Which button a pointer press belongs to. Touch and pen contact are `Primary`.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum PointerButton {
    Primary,
    /// Right button: the router's context gesture (`Bindings::on_context`).
    Secondary,
    Middle,
}
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct PointerEvent {
    pub id: PointerId,
    pub kind: PointerKind,
    pub phase: Phase,
    /// Logical px.
    pub at: Point,
    pub pressure: f32,
    /// Read for `Phase::Down` and `Phase::Up`, where it names the button that changed. `Move`, `Cancel` and `Leave`
    /// are not about a button; hosts send `Primary` on them and the router ignores it.
    pub button: PointerButton,
}
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum DeltaUnit {
    Pixels,
    Lines,
    Pages,
}
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum KeyState {
    Down,
    Up,
}
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum ImeEvent {
    Preedit(String),
    Commit(String),
    /// Composition abandoned.
    Cancel,
}
#[derive(Clone, PartialEq, Debug)]
pub enum InputEvent {
    Pointer(PointerEvent),
    Wheel {
        at: Point,
        delta: Vec2,
        unit: DeltaUnit,
    },
    /// A logical key. Shortcuts match on this, so a binding follows the keyboard layout the player types on.
    Key {
        key: Key,
        state: KeyState,
        mods: Modifiers,
    },
    /// The modifier keys held now. Hosts send it when they change (or before a pointer press) so a press binding
    /// built with `Bindings::activate_with` sees shift/cmd-click; every `Key` event also updates them.
    Modifiers(Modifiers),
    /// Committed text.
    Text(String),
    Ime(ImeEvent),
    /// Clipboard text for a paste the UI asked for (see `UiMemory::take_clipboard`).
    Paste {
        ticket: PasteTicket,
        text: String,
    },
    /// Window focus gained/lost.
    Focus(bool),
    Resize(Viewport),
    ScaleFactor(f64),
    SafeArea(Insets),
}
