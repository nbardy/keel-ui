//! The kit's starter: your scene and the kit UI in one wgpu frame, on a winit window, with real input.
//!
//! ```sh
//! cargo run -p kit_winit --example starter                           # a window
//! cargo run -p kit_winit --example starter -- --snapshot out.png     # no window: scripted input, one PNG
//! ```
//!
//! The scene is a triangle this file draws with its own pipeline. The UI over it is real kit_widgets components:
//! a text field (IME and clipboard), a slider for the spin speed, a counter and the README's shop list. Input the UI
//! does not want (`Routing::Passed`) reaches the scene: drag empty space to spin the triangle, press C to recolour
//! it. Typing C in the text field does not recolour it: that key was the field's.
use kit_core::kurbo::{Point, Size};
use kit_core::{ColorVision, Identity, Key, MotionPref, Theme, Time, Viewport};
use kit_input::{Clipboard, InputEvent, KeyState, Phase, Router, Routing, TextTarget, UiMemory};
use kit_layout::TaffyLayout;
use kit_paint::paint;
use kit_state::{Act, Cx, Host, Ident, Intent, Life, Store, dispatch, wake_due};
use kit_view::kit_text::{TextConfig, inter};
use kit_view::taffy::AlignItems;
use kit_view::{Availability, CosmicText, LaidOut, NoWorld, Node, blocker, column, row};
use kit_wgpu::{FrameTarget, WgpuUi};
use kit_widgets::slider::Step;
use kit_widgets::{Icon, Ink, PathEl, Press, Slider, Submitted, Switch, TextField, Ui};
use kit_widgets::{slider, switch, text_field};
use kit_winit::{ArboardClipboard, Input, TextEntry};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

// ---------------------------------------------------------------------------------------------------------------
// Your app model. The kit never owns it: views read it, reducers ask it to do things (`Host::ask`).

pub struct Demo {
    now: Time,
    items: Vec<Item>, // as the server last listed them
    bought: Vec<&'static str>,
    pub callsign: String,
    pub spin: Spin,
}
struct Item {
    id: u32,
    name: &'static str,
}
/// The scene's state: the game's, not the UI's. Only `Passed` input and the slider's ask change it.
pub struct Spin {
    angle: f32,
    speed: f64, // radians per second, set by the slider
    pub colour: usize,
    drag: Drag,
}
enum Drag {
    Idle,
    From(f64), // logical x of the last pointer position
}
pub const PALETTE: [[u8; 3]; 3] = [[240, 96, 64], [80, 200, 120], [250, 210, 60]];

/// What components may ask the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    Buy(u32),
    Speed(f64),
    Callsign(String),
}
impl From<Submitted<Field>> for Ask {
    fn from(submitted: Submitted<Field>) -> Self {
        Ask::Callsign(submitted.text)
    }
}
impl Host for Demo {
    type Ask = Ask;
    fn now(&self) -> Time {
        self.now
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, ask: Ask) {
        match ask {
            Ask::Buy(id) => self
                .bought
                .extend(self.items.iter().filter(|i| i.id == id).map(|i| i.name)),
            Ask::Speed(speed) => self.spin.speed = speed,
            Ask::Callsign(text) => self.callsign = text,
        }
    }
}

