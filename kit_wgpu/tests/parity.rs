//! Cross-backend parity: every `kit_raster` fixture scene is drawn by the CPU renderer (tiny-skia) and by this GPU
//! renderer (Metal readback) from the *same* `DrawList`, then compared per pixel. A backend that drops, mis-orders or
//! mis-composites a `Draw` variant shows up here without anyone writing a per-variant test for it.
//!
//! What "the same picture" means (the differences come from anti-aliased curved edges and the phase of a long dashed circle):
//! * a pixel matches when every channel is within [`CHANNEL_TOLERANCE`] (rounding, shadow blur shape);
//! * at most [`MAX_DIFFERING_PER_MILLE`] of the pixels may not match (curved-edge coverage differs between a
//!   scanline rasteriser and a feathered triangle mesh, dash ends shift by up to a pixel);
//! * no pixel may differ by more than [`MAX_CHANNEL_DIFF`] on any channel (an AA edge never swaps a pixel to the
//!   opposite colour; a missing glyph box or caret does).
//!
//! On failure the CPU picture, the GPU picture and a diff (red = outside tolerance) are written to
//! `$TMPDIR/kit_parity/<scene>.{cpu,gpu,diff}.png` and the paths are in the panic message.
mod common;
use common::{Harness, Px};
use kit_paint::{Draw, DrawList};
use kit_raster::scene::{Fixture, Scene};
use kit_raster::tiny_skia::Pixmap;
use std::path::PathBuf;

const CHANNEL_TOLERANCE: u8 = 8;
/// Per mille of pixels (15 = 1.5%).
const MAX_DIFFERING_PER_MILLE: usize = 15;
const MAX_CHANNEL_DIFF: u8 = 128;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum Kind {
    Fill,
    Stroke,
    RoundRect,
    Glyphs,
    Caret,
    Layer,
    Image,
}

/// Exhaustive on purpose: a new `Draw` variant must be given a kind here, and then `ALL_KINDS` makes the coverage
/// tests demand a scene that uses it.
fn kind(draw: &Draw) -> Kind {
    match draw {
        Draw::Image { .. } => Kind::Image,
        Draw::Fill { .. } => Kind::Fill,
        Draw::Stroke { .. } => Kind::Stroke,
        Draw::RoundRect { .. } => Kind::RoundRect,
        Draw::Glyphs { .. } => Kind::Glyphs,
        Draw::Caret { .. } => Kind::Caret,
        Draw::PushLayer { .. } | Draw::PopLayer => Kind::Layer,
    }
}

const ALL_KINDS: [Kind; 7] = [
    Kind::Image,
    Kind::Fill,
    Kind::Stroke,
    Kind::RoundRect,
    Kind::Glyphs,
    Kind::Caret,
    Kind::Layer,
];

fn without(list: &DrawList, dropped: Kind) -> DrawList {
    DrawList {
        redraw: kit_core::Redraw::Idle,
        items: list
            .items
            .iter()
            .filter(|d| kind(d) != dropped)
            .cloned()
            .collect(),
    }
}

fn gpu_picture(fx: &mut Fixture, list: &DrawList) -> Vec<Px> {
    let (w, h) = fx.size;
    let mut gpu = Harness::new(w, h, 1.0);
    for pixels in &fx.pixels {
        gpu.ui.register_pixels(pixels).expect("fixture pixels");
    }
    let [r, g, b, a] = fx.backdrop.components.map(f64::from);
    gpu.frame(Some([r, g, b, a]), list, &mut fx.text)
        .expect("gpu renders the scene");
    gpu.read()
}

