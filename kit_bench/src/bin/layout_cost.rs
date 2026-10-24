//! `cargo run --release -p kit_bench --bin layout_cost`: layout cost per node for the game-HUD tree and for the
//! bench list (rows of label + `kit_widgets` checkbox + button), counted the same way and measured in one process.
//!
//! Trees are built outside the timer; one warm text system is reused, as a host does. Each workload runs in three
//! modes, alternating over several rounds so a change in machine load hits all of them alike:
//! * `rebuilt`: a fresh `TaffyLayout` every frame, so every node is created and solved (the cost of a frame whose tree
//!   changed shape, and what every frame cost before the layout kept its taffy tree);
//! * `steady`: one `TaffyLayout`, the same tree every frame;
//! * `one edit`: one `TaffyLayout`, one label's text changes every frame.
//! `MODE=steady` (and a workload name as the first argument) run just that, for profiling.
//! It prints node counts by kind, the median and minimum layout time, and time per node and per text leaf. Minimum is
//! the load-robust one; read both.
use std::time::{Duration, Instant};

use kit_core::kurbo::{Point, Size, Vec2};
use kit_core::peniko::{Brush, Color};
use kit_core::{Border, ColorVision, MotionPref, Shadow, Surface, Theme, Time, Viewport, WidgetId};
use kit_input::UiMemory;
use kit_layout::TaffyLayout;
use kit_state::{Act, Cx, Host, Ident, Intent, Life, Store};
use kit_view::testing::text_system;
use kit_view::*;
use kit_widgets::{Check, Checkbox, Icon, Ink, PathEl, Press, Ui, checkbox};

const COIN: Icon = Icon::new(
    &[PathEl::Move([4.0, 8.0]), PathEl::Line([12.0, 8.0])],
    Ink::Stroke(1.5),
);