impl Spin {
    pub fn angle(&self) -> f32 {
        self.angle
    }
    /// The scene's own input handler: it only ever sees events the UI passed on.
    fn input(&mut self, event: &InputEvent) {
        match (event, &self.drag) {
            (InputEvent::Pointer(p), _) if p.phase == Phase::Down => self.drag = Drag::From(p.at.x),
            (InputEvent::Pointer(p), Drag::From(x)) if p.phase == Phase::Move => {
                self.angle += ((p.at.x - x) * 0.01) as f32;
                self.drag = Drag::From(p.at.x);
            }
            (InputEvent::Pointer(p), _)
                if matches!(p.phase, Phase::Up | Phase::Cancel | Phase::Leave) =>
            {
                self.drag = Drag::Idle
            }
            (
                InputEvent::Key {
                    key: Key::Character('c'),
                    state: KeyState::Down,
                    ..
                },
                _,
            ) => self.colour = (self.colour + 1) % PALETTE.len(),
            _ => {} // everything else the UI passed is of no interest to this scene
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The UI: components are plain functions from app data to a value tree, re-run every frame.

/// The text field's key. It lives as long as the app.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Field;
impl Ident<Demo> for Field {
    fn life(&self, _: &Demo) -> Life {
        Life::Live
    }
}
/// The counter's key.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Counter;
impl Ident<Demo> for Counter {
    fn life(&self, _: &Demo) -> Life {
        Life::Live
    }
}
#[derive(Clone, Debug, PartialEq)]
struct Add;

/// The README's shop row: its key is the item's id, and it lives as long as the server lists the item.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Row(u32);
impl Ident<Demo> for Row {
    fn life(&self, demo: &Demo) -> Life {
        match demo.items.iter().any(|item| item.id == self.0) {
            true => Life::Live,
            false => Life::Over,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
enum RowEvent {
    Pin,
    Buy,
}

// Icons are authored on the kit's 32x32 canvas (`kit_widgets` `ICON_VIEW_BOX`) and scaled to the button's icon box.
const COIN: Icon = Icon::new(
    &[
        PathEl::Move([16.0, 6.0]),
        PathEl::Quad {
            control: [26.0, 6.0],
            to: [26.0, 16.0],
        },
        PathEl::Quad {
            control: [26.0, 26.0],
            to: [16.0, 26.0],
        },
        PathEl::Quad {
            control: [6.0, 26.0],
            to: [6.0, 16.0],
        },
        PathEl::Quad {
            control: [6.0, 6.0],
            to: [16.0, 6.0],
        },
        PathEl::Close,
    ],
    Ink::Stroke(3.0),
);
const PLUS: Icon = Icon::new(
    &[
        PathEl::Move([16.0, 6.0]),
        PathEl::Line([16.0, 26.0]),
        PathEl::Move([6.0, 16.0]),
        PathEl::Line([26.0, 16.0]),
    ],
    Ink::Stroke(3.0),
);

fn hud(cx: &Cx<'_, Demo>, theme: &Theme) -> Node<Intent<Demo>> {
    let (ui, demo) = (Ui::new(theme), cx.app());
    // Local state outside a loop: keyed by a type, not by call order.
    let clicks = cx.state(
        &Counter,
        || 0u32,
        |n: &mut u32, _: &Counter, _: Add, _: &Demo| {
            *n += 1;
            Act::none()
        },
    );
    let name = TextField {
        theme,
        key: Field,
        label: "Callsign",
        placeholder: "Type, paste or compose (IME)",
        availability: Availability::Enabled,
    };
    let speed = Slider {
        theme,
        key: "spin",
        label: "Spin speed",
        value: demo.spin.speed,
        range: 0.0..=6.0,
        step: Step::Continuous,
        on_change: |speed| Intent::Ask(Ask::Speed(speed)),
    };
    let panel = ui.panel(
        column([
            ui.caption("Keel kit starter").title().into(),
            text_field(cx, name),
            ui.caption(format!("Submitted (Enter): {}", demo.callsign))
                .into(),
            row([slider(speed)])
                .gap(8.0)
                .align_items(AlignItems::CENTER),
            ui.labeled_button(PLUS, format!("Clicked {}", *clicks))
                .on_press(clicks.send(Add))
                .into(),
            shop(cx, theme),
            ui.caption(format!("Bought: {}", demo.bought.join(", ")))
                .into(),
            ui.caption("Drag empty space to spin; C recolours")
                .muted()
                .into(),
        ])
        .gap(8.0),
    );
    // `blocker`: a press on the panel's background is the UI's (Captured), not the scene's.
    column([blocker(Identity::Positional, panel)])
        .pad(16.0)
        .align_items(AlignItems::START)
}

/// The README's shop list: a pin switch (local state in a loop, keyed by data) and a Buy button per item.
fn shop(cx: &Cx<'_, Demo>, theme: &Theme) -> Node<Intent<Demo>> {
    let ui = Ui::new(theme);
    column(cx.app().items.iter().map(|item| {
        let key = Row(item.id);
        let pinned = cx.state(
            &key,
            || false,
            |pinned: &mut bool, key: &Row, event: RowEvent, _: &Demo| match event {
                RowEvent::Pin => {
                    *pinned = !*pinned;
                    Act::none()
                }
                RowEvent::Buy => Act::ask(Ask::Buy(key.0)),
            },
        );
        row([
            switch(
                cx,
                Switch {
                    theme,
                    key: key.clone(),
                    label: item.name,
                    on: *pinned,
                    press: Press::Sends(pinned.send(RowEvent::Pin)),
                },
            ),
            ui.labeled_button(COIN, format!("Buy {}", item.name))
                .on_press(pinned.send(RowEvent::Buy))
                .into(),
        ])
        .gap(8.0)
    }))
    .gap(4.0)
}

// ---------------------------------------------------------------------------------------------------------------
// The scene: a triangle with its own tiny pipeline, drawn first into the frame's encoder.

const SCENE_WGSL: &str = r#"
struct Spin { angle: f32, aspect: f32, _pad: vec2<f32>, colour: vec4<f32> };
@group(0) @binding(0) var<uniform> spin: Spin;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let a = spin.angle + f32(i) * 2.0943951;
    // centred right of the UI panel, a third of the window tall
    return vec4<f32>(0.35 + 0.6 * cos(a) / spin.aspect, 0.6 * sin(a), 0.0, 1.0);
}
@fragment fn fs() -> @location(0) vec4<f32> { return spin.colour; }
"#;
pub const SCENE_CLEAR: [u8; 3] = [24, 14, 40];

struct Scene {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
}
impl Scene {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene"),
            source: wgpu::ShaderSource::Wgsl(SCENE_WGSL.into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene spin"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            uniform,
            bind,
        }
    }

    /// Clears the frame and draws the triangle: the pass every game already has.
    fn draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        size: (u32, u32),
        spin: &Spin,
    ) {
        let [r, g, b] = PALETTE[spin.colour].map(|c| f32::from(c) / 255.0);
        let aspect = size.0 as f32 / size.1 as f32;
        let floats = [spin.angle, aspect, 0.0, 0.0, r, g, b, 1.0];
        queue.write_buffer(
            &self.uniform,
            0,
            &floats
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect::<Vec<u8>>(),
        );
        let [r, g, b] = SCENE_CLEAR.map(|c| f64::from(c) / 255.0);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The frame: the README's "Drawing into your frame" and "The rest of the loop", step by step.

/// Everything that lives across frames.
pub struct Starter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    scene: Scene,
    kit_gpu: WgpuUi,    // one renderer per colour format you draw to
    text: CosmicText,   // shaping and the glyph cache (the bundled Inter font)
    store: Store<Demo>, // every component's local state, keyed by data
    memory: UiMemory,   // hover, focus, scroll, gestures
    layout: TaffyLayout,
    router: Router,
    theme: Theme,
    clipboard: ArboardClipboard,
    pasted: Vec<InputEvent>, // clipboard text, routed with the next frame's input
    pub app: Demo,
}

#[derive(Debug)]
pub enum FrameError {
    Layout(kit_view::LayoutError),
    Render(kit_paint::RenderError),
}

/// The frame the router and the IME need after drawing: what was laid out, and where text entry is.
pub struct Drawn {
    pub laid: LaidOut<Intent<Demo>>,
    pub text_target: TextTarget,
}

impl Starter {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        viewport: Viewport,
    ) -> Self {
        let text = CosmicText::new(inter::source(), TextConfig::default())
            .expect("the bundled Inter loads");
        let items = [(1, "Cannon"), (2, "Sail"), (3, "Hull")].map(|(id, name)| Item { id, name });
        Self {
            scene: Scene::new(&device, format),
            kit_gpu: WgpuUi::new(device.clone(), queue.clone(), format),
            device,
            queue,
            format,
            text,
            store: Store::new(),
            memory: UiMemory::new(viewport),
            layout: TaffyLayout::new(),
            router: Router::default(),
            theme: Theme::new(ColorVision::Normal, MotionPref::Full),
            clipboard: ArboardClipboard::new(),
            pasted: Vec::new(),
            app: Demo {
                now: Time::default(),
                items: items.into(),
                bought: Vec::new(),
                callsign: String::new(),
                spin: Spin {
                    angle: 0.0,
                    speed: 0.6,
                    colour: 0,
                    drag: Drag::Idle,
                },
            },
        }
    }

    /// One frame into `view` (any texture view of `format`, `size` physical px). `events` are this frame's input,
    /// already converted to the kit's (`kit_winit::Input::convert`). The caller submits nothing: this does, once.
    pub fn frame(
        &mut self,
        events: Vec<InputEvent>,
        viewport: Viewport,
        view: &wgpu::TextureView,
        size: (u32, u32),
        dt: Duration,
    ) -> Result<Drawn, FrameError> {
        // The clock is an input, never read from the OS by the kit: tests and replays are deterministic.
        self.app.now = Time(self.app.now.0 + dt);
        self.app.spin.angle += (self.app.spin.speed * dt.as_secs_f64()) as f32;

        // 1. State before the view: end the state of keys whose life is Over, fire timers reducers scheduled.
        self.store.collect(&self.app);
        wake_due(&mut self.app, &mut self.store);
        // 2. The view: a plain function of app data, local state and the router's memory (hover, focus).
        let root = hud(
            &Cx::routed(&self.app, &self.store, &self.memory),
            &self.theme,
        );

        // 3. Your scene, first, into YOUR encoder.
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.scene
            .draw(&self.queue, &mut encoder, view, size, &self.app.spin);

        // 4. Lay out at the frame's viewport (logical size + device scale, shared by layout and drawing).
        let mut ui = self.kit_gpu.begin_frame(&mut self.text, viewport);
        let laid = self
            .layout
            .layout(root, ui.viewport(), ui.text(), &NoWorld)
            .map_err(FrameError::Layout)?;

        // 5. Route this frame's input against what the player saw. Only what the UI passed reaches the scene.
        for event in self.pasted.drain(..).chain(events) {
            match self
                .router
                .route(&laid, &mut self.memory, event.clone(), self.app.now)
            {
                Routing::Handled(intents) => intents
                    .into_iter()
                    .for_each(|i| dispatch(&mut self.app, &mut self.store, i)),
                Routing::Captured => {} // the UI took it; the scene must not see it
                Routing::Passed => self.app.spin.input(&event), // nothing in the UI wanted it
            }
        }
        // 6. Copies and pastes the fields asked for: arboard answers at once; the text is routed next frame.
        let served = kit_input::service(
            self.memory.take_clipboard(),
            &mut self.clipboard as &mut dyn Clipboard,
        );
        served
            .failed
            .iter()
            .for_each(|(request, error)| eprintln!("clipboard {request:?}: {error:?}"));
        self.pasted.extend(served.events);

        // 7. The router's per-frame step (scroll momentum, focus requests), then the end of the input frame.
        self.router.frame(&laid, &mut self.memory, self.app.now);
        let text_target = kit_input::text_target(&laid, &self.memory);
        self.memory.end_frame();

        // 8. Paint to a plain-data DrawList and record it over the scene (LoadOp::Load) in the same encoder.
        let draws = paint(&laid, &self.memory, &self.theme, self.app.now);
        let target = FrameTarget {
            encoder: &mut encoder,
            view,
            format: self.format,
            size,
        };
        ui.record(&draws, target).map_err(FrameError::Render)?;

        // 9. One submit for scene and UI; dropping the UI frame after it is the glyph-atlas fence.
        self.queue.submit([encoder.finish()]);
        drop(ui);
        Ok(Drawn { laid, text_target })
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The window: winit events in, one frame per redraw.

enum App {
    Starting,
    Running(Box<Running>),
    Failed(String),
}
struct Running {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    input: Input, // window events -> kit events (scale, cursor and modifiers tracked)
    text_entry: TextEntry, // IME on while a field has focus, its candidate window at the caret
    events: Vec<InputEvent>,
    last: Instant,
    starter: Starter,
}

impl Running {
    fn open(events: &ActiveEventLoop) -> Result<Self, String> {
        let attributes = Window::default_attributes()
            .with_title("Keel kit starter")
            .with_inner_size(winit::dpi::LogicalSize::new(960.0, 600.0));
        let window = Arc::new(
            events
                .create_window(attributes)
                .map_err(|e| e.to_string())?,
        );
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))
            .map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("the surface is not supported by the adapter")?;
        // Scene and kit write display-encoded colour: draw through a non-sRGB view even on an sRGB swapchain.
        let format = config.format.remove_srgb_suffix();
        config.view_formats = vec![format];
        surface.configure(&device, &config);
        let input = Input::new(size, window.scale_factor());
        let starter = Starter::new(device, queue, format, input.viewport());
        Ok(Self {
            window,
            surface,
            config,
            input,
            text_entry: TextEntry::new(),
            events: Vec::new(),
            last: Instant::now(),
            starter,
        })
    }

    fn redraw(&mut self) -> Result<(), String> {
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.starter.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("surface validation error".into());
            }
        };
        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.config.view_formats[0]),
            ..Default::default()
        });
        let dt = self.last.elapsed();
        self.last = Instant::now();
        let events = std::mem::take(&mut self.events);
        let size = (self.config.width, self.config.height);
        let drawn = self
            .starter
            .frame(events, self.input.viewport(), &view, size, dt)
            .map_err(|e| format!("{e:?}"))?;
        self.window.pre_present_notify();
        self.starter.queue.present(texture);
        self.text_entry.sync(&self.window, &drawn.text_target);
        Ok(())
    }

    fn event(&mut self, event: &WindowEvent) -> Result<(), String> {
        self.events.extend(self.input.convert(event));
        match event {
            WindowEvent::Resized(size) => {
                self.config.width = size.width.max(1);
                self.config.height = size.height.max(1);
                self.surface.configure(&self.starter.device, &self.config);
                Ok(())
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => Ok(()), // input was converted above; the rest is not this host's business
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        if let App::Starting = self {
            *self = match Running::open(events) {
                Ok(running) => App::Running(Box::new(running)),
                Err(error) => {
                    events.exit();
                    App::Failed(error)
                }
            };
        }
    }
    fn window_event(&mut self, events: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let WindowEvent::CloseRequested = event {
            events.exit();
        } else if let App::Running(running) = self
            && let Err(error) = running.event(&event)
        {
            events.exit();
            *self = App::Failed(error);
        }
    }
    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let App::Running(running) = self {
            running.window.request_redraw(); // the scene animates: draw continuously (vsync paces it)
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Headless: no window. A scripted session through the same `Starter::frame`, read back as RGBA8.

pub const SNAPSHOT: (u32, u32) = (960, 600);

/// Renders a short scripted session offscreen and returns the final frame's pixels (RGBA8, row-major) and the app.
/// Script: type "Corvette" + Enter into the focused text field (its C is the field's: the triangle keeps its colour),
/// press the counter twice, pin "Sail", drag empty space (spins the triangle), press C over the scene (recolours it).
pub fn snapshot() -> Result<(Vec<u8>, Starter), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .map_err(|e| e.to_string())?;
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))
        .map_err(|e| e.to_string())?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let (width, height) = SNAPSHOT;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("snapshot"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let viewport = Viewport::new(Size::new(f64::from(width), f64::from(height)), 1.0);
    let mut starter = Starter::new(device.clone(), queue.clone(), format, viewport);
    let frame = |starter: &mut Starter, events: Vec<InputEvent>| {
        starter
            .frame(
                events,
                viewport,
                &view,
                (width, height),
                Duration::from_millis(16),
            )
            .map_err(|e| format!("{e:?}"))
    };
    let laid = frame(&mut starter, Vec::new())?.laid;
    let at = |role, label| {
        kit_view::testing::control(&laid, role, label)
            .map(|c| c.rect.center())
            .ok_or(format!("no {label}"))
    };
    let (field, counter, sail) = (
        at(kit_view::Role::TextBox, "Callsign")?,
        at(kit_view::Role::Button, "Clicked 0")?,
        at(kit_view::Role::CheckBox, "Sail")?,
    );
    let empty = Point::new(f64::from(width) * 0.8, f64::from(height) * 0.85);
    // Keys as winit reports them, through the adapter: a key press is its key, then its text.
    let press = |text: &str, key: winit::keyboard::Key| {
        let shift = text.chars().any(char::is_uppercase);
        let mods = kit_input::Modifiers {
            shift,
            ..Default::default()
        };
        let pressed = winit::event::ElementState::Pressed;
        kit_winit::key_events(&key, pressed, Some(text), mods)
    };
    let typed = |word: &str| -> Vec<InputEvent> {
        let keys = word.chars().map(|c| {
            press(
                &c.to_string(),
                winit::keyboard::Key::Character(c.to_string().into()),
            )
        });
        keys.flatten().collect()
    };
    let enter = press(
        "\r",
        winit::keyboard::Key::Named(winit::keyboard::NamedKey::Enter),
    );
    let script: Vec<Vec<InputEvent>> = vec![
        click(field),
        [typed("Corvette"), enter].concat(), // the field's: Key C is Captured, Text "C" Handled; the scene sees neither
        [click(counter), click(counter), click(sail)].concat(),
        click(empty), // blurs the field (Handled: the blur is a message)
        vec![
            mouse(Phase::Down, empty),
            mouse(Phase::Move, empty + (-150.0, 0.0)),
            mouse(Phase::Up, empty + (-150.0, 0.0)),
        ],
        typed("c"), // nothing focused: Passed to the scene, the triangle recolours
        Vec::new(),
    ];
    for events in script {
        frame(&mut starter, events)?;
    }
    Ok((
        read_back(&device, &queue, &texture, (width, height))?,
        starter,
    ))
}

fn mouse(phase: Phase, at: Point) -> InputEvent {
    InputEvent::Pointer(kit_input::PointerEvent {
        id: kit_winit::MOUSE,
        kind: kit_input::PointerKind::Mouse,
        phase,
        at,
        pressure: 1.0,
    })
}
fn click(at: Point) -> Vec<InputEvent> {
    vec![
        mouse(Phase::Move, at),
        mouse(Phase::Down, at),
        mouse(Phase::Up, at),
    ]
}

fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    (w, h): (u32, u32),
) -> Result<Vec<u8>, String> {
    let padded = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(padded * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(h),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    buffer.map_async(wgpu::MapMode::Read, .., |result| {
        result.expect("map readback")
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| e.to_string())?;
    let data = buffer.get_mapped_range(..).map_err(|e| format!("{e:?}"))?;
    Ok(data
        .chunks_exact(padded as usize)
        .flat_map(|row| row[..(w * 4) as usize].iter().copied())
        .collect())
}

fn write_png(path: &str, pixels: &[u8], (w, h): (u32, u32)) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(pixels).map_err(|e| e.to_string())
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag, path] if flag == "--snapshot" => {
            let (pixels, _) = snapshot()?;
            write_png(path, &pixels, SNAPSHOT)?;
            println!("wrote {path}");
            Ok(())
        }
        [] => {
            let events = EventLoop::new().map_err(|e| e.to_string())?;
            let mut app = App::Starting;
            events.run_app(&mut app).map_err(|e| e.to_string())?;
            match app {
                App::Failed(error) => Err(error),
                App::Starting | App::Running(_) => Ok(()),
            }
        }
        _ => Err("usage: starter [--snapshot <out.png>]".into()),
    }
}
