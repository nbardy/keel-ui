//! Web text entry, the DOM half: the hidden `<textarea>`, its listeners, and the browser clipboard. What the listeners
//! report is diffed by [`crate::text::Composer`]; this file only reads events and moves the element.
use crate::text::{Composer, Entry, Keydown, Paste, Reported, is_paste_chord, key_for};
use kit_core::kurbo::Rect;
use kit_input::{Clipboard, ClipboardError, ClipboardRead, Modifiers, PasteTicket, TextTarget};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::HtmlTextAreaElement;

type Listener = Closure<dyn FnMut(web_sys::Event)>;

fn listen(
    area: &HtmlTextAreaElement,
    listeners: &mut Vec<Listener>,
    name: &str,
    handler: impl FnMut(web_sys::Event) + 'static,
) -> Result<(), JsValue> {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(handler);
    area.add_event_listener_with_callback(name, closure.as_ref().unchecked_ref())?;
    listeners.push(closure);
    Ok(())
}

/// The modifiers a `KeyboardEvent` says were held.
pub fn key_modifiers(event: &web_sys::KeyboardEvent) -> Modifiers {
    Modifiers {
        shift: event.shift_key(),
        ctrl: event.ctrl_key(),
        alt: event.alt_key(),
        meta: event.meta_key(),
    }
}

/// The modifiers a `MouseEvent` (or a `PointerEvent`, which extends it) says were held.
pub fn mouse_modifiers(event: &web_sys::MouseEvent) -> Modifiers {
    Modifiers {
        shift: event.shift_key(),
        ctrl: event.ctrl_key(),
        alt: event.alt_key(),
        meta: event.meta_key(),
    }
}

/// The hidden element and the listeners that turn its events into [`Entry`]s for `on_entry`. A host holds it for as long
/// as it draws: dropping it removes the element. Each frame it is told where text entry is ([`TextBridge::follow`]); the
/// `Entry`s it reports go through [`crate::text::entry_events`] with the host's own [`Paste`].
pub struct TextBridge {
    area: HtmlTextAreaElement,
    /// Kept alive as long as the element is.
    _listeners: Vec<Listener>,
}

