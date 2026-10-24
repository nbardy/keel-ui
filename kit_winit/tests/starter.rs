//! The starter example, end to end on a real GPU without a window: its scripted session (`starter::snapshot`) is
//! read back and checked in the regions where the scene and the UI must each be visible.
//!
//! Mutants that fail it (2026-10-24): skipping `ui.record` (no UI over the panel region); routing every event to
//! the scene instead of only `Passed` ones (the field's C recolours the triangle a second time); dropping `Passed`
//! events (no spin, no recolour).
#[allow(dead_code)] // `main` and the window half of the example are not used here
#[path = "../examples/starter.rs"]
mod starter;

/// The pixel at (x, y) of an RGBA8 frame `SNAPSHOT.0` wide, without alpha.
fn rgb(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let at = ((y * starter::SNAPSHOT.0 + x) * 4) as usize;
    [pixels[at], pixels[at + 1], pixels[at + 2]]
}

#[test]
fn ui_is_drawn_over_the_scene_and_only_passed_input_reaches_the_scene() {
    let (pixels, starter) =
        starter::snapshot().expect("a GPU adapter renders the starter offscreen");
    let (w, h) = starter::SNAPSHOT;

    // The scene: the triangle's centroid (NDC x 0.35) is in the colour after exactly one recolour. The field's
    // "Corvette" has a C too: had the scene seen it, this would be the third colour.
    let centroid = rgb(&pixels, (w as f32 * 0.675) as u32, h / 2);
    assert_eq!(
        centroid,
        starter::PALETTE[1],
        "triangle colour after one Passed C"
    );
    assert_eq!(
        rgb(&pixels, w - 10, h - 10),
        starter::SCENE_CLEAR,
        "the scene's own clear"
    );
    // Time alone turns it by under 0.1 rad in the session; the 150 px drag turns it by 1.5.
    let angle = starter.app.spin.angle();
    assert!(
        angle.abs() > 1.0,
        "a drag on empty space spun the triangle (angle {angle})"
    );

    // The UI: the panel at the top left covers the scene's clear colour almost everywhere inside it.
    let panel = (30..290).flat_map(|x| (30..380).map(move |y| (x, y)));
    let total = panel.clone().count();
    let cleared = panel
        .filter(|&(x, y)| rgb(&pixels, x, y) == starter::SCENE_CLEAR)
        .count();
    assert!(
        cleared * 50 < total,
        "{cleared} of {total} panel pixels show the bare scene"
    );

    // The field took the keys and Enter submitted them.
    assert_eq!(starter.app.callsign, "Corvette");
}