struct App {
    now: Time,
}
impl Host for App {
    type Ask = ();
    fn now(&self) -> Time {
        self.now
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, _: ()) {}
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Row(usize);
impl Ident<App> for Row {
    fn life(&self, _: &App) -> Life {
        Life::Live
    }
}

type Msg = Intent<App>;

/// What the tree looks like this frame.
#[derive(Copy, Clone)]
enum Shown {
    Static,
    /// The first label reads differently every frame.
    Edited(u64),
}

impl Shown {
    fn suffix(self) -> String {
        match self {
            Shown::Static => String::new(),
            Shown::Edited(n) => format!(" #{n}"),
        }
    }
}

fn list_row(cx: &Cx<'_, App>, theme: &Theme, i: usize, shown: Shown) -> Node<Msg> {
    let ui = Ui::new(theme);
    let label = TextStyle::new(13.0, Color::from_rgba8(230, 230, 230, 255));
    let key = Row(i);
    let on = cx.state(
        &key,
        || false,
        |on: &mut bool, _: &Row, _: (), _: &App| {
            *on = !*on;
            Act::none()
        },
    );
    row([
        text(
            format!(
                "Item {i}{}",
                if i == 0 {
                    shown.suffix()
                } else {
                    String::new()
                }
            ),
            label,
        ),
        checkbox(
            cx,
            Checkbox {
                theme,
                key: key.clone(),
                label: "On",
                check: Check::Off,
                press: Press::Sends(on.send(())),
            },
        ),
        ui.labeled_button(COIN, "Buy").on_press(on.send(())).into(),
    ])
}

fn list(rows: usize) -> impl FnMut(Shown) -> Node<Msg> {
    let theme = Theme::new(ColorVision::Normal, MotionPref::Full);
    let store = Store::new();
    let memory = UiMemory::new(Viewport::new(Size::new(1280.0, 800.0), 1.0));
    let app = App {
        now: Time::default(),
    };
    move |shown| {
        let cx = Cx::routed(&app, &store, &memory);
        column((0..rows).map(|i| list_row(&cx, &theme, i, shown)))
    }
}

/// 4 corner clusters of slots, 60 world labels, one scroll list: `kit_layout/examples/hud_perf.rs`'s tree.
fn hud(shown: Shown) -> Node<Msg> {
    let ts = TextStyle::new(12.0, Color::WHITE);
    let white = || Brush::Solid(Color::WHITE);
    let block = |w, h| rect(Style::column().sized(w, h), white());
    let flex = |gap| {
        let mut s = Style::row()
            .gap(gap)
            .align_items(taffy::AlignItems::FLEX_START);
        s.layout.flex_wrap = taffy::FlexWrap::Wrap;
        s
    };
    let slot = |i: usize| {
        interactive(
            WidgetId::key("slot").index(i as u64),
            Bindings::none(),
            A11y::Hidden,
            panel(
                Style::column().sized(48.0, 56.0),
                Surface {
                    fill: white(),
                    radii: Default::default(),
                    border: Border::None,
                    shadow: Shadow::None,
                },
                vec![block(40.0, 40.0), text(format!("{i}"), ts.clone())],
            ),
        )
    };
    let cluster = |h, v, base: usize, n: usize| {
        stack(
            flex(4.0)
                .anchored(h, v, Vec2::new(8.0, 8.0))
                .sized(300.0, 120.0),
            (0..n).map(|i| slot(base + i)).collect(),
        )
    };
    let mut kids = vec![
        cluster(AnchorH::Start, AnchorV::Start, 0, 12),
        cluster(AnchorH::End, AnchorV::Start, 12, 12),
        cluster(AnchorH::Start, AnchorV::End, 24, 12),
        cluster(AnchorH::End, AnchorV::End, 36, 12),
    ];
    for i in 0..60 {
        kids.push(world(
            WorldAnchor {
                point: WorldPoint([20.0 + i as f64 * 9.0, 200.0 + (i % 7) as f64 * 20.0, 0.0]),
                offset: Vec2::new(0.0, -20.0),
            },
            column(vec![
                text(
                    format!(
                        "unit {i}{}",
                        if i == 0 {
                            shown.suffix()
                        } else {
                            String::new()
                        }
                    ),
                    ts.clone(),
                ),
                block(40.0, 4.0),
            ]),
        ));
    }
    let lines: Vec<Node<Msg>> = (0..20)
        .map(|i| text(format!("chat line {i} with some words"), ts.clone()))
        .collect();
    kids.push(scroll(
        WidgetId::key("chat"),
        kit_core::Axis::Vertical,
        Style::column()
            .sized(220.0, 150.0)
            .anchored(AnchorH::Center, AnchorV::End, Vec2::ZERO),
        column(lines),
    ));
    anchors(kids)
}

struct Plane;
impl WorldProjector for Plane {
    fn project(&self, point: WorldPoint) -> Projection {
        let [x, y, _] = point.0;
        Projection::OnScreen(Point::new(x, y))
    }
}

#[derive(Default)]
struct Counts {
    nodes: usize,
    text: usize,
    containers: usize,
    other_leaves: usize,
}

fn counts(laid: &LaidOut<Msg>) -> Counts {
    let mut c = Counts::default();
    for n in laid.root.descendants() {
        c.nodes += 1;
        match &n.kind {
            LaidKind::Text { .. } => c.text += 1,
            LaidKind::Stack { .. } | LaidKind::Panel { .. } => c.containers += 1,
            _ => c.other_leaves += 1,
        }
    }
    c
}

struct Workload {
    name: &'static str,
    build: Box<dyn FnMut(Shown) -> Node<Msg>>,
    /// Layout times per mode: rebuilt, steady, one edit.
    times: [Vec<Duration>; 3],
    counts: Counts,
}

fn median<T: Ord + Copy>(v: &[T]) -> T {
    let mut s = v.to_vec();
    s.sort();
    s[s.len() / 2]
}

const MODES: [&str; 3] = ["rebuilt", "steady", "one edit"];

fn main() {
    let vp = Viewport::new(Size::new(1280.0, 800.0), 1.0);
    let mut text = text_system();
    let mut work: Vec<Workload> = vec![
        ("hud", Box::new(hud) as Box<dyn FnMut(Shown) -> Node<Msg>>),
        ("list 100", Box::new(list(100))),
        ("list 1000", Box::new(list(1000))),
    ]
    .into_iter()
    .map(|(name, build)| Workload {
        name,
        build,
        times: Default::default(),
        counts: Counts::default(),
    })
    .collect();
    let only = std::env::args().nth(1);
    work.retain(|w| only.as_deref().is_none_or(|o| o == w.name));
    let rounds: usize = std::env::var("ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6);
    let only_mode = std::env::var("MODE").ok();
    let frames = 40;
    let mut tick = 0;
    for _ in 0..rounds {
        for w in &mut work {
            let mut kept = TaffyLayout::new();
            for mode in 0..MODES.len() {
                if only_mode.as_deref().is_some_and(|m| m != MODES[mode]) {
                    continue;
                }
                for f in 0..frames + 5 {
                    tick += 1;
                    let shown = [Shown::Static, Shown::Static, Shown::Edited(tick)][mode];
                    let tree = (w.build)(shown);
                    let mut fresh = TaffyLayout::new();
                    let layout = if mode == 0 { &mut fresh } else { &mut kept };
                    let t = Instant::now();
                    let laid = layout
                        .layout(tree, vp, &mut text, &Plane)
                        .expect("lays out");
                    let took = t.elapsed();
                    if f >= 5 {
                        w.times[mode].push(took);
                    }
                    w.counts = counts(&laid);
                }
            }
        }
    }
    println!(
        "| workload | mode | nodes | text | containers | other leaves | median us | min us | ns/node (median) | ns/node (min) | ns/text leaf (min) |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for w in &work {
        for (mode, times) in MODES.iter().zip(&w.times).filter(|(_, t)| !t.is_empty()) {
            let med = median(times).as_secs_f64() * 1e6;
            let min = times.iter().min().expect("frames ran").as_secs_f64() * 1e6;
            let c = &w.counts;
            println!(
                "| {} | {} | {} | {} | {} | {} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} |",
                w.name,
                mode,
                c.nodes,
                c.text,
                c.containers,
                c.other_leaves,
                med,
                min,
                med * 1e3 / c.nodes as f64,
                min * 1e3 / c.nodes as f64,
                min * 1e3 / c.text as f64
            );
        }
    }
}
