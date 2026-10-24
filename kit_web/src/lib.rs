//! The kit in a browser tab: DOM events in, [`kit_input::InputEvent`]s out, a hidden textarea for the IME and the
//! clipboard, and a canvas host loop that draws a frame only when something asks for one.
//!
//! The browser counterpart of `kit_winit`. Like it, the work is in small pure functions over plain values (a key
//! string, a pointer's numbers) so the conversions are tested natively without a browser; `cfg(target_arch =
//! "wasm32")` code only reads those values off DOM events and calls them.
//!
//! ```ignore
//! // wasm only: `Gpu` is the device, queue and colour format the host made for the canvas.
//! let running = kit_web::start(canvas, |gpu, viewport, platform| MyApp::new(gpu, viewport, platform)).await?;
//! ```
//!
//! `MyApp` implements [`App`]: one `frame` per draw, which routes the events, draws, and answers with the next
//! [`Redraw`](kit_core::Redraw). An idle page stops asking the browser for animation frames.
//!
//! What the browser makes you do differently from a native host, in one place:
//! - Input is queued by the DOM callbacks and routed at the next frame (as in the native starter), so a callback
//!   cannot know if the kit took a key; unmodified keys the kit names are `preventDefault`ed (Tab, Space, arrows)
//!   because the page never scrolls or tabs away from the canvas.
//! - Text comes from a hidden `<textarea>` that follows the caret ([`text`]), not from key events.
//! - The clipboard read is asynchronous: a paste arrives as the textarea's `paste` event.
//!
//! A host with its own input layer (a game that already routes its own DOM events) uses the pieces without [`start`]:
//! the conversions in [`convert`] and [`text`] take the DOM's numbers; [`identify_pointer`] maps a DOM pointer to the
//! host's own identity, side buttons included; [`delta_unit`] maps a wheel's `deltaMode`; and on wasm, [`TextBridge`]
//! and [`WebClipboard`] put the hidden textarea and the clipboard over the host's own [`Paste`] state, and [`platform`]
//! reads the browser's platform with a console report for each lookup that fails.
pub mod convert;
pub mod schedule;
pub mod text;

pub use convert::{
    DomButton, DomDevice, DomPointer, IdentifiedPointer, MOUSE, PointerIdentity, UnknownDeltaMode,
    backing_size, delta_unit, identify_pointer, key_event, key_from_dom, platform_of, pointer,
    pointer_device, viewport, wheel,
};
pub use schedule::{Wake, wake};
pub use text::{Composer, Entry, Keydown, Paste, Reported, entry_events, is_paste_chord, key_for};

#[cfg(target_arch = "wasm32")]
mod host;
#[cfg(target_arch = "wasm32")]
mod textarea;

#[cfg(target_arch = "wasm32")]
pub use host::{App, FrameInput, FrameOutput, Gpu, Running, log_panics, platform, start};
#[cfg(target_arch = "wasm32")]
pub use textarea::{
    TextBridge, WebClipboard, WriteOutcome, key_modifiers, mouse_modifiers, write_text,
};
