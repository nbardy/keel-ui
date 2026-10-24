//! The canvas host loop: a WebGPU surface on a `<canvas>`, DOM input queued for the next frame, and a frame loop that
//! asks the browser for an animation frame only while the kit says something changes.
//!
//! One [`Core`] owns everything. DOM callbacks only push to its inbox and ask for a frame; the single place that borrows
//! the host state mutably is [`Core::frame`], so a callback fired from inside a frame (a synchronous `focus()` event)
//! cannot hit a borrow that is already taken.
use crate::convert::{self, DomPointer};
use crate::schedule::{Wake, wake};
use crate::text::{Entry, Paste, entry_events};
use crate::textarea::{TextBridge, WebClipboard, key_modifiers, mouse_modifiers};
use kit_core::kurbo::Point;
use kit_core::{Redraw, Time, Viewport};
use kit_input::{
    Clipboard, ClipboardError, ClipboardRequest, InputEvent, KeyState, PasteTicket, Phase,
    Platform, TextTarget,
};
use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, PointerEvent};

/// The device the canvas draws with, for the app to make its own resources (pipelines, a kit renderer).
#[derive(Clone)]
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// The canvas's colour format. Never an sRGB one (see [`start`]).
    pub format: wgpu::TextureFormat,
}

/// One frame's inputs.
pub struct FrameInput<'a> {
    /// The DOM input since the last frame, already converted, oldest first. A resize comes first.
    pub events: Vec<InputEvent>,
    pub viewport: Viewport,
    /// The canvas's texture for this frame, `size` physical px.
    pub view: &'a wgpu::TextureView,
    pub size: (u32, u32),
    /// Real time since the previous frame. After an idle gap this is long: the app's clock keeps real time.
    pub dt: Duration,
    /// The browser clipboard: writes go out now, a read is answered later by the textarea's `paste` event.
    pub clipboard: &'a mut dyn Clipboard,
}

/// What the host needs after a frame.
pub struct FrameOutput {
    /// Where text entry is, for the hidden textarea (the IME and soft keyboard follow it).
    pub text_target: TextTarget,
    /// A paste that starts now, for text the browser pastes unasked into the focused field.
    pub paste_ticket: Option<PasteTicket>,
    /// When to draw again without input. `Redraw::At` is on the app's clock: the sum of the `dt`s it was given, from
    /// `Time::default()`.
    pub redraw: Redraw,
    /// Clipboard requests the browser refused; the host logs them to the console.
    pub clipboard_failures: Vec<(ClipboardRequest, ClipboardError)>,
}

/// What the host draws: one `frame` per draw. The app records into the encoder it makes, submits once, and answers with
/// the next [`Redraw`].
pub trait App: 'static {
    fn frame(&mut self, input: FrameInput<'_>) -> Result<FrameOutput, String>;
}

/// A running host. Dropping it stops the loop and removes its listeners and the hidden textarea.
pub struct Running<A: App>(Rc<Core<A>>);

impl<A: App> Running<A> {
    /// Frames drawn so far. An idle page's count stops rising.
    pub fn frames(&self) -> u64 {
        self.0.frames.get()
    }

    /// Read the app (to expose a probe to the page; never from inside `App::frame`).
    pub fn with_app<R>(&self, read: impl FnOnce(&A) -> R) -> R {
        read(&self.0.host.borrow().app)
    }
}

/// Routes `panic!` messages to `console.error`, which is where a wasm panic is otherwise invisible (the default hook
/// prints to stderr, which a browser does not have).
pub fn log_panics() {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&info.to_string().into());
    }));
}

/// The canvas's CSS size and device pixel ratio, as measured at the start of a frame.
#[derive(Copy, Clone, PartialEq, Debug)]
struct Extent {
    css: (f64, f64),
    dpr: f64,
}

fn measure(canvas: &HtmlCanvasElement) -> Result<Extent, String> {
    let rect = canvas.get_bounding_client_rect();
    let dpr = web_sys::window().ok_or("no window")?.device_pixel_ratio();
    Ok(Extent {
        css: (rect.width(), rect.height()),
        dpr,
    })
}

enum Timer {
    Unarmed,
    Armed(i32),
}

struct Host<A> {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
    app: A,
    clipboard: WebClipboard,
    extent: Extent,
    last_ms: f64,
    clock: Time,
}

