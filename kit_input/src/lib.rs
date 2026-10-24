//! Seam: normalised, platform-free input. Platform adapters (winit, web canvas, iOS) produce `InputEvent`s; the
//! `Router` turns them plus a `LaidOut` tree into typed app messages, and all retained UI state lives in `UiMemory`.

mod clipboard;
mod event;
mod field;
mod focus;
mod hit;
mod layers;
mod memory;
mod nav;
mod router;
mod scroll;
mod track;

pub use clipboard::{
    Clipboard, ClipboardError, ClipboardRead, ClipboardRequest, PasteTicket, Serviced, service,
};
pub use event::{
    DeltaUnit, ImeEvent, InputEvent, Key, KeyState, Modifiers, NamedKey, NavAction, NavDirection,
    Phase, PointerButton, PointerEvent, PointerId, PointerKind,
};
pub use field::{Granularity, Platform, TextTarget, paste_ticket, text_target};
pub use focus::FocusOutcome;
pub use hit::visible_rect;
pub use memory::{AnimKey, DragView, Focus, Gesture, Hover, PointerState, UiMemory};
pub use router::{GestureConfig, Router, Routing};
pub use scroll::{BAR_STRIP, BAR_THIN, Bar, BarView, Grip, Scrolls};