impl TextBridge {
    /// Attach the element to the canvas's document body and start listening. Viewport positioning keeps it independent
    /// of the canvas's offset parent (including fixed canvases and canvases in positioned wrappers).
    pub fn attach(
        canvas: &web_sys::HtmlCanvasElement,
        on_entry: impl Fn(Entry) + 'static,
    ) -> Result<Self, JsValue> {
        let document = canvas.owner_document().ok_or("canvas has no document")?;
        let area = document
            .create_element("textarea")?
            .dyn_into::<HtmlTextAreaElement>()?;
        for (name, value) in [
            ("aria-hidden", "true"),
            ("tabindex", "-1"),
            ("autocomplete", "off"),
            ("autocapitalize", "off"),
            ("autocorrect", "off"),
        ] {
            area.set_attribute(name, value)?;
        }
        area.set_spellcheck(false);
        area.set_rows(1);
        let style = area.style();
        for (name, value) in [
            ("position", "fixed"),
            ("left", "0px"),
            ("top", "0px"),
            ("opacity", "0"),
            ("width", "1px"),
            ("height", "1px"),
            ("border", "none"),
            ("outline", "none"),
            ("padding", "0"),
            ("margin", "0"),
            ("resize", "none"),
            ("overflow", "hidden"),
            ("pointer-events", "none"),
            ("caret-color", "transparent"),
            // 16px or more stops mobile Safari zooming the page when the element is focused.
            ("font-size", "16px"),
        ] {
            style.set_property(name, value)?;
        }
        document
            .body()
            .ok_or("canvas document has no body to hold the text agent")?
            .append_child(&area)?;

        let composer = Rc::new(RefCell::new(Composer::default()));
        let on_entry = Rc::new(on_entry);
        let report = {
            let (area, on_entry) = (area.clone(), on_entry.clone());
            Rc::new(move |reported: Reported| {
                if reported.empty_element {
                    area.set_value("");
                }
                reported.entries.into_iter().for_each(&*on_entry);
            })
        };
        let mut listeners = Vec::new();

        listen(&area, &mut listeners, "keydown", {
            let (composer, on_entry) = (composer.clone(), on_entry.clone());
            move |event| {
                let Ok(event) = event.dyn_into::<web_sys::KeyboardEvent>() else {
                    return;
                };
                let handling = composer
                    .borrow_mut()
                    .keydown(event.is_composing(), event.key_code());
                let mods = key_modifiers(&event);
                let key = match handling {
                    Keydown::Consumed => None,
                    Keydown::Forward => key_for(&event.key(), mods),
                };
                if let Some(key) = key {
                    // Paste is the one default action wanted: the browser's `paste` event carries the text.
                    if !is_paste_chord(key, mods) {
                        event.prevent_default();
                    }
                    on_entry(Entry::Key { key, mods });
                }
            }
        })?;
        listen(&area, &mut listeners, "keyup", {
            let composer = composer.clone();
            move |_| composer.borrow_mut().keyup()
        })?;
        listen(&area, &mut listeners, "input", {
            let (composer, area, report) = (composer.clone(), area.clone(), report.clone());
            move |event| {
                let Ok(event) = event.dyn_into::<web_sys::InputEvent>() else {
                    return;
                };
                let reported = composer.borrow_mut().input(
                    &area.value(),
                    event.is_composing(),
                    &event.input_type(),
                );
                report(reported);
            }
        })?;
        listen(&area, &mut listeners, "compositionend", {
            let (composer, area, report) = (composer.clone(), area.clone(), report.clone());
            move |_| {
                let reported = composer.borrow_mut().composition_end(&area.value());
                report(reported);
            }
        })?;
        listen(&area, &mut listeners, "paste", {
            let on_entry = on_entry.clone();
            move |event| {
                let Ok(event) = event.dyn_into::<web_sys::ClipboardEvent>() else {
                    return;
                };
                event.prevent_default();
                let text = event
                    .clipboard_data()
                    .and_then(|data| data.get_data("text/plain").ok())
                    // A paste with no text data (an image) pastes nothing: an empty entry is dropped.
                    .unwrap_or_default();
                on_entry(Entry::Paste(text));
            }
        })?;
        Ok(Self {
            area,
            _listeners: listeners,
        })
    }

    /// Whether `target` is the hidden element (its key events are the bridge's, not the window handler's).
    pub fn owns(&self, target: Option<web_sys::EventTarget>) -> bool {
        let area: &web_sys::EventTarget = self.area.as_ref();
        target.is_some_and(|target| target == *area)
    }

    /// Put the element where text entry is: at the caret and focused while a field has focus, blurred (which hides the
    /// soft keyboard) when none does. `caret` is in the canvas's CSS px (the kit's logical px). Call once per frame.
    pub fn follow(
        &self,
        target: &TextTarget,
        canvas: &web_sys::HtmlCanvasElement,
    ) -> Result<(), JsValue> {
        match target {
            TextTarget::Unfocused => {
                if self.has_focus() {
                    self.area.blur()?;
                    self.area.set_value("");
                }
                // Stale desktop caret coordinates overflowed the phone lobby after blur + resize. Park even when
                // already blurred; no focused field owns a caret position then.
                self.area.style().set_property("left", "0px")?;
                self.area.style().set_property("top", "0px")?;
                Ok(())
            }
            TextTarget::Field {
                caret,
                mode,
                secret,
            } => {
                self.place(caret, canvas)?;
                self.area.set_attribute(
                    "enterkeyhint",
                    match mode {
                        kit_view::Mode::SingleLine => "done",
                        kit_view::Mode::MultiLine => "enter",
                    },
                )?;
                // Not `type=password`: that confuses password managers. The field masks its own bullets; this only
                // asks the keyboard for no suggestions.
                self.area
                    .set_attribute("autocorrect", if *secret { "off" } else { "on" })?;
                if !self.has_focus() {
                    let options = web_sys::FocusOptions::new();
                    options.set_prevent_scroll(true);
                    self.area.focus_with_options(&options)?;
                }
                Ok(())
            }
        }
    }

