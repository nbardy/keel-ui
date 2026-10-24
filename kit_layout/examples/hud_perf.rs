//! `cargo run --release -p kit_layout --example hud_perf`: layout time for a ~300-node HUD tree.
#[path = "../tests/common/mod.rs"]
mod common;

use common::*;
use kit_core::kurbo::{Size, Vec2};
use kit_core::peniko::Color;
use kit_core::{Border, Shadow, Surface, Viewport, WidgetId};
use kit_layout::TaffyLayout;
use kit_view::*;
use std::time::Instant;

/// 4 corner clusters (flex rows of slots with captions), 60 world labels, one scroll list.
fn hud() -> Node<()> {
    let ts = TextStyle::new(12.0, Color::from_rgba8(255, 255, 255, 255));
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
    let cluster = |h, v, n: usize| {
        stack(
            flex(taffy::FlexDirection::Row, taffy::FlexWrap::Wrap, 4.0)
                .anchored(h, v, Vec2::new(8.0, 8.0))
                .sized(300.0, 120.0),
            (0..n).map(slot).collect(),
        )
    };
    let mut kids = vec![
        cluster(AnchorH::Start, AnchorV::Start, 12),
        cluster(AnchorH::End, AnchorV::Start, 12),
        cluster(AnchorH::Start, AnchorV::End, 12),
        cluster(AnchorH::End, AnchorV::End, 12),
    ];
    for i in 0..60 {
        kids.push(world(
            WorldAnchor {
                point: WorldPoint([20.0 + i as f64 * 9.0, 200.0 + (i % 7) as f64 * 20.0, 0.0]),
                offset: Vec2::new(0.0, -20.0),
            },
            column(vec![
                text(format!("unit {i}"), ts.clone()),
                block(40.0, 4.0),
            ]),
        ));
    }
    let list: Vec<Node<()>> = (0..20)
        .map(|i| text(format!("chat line {i} with some words"), ts.clone()))
        .collect();
    kids.push(scroll(
        WidgetId::key("chat"),
        kit_core::Axis::Vertical,
        Style::column()
            .sized(220.0, 150.0)
            .anchored(AnchorH::Center, AnchorV::End, Vec2::ZERO),
        column(list),
    ));
    anchors(kids)
}

fn bench(name: &str, mut layout: impl FnMut(Node<()>)) {
    let n = hud().count();
    for _ in 0..50 {
        layout(hud());
    }
    let frames = 2000;
    let mut total = std::time::Duration::ZERO;
    for _ in 0..frames {
        let tree = hud();
        let t = Instant::now();
        layout(tree);
        total += t.elapsed();
    }
    println!(
        "{name}: {n} nodes, {:.1} us/frame (layout only, tree build excluded)",
        total.as_secs_f64() * 1e6 / frames as f64
    );
}

fn main() {
    let vp = Viewport::new(Size::new(844.0, 390.0), 3.0);
    let mut taffy = TaffyLayout::new();
    bench("taffy ", |t| {
        taffy.layout(t, vp, &mut text_system(), &Plane).unwrap();
    });
}