impl<A> Host<A> {
    /// Resizing a WebGPU canvas invalidates its texture: only when the size really changed. `configure` also sets the
    /// canvas element's `width`/`height` (its drawing buffer) to the configuration's, so nothing else sizes the buffer.
    fn resize(&mut self, extent: Extent) {
        let (width, height) = convert::backing_size(extent.css, extent.dpr);
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.gpu.device, &self.config);
        self.extent = extent;
    }
}

struct Core<A> {
    canvas: HtmlCanvasElement,
    inbox: RefCell<Vec<InputEvent>>,
    /// An animation frame has been requested and has not run yet.
    pending: Cell<bool>,
    timer: Cell<Timer>,
    /// Where the current paste is (shortcut, text, the router's ticket); shared with the clipboard.
    paste: Rc<RefCell<Paste>>,
    /// The focused field's ticket as of the last frame, for pastes nobody asked for.
    unasked: Cell<Option<PasteTicket>>,
    failed: Cell<bool>,
    frames: Cell<u64>,
    host: RefCell<Host<A>>,
    frame_cb: Closure<dyn FnMut(f64)>,
    timer_cb: Closure<dyn FnMut()>,
    /// Set once, right after construction: the textarea's callbacks hold a `Weak<Core>`, which exists only then.
    bridge: OnceCell<TextBridge>,
    listeners: RefCell<Listeners>,
    /// Set once by `listen`, like the bridge: its callback needs the finished `Weak<Core>`.
    resize_observer: OnceCell<ResizeWatch>,
}

/// The canvas's ResizeObserver and the callback it needs kept alive.
type ResizeWatch = (web_sys::ResizeObserver, Closure<dyn FnMut(js_sys::Array)>);

/// A DOM callback with the host it belongs to.
type Act<A> = Box<dyn Fn(&Core<A>, web_sys::Event)>;

type Handler = Closure<dyn FnMut(web_sys::Event)>;

