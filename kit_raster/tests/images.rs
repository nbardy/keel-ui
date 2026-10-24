use kit_core::kurbo::Rect;
use kit_paint::{Draw, DrawList, RenderError};
use kit_raster::{RasterUi, tiny_skia::Pixmap};
use kit_view::{
    Image, ImagePixels, ImageRegion, ImageRegionError, ImageSize, ImageStyle, testing::text_system,
};

/// 2x2, top row then bottom row: red, green / blue, white. Each quadrant is a different colour.
fn quadrants() -> ImagePixels {
    ImagePixels::new(
        2,
        2,
        vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ],
    )
    .unwrap()
}

fn region_draw(image: Image, region: ImageRegion) -> DrawList {
    DrawList {
        redraw: kit_core::Redraw::Idle,
        items: vec![Draw::Image {
            image,
            rect: Rect::new(0.0, 0.0, 4.0, 4.0),
            style: ImageStyle::stretch().region(region),
        }],
    }
}

fn draw(image: Image) -> DrawList {
    DrawList {
        redraw: kit_core::Redraw::Idle,
        items: vec![Draw::Image {
            image,
            rect: Rect::new(0.0, 0.0, 4.0, 4.0),
            style: ImageStyle::stretch(),
        }],
    }
}

#[test]
fn registration_release_and_external_errors_leave_target_untouched() {
    let pixels = ImagePixels::new(1, 1, vec![200, 100, 50, 128]).unwrap();
    let image = pixels.image();
    let mut ui = RasterUi::new();
    let mut target = Pixmap::new(4, 4).unwrap();
    let mut text = text_system();
    assert_eq!(
        ui.render(&draw(image), &mut text, &mut target),
        Err(RenderError::ImageNotRegistered(image))
    );
    assert_eq!(
        ui.release(image),
        Err(RenderError::ImageNotRegistered(image))
    );
    assert_eq!(ui.register_pixels(&pixels).unwrap(), image);
    ui.render(&draw(image), &mut text, &mut target).unwrap();
    let px = target.pixel(2, 2).unwrap();
    assert_eq!(
        [px.red(), px.green(), px.blue(), px.alpha()],
        [100, 50, 25, 128]
    );
    ui.release(image).unwrap();
    let before = target.clone();
    assert_eq!(
        ui.render(&draw(image), &mut text, &mut target),
        Err(RenderError::ImageReleased(image))
    );
    assert_eq!(target.data(), before.data());
    assert_eq!(
        ui.register_pixels(&pixels),
        Err(RenderError::ImageReleased(image))
    );
    assert_eq!(ui.release(image), Err(RenderError::ImageReleased(image)));
    let external = Image::external(ImageSize::new(4, 4).unwrap());
    assert_eq!(
        ui.render(&draw(external), &mut text, &mut target),
        Err(RenderError::ExternalTextureOnCpu(external))
    );
}

#[test]
fn region_draws_only_its_own_texels() {
    let pixels = quadrants();
    let mut ui = RasterUi::new();
    ui.register_pixels(&pixels).unwrap();
    let mut text = text_system();
    for ((x, y), expected) in [
        ((0, 0), [255, 0, 0, 255]),
        ((1, 0), [0, 255, 0, 255]),
        ((0, 1), [0, 0, 255, 255]),
        ((1, 1), [255, 255, 255, 255]),
    ] {
        let mut target = Pixmap::new(4, 4).unwrap();
        let region = ImageRegion::new(x, y, 1, 1).unwrap();
        ui.render(&region_draw(pixels.image(), region), &mut text, &mut target)
            .unwrap();
        for px in target.pixels() {
            assert_eq!(
                [px.red(), px.green(), px.blue(), px.alpha()],
                expected,
                "region at ({x}, {y})"
            );
        }
    }
}

#[test]
fn out_of_bounds_region_is_a_typed_error_and_draws_nothing() {
    let pixels = quadrants();
    let image = pixels.image();
    let mut ui = RasterUi::new();
    ui.register_pixels(&pixels).unwrap();
    let mut target = Pixmap::new(4, 4).unwrap();
    let before = target.clone();
    let region = ImageRegion::new(1, 1, 2, 1).unwrap();
    assert_eq!(
        ui.render(&region_draw(image, region), &mut text_system(), &mut target),
        Err(RenderError::ImageRegion {
            image,
            error: ImageRegionError::OutOfBounds {
                region,
                image: image.size(),
            },
        })
    );
    assert_eq!(target.data(), before.data());
    assert_eq!(
        ImageRegion::new(0, 0, 0, 1),
        Err(ImageRegionError::Empty {
            width: 0,
            height: 1
        })
    );
}
