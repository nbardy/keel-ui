//! `cargo run --release -p kit_bench --bin frame_bench`: whole-frame timings at 100, 1,000 and 10,000 rows.
//!
//! Each row is a text label, a `kit_widgets` checkbox and a `kit_widgets` button, with its own keyed `cx.state`
//! (the README shop pattern). The kit loop is the host loop from `kit_widgets/tests/components/support.rs`, with
//! `WgpuUi::begin_frame` / `UiFrame::record` from `kit_wgpu`. Per frame the phases are timed separately: view build,
//! layout, paint and GPU record (record, submit and a waiting `device.poll`, so GPU time is included). The egui side
//! is CPU only: `Context::run_ui` plus `tessellate`, over the same rows (label, checkbox, button).
use std::hint::black_box;
use std::time::{Duration, Instant};

use egui::{Pos2, RawInput, Rect, vec2};
use kit_core::kurbo::Size;
use kit_core::peniko::Color;
use kit_core::{ColorVision, MotionPref, Theme, Time, Viewport};
use kit_input::{Router, UiMemory};
use kit_layout::TaffyLayout;
use kit_paint::paint;
use kit_state::{Act, Cx, Host, Ident, Intent, Life, Store, wake_due};
use kit_view::testing::text_system;
use kit_view::{CosmicText, NoWorld, Node, TextStyle, column, row, text};
use kit_wgpu::{FrameTarget, WgpuUi};
use kit_widgets::{Check, Checkbox, Icon, Ink, PathEl, Press, Ui, checkbox};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 800;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const ROW_COUNTS: [usize; 3] = [100, 1_000, 10_000];
const WARMUP: usize = 20;
const MEASURED: usize = 200;
/// Above this median frame time, the measured run is shortened to `SLOW_MEASURED` frames.
const SLOW_FRAME: Duration = Duration::from_millis(100);
const SLOW_MEASURED: usize = 50;

const COIN: Icon = Icon::new(
    &[PathEl::Move([4.0, 8.0]), PathEl::Line([12.0, 8.0])],
    Ink::Stroke(1.5),
);

/// The app model: how many rows the server lists, and what the UI asked for.
struct Bench {
    rows: usize,
    buys: u64,
    now: Time,
}

impl Host for Bench {
    type Ask = Buy;
    fn now(&self) -> Time {
        self.now
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, _: Buy) {
        self.buys += 1;
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Buy(usize);

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Row(usize);

impl Ident<Bench> for Row {
    fn life(&self, app: &Bench) -> Life {
        match self.0 < app.rows {
            true => Life::Live,
            false => Life::Over,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum RowEvent {
    Toggle,
    Buy,
}

/// The view: a function from app data to a value tree. It runs every frame.
fn view(cx: &Cx<'_, Bench>, theme: &Theme) -> Node<Intent<Bench>> {
    let ui = Ui::new(theme);
    let label = TextStyle::new(13.0, Color::from_rgba8(230, 230, 230, 255));
    column((0..cx.app().rows).map(|i| {
        let key = Row(i);
        let on = cx.state(
            &key,
            || false,
            |on: &mut bool, key: &Row, event: RowEvent, _: &Bench| match event {
                RowEvent::Toggle => {
                    *on = !*on;
                    Act::none()
                }
                RowEvent::Buy => Act::ask(Buy(key.0)),
            },
        );
        let check = match *on {
            true => Check::On,
            false => Check::Off,
        };
        row([
            text(format!("Item {i}"), label.clone()),
            checkbox(
                cx,
                Checkbox {
                    theme,
                    key: key.clone(),
                    label: "On",
                    check,
                    press: Press::Sends(on.send(RowEvent::Toggle)),
                },
            ),
            ui.labeled_button(COIN, "Buy")
                .on_press(on.send(RowEvent::Buy))
                .into(),
        ])
    }))
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target: wgpu::TextureView,
    adapter: String,
}

/// A headless device (no window) and an offscreen RGBA8 target the UI draws into.
fn gpu() -> Gpu {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&Default::default())).expect("GPU adapter");
    let adapter_name = adapter.get_info().name;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&Default::default())).expect("device");
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bench target"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target = texture.create_view(&Default::default());
    Gpu {
        device,
        queue,
        target,
        adapter: adapter_name,
    }
}

/// The kit's per-host state, built once and reused for every frame of one run.
struct Kit {
    store: Store<Bench>,
    memory: UiMemory,
    layout: TaffyLayout,
    router: Router,
    text: CosmicText,
    theme: Theme,
    viewport: Viewport,
    ui_gpu: WgpuUi,
    gpu: Gpu,
}

impl Kit {
    fn new(gpu: Gpu) -> Self {
        let viewport = Viewport::new(Size::new(WIDTH as f64, HEIGHT as f64), 1.0);
        Self {
            store: Store::new(),
            memory: UiMemory::new(viewport),
            layout: TaffyLayout::new(),
            router: Router::default(),
            text: text_system(),
            theme: Theme::new(ColorVision::Normal, MotionPref::Full),
            viewport,
            ui_gpu: WgpuUi::new(gpu.device.clone(), gpu.queue.clone(), FORMAT),
            gpu,
        }
    }