/// DOM listeners, removed when dropped.
#[derive(Default)]
struct Listeners(Vec<(web_sys::EventTarget, &'static str, Handler)>);

impl Listeners {
    fn add(
        &mut self,
        target: &web_sys::EventTarget,
        name: &'static str,
        handler: impl FnMut(web_sys::Event) + 'static,
    ) -> Result<(), JsValue> {
        let closure = Handler::new(handler);
        target.add_event_listener_with_callback(name, closure.as_ref().unchecked_ref())?;
        self.0.push((target.clone(), name, closure));
        Ok(())
    }
}

impl Drop for Listeners {
    fn drop(&mut self) {
        for (target, name, closure) in &self.0 {
            // A listener that cannot be removed is already gone with its page.
            let _ =
                target.remove_event_listener_with_callback(name, closure.as_ref().unchecked_ref());
        }
    }
}

impl<A> Core<A> {
    fn clear_timer(&self) {
        if let Timer::Armed(handle) = self.timer.replace(Timer::Unarmed)
            && let Some(window) = web_sys::window()
        {
            window.clear_timeout_with_handle(handle);
        }
    }
}

impl<A: App> Core<A> {
    fn push(&self, events: impl IntoIterator<Item = InputEvent>) {
        self.inbox.borrow_mut().extend(events);
        self.request_frame();
    }

    fn request_frame(&self) {
        if self.failed.get() || self.pending.replace(true) {
            return;
        }
        let requested = web_sys::window()
            .ok_or_else(|| JsValue::from_str("no window"))
            .and_then(|window| {
                window.request_animation_frame(self.frame_cb.as_ref().unchecked_ref())
            });
        if let Err(error) = requested {
            self.fail(&format!("requestAnimationFrame: {error:?}"));
        }
    }

    /// A frame error is final: the page shows nothing newer than the last good frame, so say why, loudly.
    fn fail(&self, message: &str) {
        self.failed.set(true);
        web_sys::console::error_1(&format!("kit_web: {message}").into());
        if let Some(body) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.body())
        {
            // The page can show it (`body[data-kit-web-error]`) and a test can read it.
            let _ = body.set_attribute("data-kit-web-error", message);
        }
    }

    fn schedule(&self, next: Wake) {
        self.clear_timer();
        match next {
            Wake::Frame => self.request_frame(),
            Wake::Sleep => {}
            Wake::After(wait) => {
                let armed = web_sys::window()
                    .ok_or_else(|| JsValue::from_str("no window"))
                    .and_then(|window| {
                        window.set_timeout_with_callback_and_timeout_and_arguments_0(
                            self.timer_cb.as_ref().unchecked_ref(),
                            // Rounded up: firing a millisecond early would find the deadline not yet reached.
                            wait.as_millis().saturating_add(1).min(i32::MAX as u128) as i32,
                        )
                    });
                match armed {
                    Ok(handle) => self.timer.set(Timer::Armed(handle)),
                    Err(error) => self.fail(&format!("setTimeout: {error:?}")),
                }
            }
        }
    }

    fn run_frame(&self, now_ms: f64) {
        self.pending.set(false);
        if self.failed.get() {
            return;
        }
        match self.frame(now_ms) {
            Ok(next) => self.schedule(next),
            Err(message) => self.fail(&message),
        }
    }

    fn frame(&self, now_ms: f64) -> Result<Wake, String> {
        let mut host = self.host.borrow_mut();
        let host = &mut *host;
        let mut events = Vec::new();
        let extent = measure(&self.canvas)?;
        if extent != host.extent {
            host.resize(extent);
            events.push(InputEvent::Resize(convert::viewport(
                extent.css, extent.dpr,
            )));
        }
        let texture = match host.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            // Not drawable this time: configure again and retry on the next animation frame. The input stays queued.
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Outdated
            | wgpu::CurrentSurfaceTexture::Lost => {
                host.surface.configure(&host.gpu.device, &host.config);
                return Ok(Wake::Frame);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("surface validation error".into());
            }
        };
        events.append(&mut self.inbox.borrow_mut());
        let dt = Duration::from_secs_f64((now_ms - host.last_ms).max(0.0) / 1000.0);
        host.last_ms = now_ms;
        host.clock = Time(host.clock.0 + dt);
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let out = host.app.frame(FrameInput {
            events,
            viewport: convert::viewport(extent.css, extent.dpr),
            view: &view,
            size: (host.config.width, host.config.height),
            dt,
            clipboard: &mut host.clipboard,
        })?;
        host.gpu.queue.present(texture);
        self.frames.set(self.frames.get() + 1);
        self.unasked.set(out.paste_ticket);
        for (request, error) in &out.clipboard_failures {
            web_sys::console::warn_1(&format!("kit_web clipboard {request:?}: {error:?}").into());
        }
        if let Some(bridge) = self.bridge.get()
            && let Err(error) = bridge.follow(&out.text_target, &self.canvas)
        {
            web_sys::console::warn_2(&"kit_web text agent:".into(), &error);
        }
        Ok(wake(out.redraw, host.clock))
    }

    fn entry(&self, entry: Entry) {
        let events = entry_events(entry, &mut self.paste.borrow_mut(), self.unasked.get());
        self.push(events);
    }
}

impl<A> Drop for Core<A> {
    fn drop(&mut self) {
        self.clear_timer();
        if let Some((observer, _)) = self.resize_observer.get() {
            observer.disconnect();
        }
    }
}

/// A position relative to the canvas, in CSS px.
fn canvas_point(canvas: &HtmlCanvasElement, event: &web_sys::MouseEvent) -> Point {
    let rect = canvas.get_bounding_client_rect();
    Point::new(
        f64::from(event.client_x()) - rect.left(),
        f64::from(event.client_y()) - rect.top(),
    )
}

fn platform() -> Result<Platform, JsValue> {
    let navigator = web_sys::window().ok_or("no window")?.navigator();
    let text = |value: Result<JsValue, JsValue>| {
        value
            .ok()
            .and_then(|value| value.as_string())
            .unwrap_or_default()
    };
    // `userAgentData` is not in every browser: absent, it names nothing, which `platform_of` treats as no witness.
    let data_platform = text(
        js_sys::Reflect::get(&navigator, &"userAgentData".into())
            .and_then(|data| js_sys::Reflect::get(&data, &"platform".into())),
    );
    let platform = navigator.platform().unwrap_or_default();
    let agent = navigator.user_agent().unwrap_or_default();
    Ok(convert::platform_of(&[&data_platform, &platform, &agent]))
}

