//! Golden pictures of the real pipeline. Re-bless deliberately with `KIT_BLESS=1 cargo test -p kit_raster` and look at
//! the PNGs before committing them.
use kit_raster::golden::{Tolerance, assert_matches};
use kit_raster::scene::Scene;

fn check(scene: Scene) {
    assert_matches(scene.name(), &scene.render(), Tolerance::ROUNDING);
}

#[test]
fn hud_bar() {
    check(Scene::HudBar);
}
#[test]
fn nested_layers() {
    check(Scene::NestedLayers);
}
#[test]
fn clipped_scroll() {
    check(Scene::ClippedScroll);
}
#[test]
fn drag_ghost() {
    check(Scene::DragGhost);
}

#[test]
fn images() {
    check(Scene::Images);
}
