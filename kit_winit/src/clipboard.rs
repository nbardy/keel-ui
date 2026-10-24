//! The OS clipboard through `arboard` (text only). Reads and writes are immediate, so a paste is answered in the
//! same `kit_input::service` call that asked for it. Adapted from the Keel game's native host.
use kit_input::{Clipboard, ClipboardError, ClipboardRead, PasteTicket};

pub struct ArboardClipboard {
    /// The OS may refuse a handle (no display, locked pasteboard); that is kept and reported on every use.
    pasteboard: Result<arboard::Clipboard, String>,
}

impl ArboardClipboard {
    pub fn new() -> Self {
        Self {
            pasteboard: arboard::Clipboard::new().map_err(|error| error.to_string()),
        }
    }
    fn pasteboard(&mut self) -> Result<&mut arboard::Clipboard, ClipboardError> {
        self.pasteboard
            .as_mut()
            .map_err(|error| ClipboardError::Failed(error.clone()))
    }
}

impl Default for ArboardClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard for ArboardClipboard {
    fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.pasteboard()?
            .set_text(text)
            .map_err(|error| ClipboardError::Failed(error.to_string()))
    }
    fn read(&mut self, _ticket: PasteTicket) -> Result<ClipboardRead, ClipboardError> {
        self.pasteboard()?
            .get_text()
            .map(ClipboardRead::Ready)
            .map_err(|error| ClipboardError::Failed(error.to_string()))
    }
}