/// A WebGPU surface on `canvas`, a device for it, and the surface configured at the canvas's current size.
async fn gpu(
    canvas: &HtmlCanvasElement,
    extent: Extent,
) -> Result<(wgpu::Surface<'static>, wgpu::SurfaceConfiguration, Gpu), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .map_err(|e| format!("create WebGPU surface: {e}"))?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        })
        .await
        .map_err(|e| format!("no WebGPU adapter: {e}"))?;
    let (device, queue) = adapter
        .request_device(&Default::default())
        .await
        .map_err(|e| format!("WebGPU device: {e}"))?;
    let (width, height) = convert::backing_size(extent.css, extent.dpr);
    let config = surface
        .get_default_config(&adapter, width, height)
        .ok_or("the canvas is not supported by the adapter")?;
    // The kit and most scenes write display-encoded colour. A browser canvas offers non-sRGB formats (bgra8unorm,
    // rgba8unorm), so drawing straight into one is right; an sRGB canvas would brighten everything, and silently, so it
    // is an error here. (`kit_winit` draws through a non-sRGB view of an sRGB swapchain; WebGPU has no such need.)
    if config.format.is_srgb() {
        return Err(format!("the canvas format {:?} is sRGB", config.format));
    }
    surface.configure(&device, &config);
    let format = config.format;
    Ok((
        surface,
        config,
        Gpu {
            device,
            queue,
            format,
        },
    ))
}

/// Start the host on `canvas`. `build` makes the app from the device, the canvas's viewport and the browser's platform
/// (Cmd or Ctrl in text editing), and the first frame is requested. Size the canvas with CSS (it is measured every
/// frame); the host sets its drawing-buffer size to the CSS size times the device pixel ratio.
pub async fn start<A: App>(
    canvas: HtmlCanvasElement,
    build: impl FnOnce(&Gpu, Viewport, Platform) -> A,
) -> Result<Running<A>, JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let extent = measure(&canvas)?;
    let (surface, config, gpu) = gpu(&canvas, extent).await?;
    let app = build(&gpu, convert::viewport(extent.css, extent.dpr), platform()?);
    let paste = Rc::new(RefCell::new(Paste::default()));
    let mut host = Host {
        surface,
        config,
        gpu,
        app,
        clipboard: WebClipboard {
            paste: paste.clone(),
        },
        extent,
        last_ms: window.performance().map_or(0.0, |p| p.now()),
        clock: Time::default(),
    };
    host.resize(extent);
    let core = Rc::new_cyclic(|weak: &Weak<Core<A>>| Core {
        canvas: canvas.clone(),
        inbox: RefCell::new(Vec::new()),
        pending: Cell::new(false),
        timer: Cell::new(Timer::Unarmed),
        paste,
        unasked: Cell::new(None),
        failed: Cell::new(false),
        frames: Cell::new(0),
        host: RefCell::new(host),
        frame_cb: Closure::new({
            let weak = weak.clone();
            move |now_ms| {
                if let Some(core) = weak.upgrade() {
                    core.run_frame(now_ms);
                }
            }
        }),
        timer_cb: Closure::new({
            let weak = weak.clone();
            move || {
                if let Some(core) = weak.upgrade() {
                    core.timer.set(Timer::Unarmed);
                    core.request_frame();
                }
            }
        }),
        bridge: OnceCell::new(),
        listeners: RefCell::new(Listeners::default()),
        resize_observer: OnceCell::new(),
    });
    let bridge = TextBridge::attach(&canvas, {
        let weak = Rc::downgrade(&core);
        move |entry| {
            if let Some(core) = weak.upgrade() {
                core.entry(entry);
            }
        }
    })?;
    core.bridge
        .set(bridge)
        .map_err(|_| "the text agent was attached twice")?;
    listen(&core, &window, &canvas)?;
    core.request_frame();
    Ok(Running(core))
}

