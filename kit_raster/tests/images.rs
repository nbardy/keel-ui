use kit_core::kurbo::Rect;
use kit_paint::{Draw, DrawList, RenderError};
use kit_raster::{RasterUi, tiny_skia::Pixmap};
use kit_view::{Image, ImagePixels, ImageSize, ImageStyle, testing::text_system};

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
