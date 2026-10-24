//! The starter's app: the model, the view, the scene and one frame, shared by the native (`kit_winit`) and browser
//! (`kit_web`) starters. Each example includes this file with `#[path]` and adds only its host: how events arrive, how
//! the surface is made, when the next frame runs. Nothing here names winit or the DOM.
//!
//! The scene is a triangle this file draws with its own pipeline. The UI over it is real kit_widgets components:
//! a text field (IME and clipboard), a slider for the spin speed, a counter and the README's shop list. Input the UI
//! does not want (`Routing::Passed`) reaches the scene: drag empty space to spin the triangle, press C to recolour
//! it. Typing C in the text field does not recolour it: that key was the field's.
use kit_core::{ColorVision, Identity, Key, MotionPref, Redraw, Theme, Time, Viewport};
use kit_input::{
    Clipboard, ClipboardError, ClipboardRequest, GestureConfig, InputEvent, KeyState, PasteTicket,
    Phase, Platform, Router, Routing, TextTarget, UiMemory,
};
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
use std::time::Duration;

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
    pub speed: f64, // radians per second, set by the slider
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
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    scene: Scene,
    kit_gpu: WgpuUi,    // one renderer per colour format you draw to
    text: CosmicText,   // shaping and the glyph cache (the bundled Inter font)
    store: Store<Demo>, // every component's local state, keyed by data
    memory: UiMemory,   // hover, focus, scroll, gestures
    layout: TaffyLayout,
    router: Router,
    theme: Theme,
    pasted: Vec<InputEvent>, // clipboard text, routed with the next frame's input
    pub app: Demo,
}

#[derive(Debug)]
pub enum FrameError {
    Layout(kit_view::LayoutError),
    Render(kit_paint::RenderError),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Layout(error) => write!(f, "layout: {error:?}"),
            FrameError::Render(error) => write!(f, "render: {error:?}"),
        }
    }
}

/// What the host needs after a frame: what was laid out, where text entry is, when to draw next, and what the
/// clipboard could not do.
pub struct Drawn {
    pub laid: LaidOut<Intent<Demo>>,
    pub text_target: TextTarget,
    /// A paste that starts now, for text the platform delivers unasked (a touch menu's Paste); `None`: no field
    /// has focus.
    pub paste_ticket: Option<PasteTicket>,
    /// The earliest the host must draw again without input: the kit's demand and the scene's own.
    pub redraw: Redraw,
    pub clipboard_failures: Vec<(ClipboardRequest, ClipboardError)>,
}

impl Starter {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        viewport: Viewport,
        platform: Platform,
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
            router: Router {
                config: GestureConfig {
                    platform,
                    ..GestureConfig::default()
                },
            },
            theme: Theme::new(ColorVision::Normal, MotionPref::Full),
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
    /// already converted to the kit's (`kit_winit::Input::convert`, `kit_web::convert`). The caller submits nothing:
    /// this does, once. `clipboard` is the host's: copies are written and pastes read through it.
    pub fn frame(
        &mut self,
        events: Vec<InputEvent>,
        viewport: Viewport,
        view: &wgpu::TextureView,
        size: (u32, u32),
        dt: Duration,
        clipboard: &mut dyn Clipboard,
    ) -> Result<Drawn, FrameError> {
        // The clock is an input, never read from the OS by the kit: tests and replays are deterministic.
        self.app.now = Time(self.app.now.0 + dt);
        self.app.spin.angle += (self.app.spin.speed * dt.as_secs_f64()) as f32;

        // 1. State before the view: end the state of keys whose life is Over, fire timers reducers scheduled.
        self.store.collect(&self.app);
        wake_due(&mut self.app, &mut self.store);
        // 2. The view: a plain function of app data, local state and the router's memory (hover, focus).
        let cx = Cx::routed(&self.app, &self.store, &self.memory);
        let root = hud(&cx, &self.theme);
        let view_redraw = cx.redraw();

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
        // 6. Copies and pastes the fields asked for. A clipboard that answers at once (arboard) gives its text here,
        //    routed next frame; one that answers later (the browser's) sends `InputEvent::Paste` itself.
        let served = kit_input::service(self.memory.take_clipboard(), clipboard);
        self.pasted.extend(served.events);

        // 7. The router's per-frame step (scroll momentum, focus requests), then the end of the input frame.
        self.router.frame(&laid, &mut self.memory, self.app.now);
        let text_target = kit_input::text_target(&laid, &self.memory);
        let paste_ticket = kit_input::paste_ticket(&laid, &self.memory);
        self.memory.end_frame();
        // After `end_frame`: a drag that moved this frame asks for one follow-up view there.
        let input_redraw = self.memory.redraw(&laid);

        // 8. Paint to a plain-data DrawList and record it over the scene (LoadOp::Load) in the same encoder.
        let draws = paint(&laid, &self.memory, &self.theme, self.app.now);
        // The kit's demand, plus the scene's: a spinning triangle is an animation the kit knows nothing of. At speed 0
        // nothing changes until input, and an event-driven host stops drawing.
        let scene_redraw = match self.app.spin.speed > 0.0 {
            true => Redraw::Now,
            false => Redraw::Idle,
        };
        let redraw = draws
            .frame_redraw(view_redraw, self.store.redraw(&self.app), input_redraw)
            .earliest(scene_redraw);
        let target = FrameTarget {
            encoder: &mut encoder,
            view,
            format: self.format,
            size,
        };
        let recorded = ui.record(&draws, target);

        // 9. One submit for scene and UI; acknowledge it even after a recording error.
        ui.submitted(self.queue.submit([encoder.finish()]));
        recorded.map_err(FrameError::Render)?;
        Ok(Drawn {
            laid,
            text_target,
            paste_ticket,
            redraw,
            clipboard_failures: served.failed,
        })
    }
}