    /// One host frame. Returns the time of each phase: build, layout, paint, GPU record.
    fn frame(&mut self, app: &mut Bench) -> [Duration; 4] {
        let t = Instant::now();
        self.store.collect(app);
        wake_due(app, &mut self.store);
        let root = view(&Cx::routed(app, &self.store, &self.memory), &self.theme);
        let build = t.elapsed();

        let mut ui = self.ui_gpu.begin_frame(&mut self.text, self.viewport);
        let t = Instant::now();
        let laid = self
            .layout
            .layout(root, ui.viewport(), ui.text(), &NoWorld)
            .expect("the bench tree lays out");
        let layout = t.elapsed();

        let t = Instant::now();
        let list = paint(&laid, &self.memory, &self.theme, app.now());
        let paint_time = t.elapsed();

        let t = Instant::now();
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let recorded = ui.record(
            &list,
            FrameTarget {
                encoder: &mut encoder,
                view: &self.gpu.target,
                format: FORMAT,
                size: (WIDTH, HEIGHT),
            },
        );
        ui.submitted(self.gpu.queue.submit([encoder.finish()]));
        recorded.expect("the bench list records");
        self.gpu
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU poll");
        let gpu_time = t.elapsed();

        self.router.frame(&laid, &mut self.memory, app.now());
        self.memory.end_frame();
        [build, layout, paint_time, gpu_time]
    }
}

/// Per-frame durations of one configuration.
struct Series {
    frames: usize,
    /// Build, layout, paint, GPU record; or a single run + tessellate series for egui.
    phases: Vec<Vec<Duration>>,
}

fn kit_series(gpu: Gpu, rows: usize) -> Series {
    let mut kit = Kit::new(gpu);
    let mut app = Bench {
        rows,
        buys: 0,
        now: Time::default(),
    };
    let mut warm: Vec<Duration> = (0..WARMUP)
        .map(|_| kit.frame(&mut app).iter().sum())
        .collect();
    warm.sort();
    let frames = match warm[warm.len() / 2] > SLOW_FRAME {
        true => SLOW_MEASURED,
        false => MEASURED,
    };
    let mut phases: Vec<Vec<Duration>> = (0..4).map(|_| Vec::with_capacity(frames)).collect();
    for _ in 0..frames {
        for (phase, time) in phases.iter_mut().zip(kit.frame(&mut app)) {
            phase.push(time);
        }
    }
    black_box(app.buys);
    Series { frames, phases }
}

/// egui 0.36.2, CPU only: the same rows (label, checkbox, button), `run_ui` plus `tessellate`. No GPU side.
fn egui_series(rows: usize, frames: usize) -> Series {
    let ctx = egui::Context::default();
    let mut checks = vec![false; rows];
    let input = || RawInput {
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            vec2(WIDTH as f32, HEIGHT as f32),
        )),
        ..Default::default()
    };
    let mut times = Vec::with_capacity(frames);
    for i in 0..WARMUP + frames {
        let t = Instant::now();
        let output = ctx.run_ui(input(), |ui| {
            for (n, on) in checks.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("Item {n}"));
                    ui.checkbox(on, "On");
                    let _ = ui.button("Buy");
                });
            }
        });
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        black_box(&primitives);
        let elapsed = t.elapsed();
        if i >= WARMUP {
            times.push(elapsed);
        }
    }
    Series {
        frames,
        phases: vec![times],
    }
}

/// Nearest-rank percentile, in milliseconds.
fn percentile(values: &[Duration], p: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort();
    let rank = (p * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1].as_secs_f64() * 1e3
}

fn sum_per_frame(phases: &[Vec<Duration>]) -> Vec<Duration> {
    (0..phases[0].len())
        .map(|frame| phases.iter().map(|phase| phase[frame]).sum())
        .collect()
}

fn row_line(rows: usize, phase: &str, frames: usize, values: &[Duration]) -> String {
    format!(
        "| {rows} | {phase} | {frames} | {:.3} | {:.3} |",
        percentile(values, 0.5),
        percentile(values, 0.95)
    )
}

fn main() {
    let gpu_probe = gpu();
    eprintln!("GPU adapter: {}", gpu_probe.adapter);
    let mut gpu_slot = Some(gpu_probe);

    println!("| Rows | Phase | Frames | Median (ms) | p95 (ms) |");
    println!("|---:|---|---:|---:|---:|");
    for rows in ROW_COUNTS {
        let gpu_for_run = gpu_slot.take().unwrap_or_else(gpu);
        let kit = kit_series(gpu_for_run, rows);
        let names = ["view build", "layout", "paint", "GPU record"];
        for (name, values) in names.iter().zip(&kit.phases) {
            println!(
                "{}",
                row_line(rows, &format!("kit {name}"), kit.frames, values)
            );
        }
        let kit_cpu: Vec<Duration> = sum_per_frame(&kit.phases[..3]);
        println!(
            "{}",
            row_line(rows, "kit build + layout + paint", kit.frames, &kit_cpu)
        );
        let kit_total = sum_per_frame(&kit.phases);
        println!(
            "{}",
            row_line(rows, "kit total (incl. GPU record)", kit.frames, &kit_total)
        );

        let egui = egui_series(rows, kit.frames);
        println!(
            "{}",
            row_line(
                rows,
                "egui run + tessellate (CPU only)",
                egui.frames,
                &egui.phases[0]
            )
        );
    }
}
