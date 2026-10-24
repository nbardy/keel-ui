//! The kit on a winit window: window events in, [`kit_input::InputEvent`]s out, plus the two things a native host
//! owes the kit's text fields (an OS clipboard and the IME candidate-window position).
//!
//! Small on purpose: copy it into your crate when you need a different policy (another mouse button as a press,
//! another pointer-id scheme).
//!
//! ```ignore
//! let mut input = kit_winit::Input::new(window.inner_size(), window.scale_factor());
//! // in `ApplicationHandler::window_event`:
//! pending.extend(input.convert(&event));          // route these against the frame the player saw
//! // after routing, once per frame:
//! let served = kit_input::service(memory.take_clipboard(), &mut clipboard); // clipboard: ArboardClipboard
//! text_entry.sync(&window, &kit_input::text_target(&laid, &memory));         // IME on/off and its caret
//! ```
//!
//! Everything [`Input::convert`] does is one of the pure functions below, so the conversions are tested without a
//! window (winit's `DeviceId` and `KeyEvent` cannot be built outside winit).
//!
//! The kit has no cursor-icon output, so there is nothing to set on the window for it.

mod clipboard;
mod convert;
mod ime;

pub use clipboard::ArboardClipboard;
pub use convert::{
    Input, MOUSE, ime_event, key, key_events, logical, modifiers, touch, viewport, wheel,
};
pub use ime::{ImeRequest, TextEntry, ime_request};