fn channel_diff(cpu: &Pixmap, gpu: &[Px]) -> impl Iterator<Item = u8> {
    cpu.pixels().iter().zip(gpu.to_vec()).map(|(c, g)| {
        [
            c.red().abs_diff(g[0]),
            c.green().abs_diff(g[1]),
            c.blue().abs_diff(g[2]),
            c.alpha().abs_diff(g[3]),
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
    })
}

struct Verdict {
    differing: usize,
    total: usize,
    worst: u8,
}

impl Verdict {
    fn of(cpu: &Pixmap, gpu: &[Px]) -> Self {
        let diffs: Vec<u8> = channel_diff(cpu, gpu).collect();
        Self {
            differing: diffs.iter().filter(|d| **d > CHANNEL_TOLERANCE).count(),
            total: diffs.len(),
            worst: diffs.into_iter().max().unwrap_or(0),
        }
    }
    fn within_tolerance(&self) -> bool {
        self.differing * 1000 <= self.total * MAX_DIFFERING_PER_MILLE
            && self.worst <= MAX_CHANNEL_DIFF
    }
    fn describe(&self) -> String {
        format!(
            "{} of {} pixels differ by more than {CHANNEL_TOLERANCE} (budget {}.{}%), worst channel difference {} (cap {MAX_CHANNEL_DIFF})",
            self.differing,
            self.total,
            MAX_DIFFERING_PER_MILLE / 10,
            MAX_DIFFERING_PER_MILLE % 10,
            self.worst
        )
    }
}

/// CPU picture, GPU picture and diff as PNGs; returns the directory.
fn write_pngs(scene: Scene, cpu: &Pixmap, gpu: &[Px]) -> PathBuf {
    let dir = std::env::temp_dir().join("kit_parity");
    std::fs::create_dir_all(&dir).expect("create parity output dir");
    let mut gpu_png = Pixmap::new(cpu.width(), cpu.height()).expect("size");
    let mut diff_png = gpu_png.clone();
    for (i, d) in channel_diff(cpu, gpu).enumerate() {
        let g = gpu[i];
        gpu_png.data_mut()[i * 4..i * 4 + 4].copy_from_slice(&g);
        let c = cpu.data()[i * 4..i * 4 + 4].to_vec();
        let out = &mut diff_png.data_mut()[i * 4..i * 4 + 4];
        match d > CHANNEL_TOLERANCE {
            true => out.copy_from_slice(&[255, 0, 0, 255]),
            false => out.copy_from_slice(&[c[0] / 3, c[1] / 3, c[2] / 3, 255]),
        }
    }
    for (name, png) in [("cpu", cpu), ("gpu", &gpu_png), ("diff", &diff_png)] {
        png.save_png(dir.join(format!("{}.{name}.png", scene.name())))
            .expect("write parity png");
    }
    dir
}

#[test]
fn every_scene_renders_the_same_picture_on_both_backends() {
    let mut failures = Vec::new();
    for scene in Scene::ALL {
        let mut fx = scene.fixture();
        let cpu = fx.render();
        let list = fx.list.clone();
        let gpu = gpu_picture(&mut fx, &list);
        let verdict = Verdict::of(&cpu, &gpu);
        if !verdict.within_tolerance() {
            let dir = write_pngs(scene, &cpu, &gpu);
            failures.push(format!(
                "{}: {} (pngs in {})",
                scene.name(),
                verdict.describe(),
                dir.display()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "backends disagree:\n{}",
        failures.join("\n")
    );
}

/// Without this the parity test could pass while a whole class of draws is never compared.
#[test]
fn the_scenes_exercise_every_draw_kind() {
    let used: Vec<Kind> = Scene::ALL
        .into_iter()
        .flat_map(|s| s.fixture().list.items.iter().map(kind).collect::<Vec<_>>())
        .collect();
    let missing: Vec<Kind> = ALL_KINDS
        .into_iter()
        .filter(|k| !used.contains(k))
        .collect();
    assert!(missing.is_empty(), "no scene draws {missing:?}");
}

/// The tolerance is only worth anything if it is tighter than a missing feature: remove each draw kind from the GPU's
/// list and the comparison must fail. This is what "fails if either backend drops a Draw variant" means in practice.
#[test]
fn a_backend_that_drops_a_draw_kind_fails_parity() {
    let mut escaped = Vec::new();
    for scene in Scene::ALL {
        let mut fx = scene.fixture();
        let cpu = fx.render();
        for dropped in ALL_KINDS {
            if !fx.list.items.iter().any(|d| kind(d) == dropped) {
                continue;
            }
            let list = without(&fx.list, dropped);
            let gpu = gpu_picture(&mut fx, &list);
            let verdict = Verdict::of(&cpu, &gpu);
            if verdict.within_tolerance() {
                escaped.push(format!(
                    "{} without {dropped:?}: {}",
                    scene.name(),
                    verdict.describe()
                ));
            }
        }
    }
    assert!(
        escaped.is_empty(),
        "parity would not notice:\n{}",
        escaped.join("\n")
    );
}
