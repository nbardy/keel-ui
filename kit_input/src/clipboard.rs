//! The clipboard seam. The router never touches the OS: a copy or a paste shortcut in a focused field leaves a
//! [`ClipboardRequest`] in `UiMemory`'s outbox. The host drains it after routing, performs it through its
//! [`Clipboard`], and feeds any pasted text back as `InputEvent::Paste`, because a browser's clipboard read is
//! asynchronous: the text arrives some time after the key was pressed, and the field may have changed by then.
use crate::InputEvent;
use kit_core::WidgetId;
use kit_view::kit_text::Rev;

/// A paste that was asked for: which field, and the revision of its text at that moment. The answer carries it back,
/// and the field refuses the paste if its text has moved on (`Applied::Stale`).
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct PasteTicket {
    pub field: WidgetId,
    pub rev: Rev,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum ClipboardRequest {
    Write(String),
    Read(PasteTicket),
}

/// What a read gave.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum ClipboardRead {
    Ready(String),
    /// The text will arrive later: the host sends `InputEvent::Paste` with the ticket when it does.
    Later,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum ClipboardError {
    /// This host has no clipboard.
    Unsupported,
    /// The platform refused or failed (permission denied, clipboard busy, not text).
    Failed(String),
}

/// The host's clipboard. Plain text only.
pub trait Clipboard {
    fn write(&mut self, text: &str) -> Result<(), ClipboardError>;
    fn read(&mut self, ticket: PasteTicket) -> Result<ClipboardRead, ClipboardError>;
}

/// What came of a batch of requests.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Serviced {
    /// Pasted text that was ready now: route these like any other event.
    pub events: Vec<InputEvent>,
    /// Requests the clipboard could not do. Nothing is retried; the host decides whether to tell the player.
    pub failed: Vec<(ClipboardRequest, ClipboardError)>,
}

/// Perform `requests` (from `UiMemory::take_clipboard`) on `clipboard`.
pub fn service(requests: Vec<ClipboardRequest>, clipboard: &mut dyn Clipboard) -> Serviced {
    let mut done = Serviced::default();
    for request in requests {
        let result = match &request {
            ClipboardRequest::Write(text) => clipboard.write(text).map(|()| None),
            ClipboardRequest::Read(ticket) => clipboard.read(*ticket).map(|read| match read {
                ClipboardRead::Ready(text) => Some(InputEvent::Paste {
                    ticket: *ticket,
                    text,
                }),
                ClipboardRead::Later => None,
            }),
        };
        match result {
            Ok(Some(event)) => done.events.push(event),
            Ok(None) => {}
            Err(error) => done.failed.push((request, error)),
        }
    }
    done
}
