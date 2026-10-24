//! Where text entry is, told to the window: IME on only while a kit text field has focus, its candidate window at
//! the caret, and the password purpose for a secret field (no suggestions or history).
use kit_input::TextTarget;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::window::{ImePurpose, Window};

/// What the window should do with its input method this frame.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum ImeRequest {
    Off,
    /// The caret, in logical px (the kit's unit, and winit's logical unit for this window).
    On {
        at: LogicalPosition<f64>,
        size: LogicalSize<f64>,
        purpose: ImePurpose,
    },
}

/// The request for the kit's text target (`kit_input::text_target(&laid, &memory)`).
pub fn ime_request(target: &TextTarget) -> ImeRequest {
    match target {
        TextTarget::Unfocused => ImeRequest::Off,
        TextTarget::Field { caret, secret, .. } => ImeRequest::On {
            at: LogicalPosition::new(caret.x0, caret.y0),
            size: LogicalSize::new(1.0, caret.height()),
            purpose: match secret {
                true => ImePurpose::Password,
                false => ImePurpose::Normal,
            },
        },
    }
}

/// Remembers whether IME is allowed, so it is toggled only on a change (re-allowing it every frame resets a
/// composition on some platforms).
#[derive(Default, Debug)]
pub struct TextEntry {
    allowed: bool,
}

impl TextEntry {
    pub fn new() -> Self {
        Self::default()
    }
    /// Call once per frame after routing.
    pub fn sync(&mut self, window: &Window, target: &TextTarget) {
        match ime_request(target) {
            ImeRequest::Off => self.allow(window, false),
            ImeRequest::On { at, size, purpose } => {
                self.allow(window, true);
                window.set_ime_purpose(purpose);
                window.set_ime_cursor_area(at, size);
            }
        }
    }
    fn allow(&mut self, window: &Window, allowed: bool) {
        if self.allowed != allowed {
            window.set_ime_allowed(allowed);
            self.allowed = allowed;
        }
    }
}
