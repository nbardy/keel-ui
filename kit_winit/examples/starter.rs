//! The kit's starter: your scene and the kit UI in one wgpu frame, on a winit window, with real input.
//!
//! ```sh
//! cargo run -p kit_winit --example starter                           # a window
//! cargo run -p kit_winit --example starter -- --snapshot out.png     # no window: scripted input, one PNG
//! ```
//!
//! The app (model, view, scene, one frame) is `examples_shared/starter_app.rs`, shared with the browser starter in
//! `kit_web`. This file is the winit host: window events in, one frame per redraw, the OS clipboard and IME.
use kit_core::Viewport;
use kit_core::kurbo::{Point, Size};
use kit_input::{InputEvent, Phase, Platform};
use kit_winit::{ArboardClipboard, Input, TextEntry};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

#[path = "../../examples_shared/starter_app.rs"]
mod app;
pub use app::{PALETTE, SCENE_CLEAR, Starter};

// ---------------------------------------------------------------------------------------------------------------
// The window: winit events in, one frame per redraw.

enum Host {
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
    clipboard: ArboardClipboard,
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
        let starter = Starter::new(device, queue, format, input.viewport(), Platform::native());
        Ok(Self {
            window,
            surface,
            config,
            input,
            text_entry: TextEntry::new(),
            events: Vec::new(),
            last: Instant::now(),
            starter,
            clipboard: ArboardClipboard::new(),
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
            .frame(
                events,
                self.input.viewport(),
                &view,
                size,
                dt,
                &mut self.clipboard,
            )
            .map_err(|e| e.to_string())?;
        for (request, error) in &drawn.clipboard_failures {
            eprintln!("clipboard {request:?}: {error:?}");
        }
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

impl ApplicationHandler for Host {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        if let Host::Starting = self {
            *self = match Running::open(events) {
                Ok(running) => Host::Running(Box::new(running)),
                Err(error) => {
                    events.exit();
                    Host::Failed(error)
                }
            };
        }
    }
    fn window_event(&mut self, events: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let WindowEvent::CloseRequested = event {
            events.exit();
        } else if let Host::Running(running) = self
            && let Err(error) = running.event(&event)
        {
            events.exit();
            *self = Host::Failed(error);
        }
    }
    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Host::Running(running) = self {
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
    let mut starter = Starter::new(
        device.clone(),
        queue.clone(),
        format,
        viewport,
        Platform::native(),
    );
    let mut clipboard = ArboardClipboard::new();
    let mut frame = |starter: &mut Starter, events: Vec<InputEvent>| {
        starter
            .frame(
                events,
                viewport,
                &view,
                (width, height),
                Duration::from_millis(16),
                &mut clipboard,
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
        button: kit_input::PointerButton::Primary,
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
    let data = buffer.get_mapped_range(..).map_err(|e| e.to_string())?;
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
            let mut host = Host::Starting;
            events.run_app(&mut host).map_err(|e| e.to_string())?;
            match host {
                Host::Failed(error) => Err(error),
                Host::Starting | Host::Running(_) => Ok(()),
            }
        }
        _ => Err("usage: starter [--snapshot <out.png>]".into()),
    }
}
