use kit_core::{
    Viewport,
    kurbo::{Rect, Size},
};
use kit_layout::TaffyLayout;
use kit_view::{
    Fit, ImagePixels, ImageRegion, ImageSize, ImageStyle, NoWorld, column, image,
    testing::text_system,
};

#[test]
fn contain_centres_and_keeps_aspect_for_both_axes_with_tolerance() {
    let size = ImageSize::new(640, 360).unwrap();
    for (bounds, expected) in [
        (
            Rect::new(10.0, 20.0, 110.0, 120.0),
            Rect::new(10.0, 41.875, 110.0, 98.125),
        ),
        (
            Rect::new(7.0, 11.0, 307.0, 61.0),
            Rect::new(112.55555556, 11.0, 201.44444444, 61.0),
        ),
    ] {
        let got = Fit::Contain.rect(size.logical(), bounds);
        for (actual, want) in [got.x0, got.y0, got.x1, got.y1].into_iter().zip([
            expected.x0,
            expected.y0,
            expected.x1,
            expected.y1,
        ]) {
            assert!((actual - want).abs() < 1e-6, "{got:?} != {expected:?}");
        }
        assert!((got.width() / got.height() - 16.0 / 9.0).abs() < 1e-6);
    }
}

#[test]
fn image_intrinsic_size_and_explicit_taffy_box() {
    let pixels = ImagePixels::new(40, 20, vec![255; 40 * 20 * 4]).unwrap();
    let root = column::<()>([
        image(pixels.image(), ImageStyle::contain()),
        image(pixels.image(), ImageStyle::stretch()).size(70.0, 30.0),
    ])
    .align_items(kit_view::taffy::AlignItems::FLEX_START);
    let laid = TaffyLayout::new()
        .layout(
            root,
            Viewport::new(Size::new(200.0, 100.0), 1.0),
            &mut text_system(),
            &NoWorld,
        )
        .unwrap();
    let kids = laid.root.children();
    assert_eq!(kids[0].rect.size(), Size::new(40.0, 20.0));
    assert_eq!(kids[1].rect, Rect::new(0.0, 20.0, 70.0, 50.0));
}

#[test]
fn region_without_explicit_size_measures_the_region_not_the_image() {
    let pixels = ImagePixels::new(40, 20, vec![255; 40 * 20 * 4]).unwrap();
    let region = ImageRegion::new(8, 4, 12, 6).unwrap();
    let root = column::<()>([image(pixels.image(), ImageStyle::stretch().region(region))])
        .align_items(kit_view::taffy::AlignItems::FLEX_START);
    let laid = TaffyLayout::new()
        .layout(
            root,
            Viewport::new(Size::new(200.0, 100.0), 1.0),
            &mut text_system(),
            &NoWorld,
        )
        .unwrap();
    assert_eq!(laid.root.children()[0].rect.size(), Size::new(12.0, 6.0));
}