/// The DOM listeners: pointer, wheel, keys, focus and size. Each converts with `crate::convert` and pushes.
fn listen<A: App>(
    core: &Rc<Core<A>>,
    window: &web_sys::Window,
    canvas: &HtmlCanvasElement,
) -> Result<(), JsValue> {
    let mut listeners = core.listeners.borrow_mut();
    let style = canvas.style();
    // The canvas takes every touch gesture: the page neither scrolls nor zooms under it.
    style.set_property("touch-action", "none")?;
    let weak = Rc::downgrade(core);
    let on_core = |act: Act<A>| {
        let weak = weak.clone();
        move |event: web_sys::Event| {
            if let Some(core) = weak.upgrade() {
                act(&core, event);
            }
        }
    };

    for (name, phase) in [
        ("pointerdown", Phase::Down),
        ("pointermove", Phase::Move),
        ("pointerup", Phase::Up),
        ("pointercancel", Phase::Cancel),
        ("pointerleave", Phase::Leave),
    ] {
        listeners.add(
            canvas,
            name,
            on_core(Box::new(move |core, event| {
                let Ok(event) = event.dyn_into::<PointerEvent>() else {
                    return;
                };
                let at = canvas_point(&core.canvas, &event);
                let dom = DomPointer {
                    pointer_type: &event.pointer_type(),
                    id: event.pointer_id(),
                    button: event.button(),
                    at,
                    pressure: event.pressure(),
                    mods: mouse_modifiers(&event),
                };
                // Capture keeps a drag going when the pointer leaves the canvas, and delivers its release.
                // Failing to capture (the pointer is already gone) only loses that continuation.
                match phase {
                    Phase::Down => {
                        let _ = core.canvas.set_pointer_capture(event.pointer_id());
                    }
                    Phase::Up | Phase::Cancel => {
                        let _ = core.canvas.release_pointer_capture(event.pointer_id());
                    }
                    Phase::Move | Phase::Leave => {}
                }
                core.push(convert::pointer(phase, &dom));
            })),
        )?;
    }
    listeners.add(
        canvas,
        "wheel",
        on_core(Box::new(|core, event| {
            let Ok(event) = event.dyn_into::<web_sys::WheelEvent>() else {
                return;
            };
            // The page never scrolls: the wheel is the kit's scroll areas' or nobody's.
            event.prevent_default();
            let at = canvas_point(&core.canvas, &event);
            core.push(convert::wheel(
                event.delta_x(),
                event.delta_y(),
                event.delta_mode(),
                at,
            ));
        })),
    )?;
    // The secondary button is the kit's context gesture, not the browser's menu.
    listeners.add(
        canvas,
        "contextmenu",
        on_core(Box::new(|_, event| event.prevent_default())),
    )?;
    for (name, state) in [("keydown", KeyState::Down), ("keyup", KeyState::Up)] {
        listeners.add(
            window,
            name,
            on_core(Box::new(move |core, event| {
                let Ok(event) = event.dyn_into::<web_sys::KeyboardEvent>() else {
                    return;
                };
                // The hidden textarea has its own listeners; its keys bubble to the window and must not count twice.
                let from_textarea = core.bridge.get().is_some_and(|b| b.owns(event.target()));
                if from_textarea || event.is_composing() {
                    return;
                }
                let mods = key_modifiers(&event);
                let key = convert::key_event(&event.key(), state, mods);
                // The kit decides what a key does at the next frame, so a callback cannot ask. Unmodified keys the kit
                // names (Tab, Space, arrows, Backspace) are taken: the page never scrolls or tabs away. Chords stay the
                // browser's (reload, find, devtools).
                if key.is_some() && !(mods.ctrl || mods.meta) {
                    event.prevent_default();
                }
                core.push(key);
            })),
        )?;
    }
    for (name, focused) in [("focus", true), ("blur", false)] {
        listeners.add(
            window,
            name,
            on_core(Box::new(move |core, _| {
                core.push([InputEvent::Focus(focused)])
            })),
        )?;
    }
    // Zooming changes the device pixel ratio without the canvas's CSS size changing: both reach `frame`, which measures.
    listeners.add(
        window,
        "resize",
        on_core(Box::new(|core, _| core.request_frame())),
    )?;
    let observer = {
        let weak = Rc::downgrade(core);
        let callback = Closure::<dyn FnMut(js_sys::Array)>::new(move |_| {
            if let Some(core) = weak.upgrade() {
                core.request_frame();
            }
        });
        let observer = web_sys::ResizeObserver::new(callback.as_ref().unchecked_ref())?;
        observer.observe(canvas);
        (observer, callback)
    };
    core.resize_observer
        .set(observer)
        .map_err(|_| "the resize observer was attached twice")?;
    Ok(())
}