    /// Convert canvas CSS coordinates to viewport coordinates, then constrain the entire 1px element to the viewport.
    fn place(&self, caret: &Rect, canvas: &web_sys::HtmlCanvasElement) -> Result<(), JsValue> {
        let rect = canvas.get_bounding_client_rect();
        let x = rect.left() + caret.center().x.clamp(0.0, rect.width());
        let y = rect.top() + caret.center().y.clamp(0.0, rect.height());
        let style = self.area.style();
        // Canvas offsets use its own containing block, which need not be the textarea's. CSS clamp also bounds a
        // stale focused caret immediately during resize, before the host draws its next frame (viewport overflow bug).
        style.set_property("left", &format!("clamp(0px, {x}px, calc(100% - 1px))"))?;
        style.set_property("top", &format!("clamp(0px, {y}px, calc(100% - 1px))"))
    }

    fn has_focus(&self) -> bool {
        self.area
            .owner_document()
            .and_then(|document| document.active_element())
            .is_some_and(|active| active.is_same_node(Some(&self.area)))
    }
}

impl Drop for TextBridge {
    fn drop(&mut self) {
        self.area.remove();
    }
}

/// What `navigator.clipboard.writeText` returned. A browser that returns a promise settles it later, and a refusal is
/// logged then. One that returns something else has nothing to wait on and nothing to report, so the write is
/// `Synchronous`: a typed outcome, not an error the caller has to ignore.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    Settling,
    Synchronous,
}

/// `navigator.clipboard.writeText(text)`. Fire and forget: a refusal is logged when its promise settles, not retried.
/// An absent clipboard API, or one whose call throws, is an error.
///
/// `navigator.clipboard` is reached through `js_sys::Reflect`: web-sys gates its typed binding behind the
/// `web_sys_unstable_apis` cfg, which would need a RUSTFLAGS setting on every build of this crate.
pub fn write_text(text: &str) -> Result<WriteOutcome, ClipboardError> {
    let navigator = web_sys::window()
        .ok_or_else(|| ClipboardError::Failed("no window".into()))?
        .navigator();
    let clipboard = js_sys::Reflect::get(&navigator, &"clipboard".into())
        .ok()
        .filter(|clipboard| !clipboard.is_undefined())
        .ok_or(ClipboardError::Unsupported)?;
    let write = js_sys::Reflect::get(&clipboard, &"writeText".into())
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .ok_or(ClipboardError::Unsupported)?;
    let returned = write
        .call1(&clipboard, &text.into())
        .map_err(|error| ClipboardError::Failed(format!("{error:?}")))?;
    match returned.dyn_into::<js_sys::Promise>() {
        Ok(promise) => {
            wasm_bindgen_futures::spawn_local(async move {
                if let Err(error) = wasm_bindgen_futures::JsFuture::from(promise).await {
                    web_sys::console::warn_2(&"kit_web clipboard write refused:".into(), &error);
                }
            });
            Ok(WriteOutcome::Settling)
        }
        Err(_) => Ok(WriteOutcome::Synchronous),
    }
}

/// The browser clipboard, over the caller's [`Paste`] state. A write goes out through [`write_text`]. A read cannot be
/// answered from the browser at all: the text arrives in the textarea's `paste` event, so the read is answered from (or
/// waits on) the `Paste` that `entry_events` also drives. The caller owns that state and lends it for one frame.
pub struct WebClipboard<'a> {
    pub paste: &'a mut Paste,
}

impl Clipboard for WebClipboard<'_> {
    fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
        write_text(text).map(|_| ())
    }

    fn read(&mut self, ticket: PasteTicket) -> Result<ClipboardRead, ClipboardError> {
        Ok(self.paste.read(ticket))
    }
}
