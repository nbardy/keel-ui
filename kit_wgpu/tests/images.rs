mod common;
use common::Harness;
use kit_core::{
    Viewport,
    kurbo::{Rect, Size},
};
use kit_paint::{Draw, DrawList, RenderError, paint};
use kit_view::{
    Image, ImagePixels, ImageSize, ImageStyle, LaidOut, Placed, Shown, image, testing::text_system,
};
use kit_wgpu::WgpuTarget;

fn list(handle: Image) -> DrawList {
    DrawList {
        redraw: kit_core::Redraw::Idle,
        items: vec![Draw::Image {
            image: handle,
            rect: Rect::new(0.0, 0.0, 16.0, 16.0),
            style: ImageStyle::stretch(),
        }],
    }
}
fn near(actual: [u8; 4], expected: [u8; 4]) {
    assert!(
        actual
            .into_iter()
            .zip(expected)
            .all(|(a, b)| a.abs_diff(b) <= 2),
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn game_render_pass_texture_is_sampled_through_a_node_and_updates_live() {
    let mut h = Harness::new(16, 16, 1.0);
    let texture = h.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("game minimap"),
        size: wgpu::Extent3d {
            width: 8,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let handle = h.ui.register_texture(&view, ImageSize::new(8, 4).unwrap());
    let mut text = text_system();
    let laid = LaidOut::assemble(
        image::<()>(handle, ImageStyle::contain()),
        Viewport::new(Size::new(16.0, 16.0), 1.0),
        &[Placed {
            rect: Rect::new(0.0, 0.0, 16.0, 16.0),
            shown: Shown::Visible,
        }],
        &mut text,
    )
    .unwrap();
    let theme = kit_core::Theme::new(kit_core::ColorVision::Normal, kit_core::MotionPref::Full);
    let memory = kit_input::UiMemory::new(laid.viewport);
    let draws = paint(&laid, &memory, &theme, kit_core::Time::default());
    for (color, expected) in [
        ([0.125, 0.25, 0.5, 0.5], [32, 64, 128, 128]),
        ([0.75, 0.125, 0.25, 1.0], [191, 32, 64, 255]),
    ] {
        h.frame(Some([0.0; 4]), &DrawList::default(), &mut text)
            .unwrap();
        let mut encoder = h.device.create_command_encoder(&Default::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: color[0],
                            g: color[1],
                            b: color[2],
                            a: color[3],
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        h.ui.render(
            &draws,
            &mut text,
            WgpuTarget {
                encoder: &mut encoder,
                surface: &h.surface,
            },
        )
        .unwrap();
        h.queue.submit([encoder.finish()]);
        h.ui.frame_submitted();
        let pixels = h.read();
        near(pixels[8 * 16 + 8], expected);
        near(pixels[24], [0; 4]);
        near(pixels[14 * 16 + 8], [0; 4]);
    }
}

#[test]
fn gpu_rejects_unregistered_and_released_pixels_and_textures() {
    let mut h = Harness::new(16, 16, 1.0);
    let pixels = ImagePixels::new(1, 1, vec![255; 4]).unwrap();
    let handle = pixels.image();
    let mut text = text_system();
    assert_eq!(
        h.frame(None, &list(handle), &mut text),
        Err(RenderError::ImageNotRegistered(handle))
    );
    assert_eq!(
        h.ui.release(handle),
        Err(RenderError::ImageNotRegistered(handle))
    );
    assert_eq!(h.ui.register_pixels(&pixels).unwrap(), handle);
    h.frame(Some([0.0; 4]), &list(handle), &mut text).unwrap();
    near(h.read()[8 * 16 + 8], [255; 4]);
    h.ui.release(handle).unwrap();
    assert_eq!(
        h.frame(None, &list(handle), &mut text),
        Err(RenderError::ImageReleased(handle))
    );
    assert_eq!(
        h.ui.register_pixels(&pixels),
        Err(RenderError::ImageReleased(handle))
    );
    assert_eq!(
        h.ui.release(handle),
        Err(RenderError::ImageReleased(handle))
    );
    let unregistered = Image::external(ImageSize::new(16, 16).unwrap());
    assert_eq!(
        h.frame(None, &list(unregistered), &mut text),
        Err(RenderError::ImageNotRegistered(unregistered))
    );
    let texture = h.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let external = h.ui.register_texture(
        &texture.create_view(&Default::default()),
        ImageSize::new(1, 1).unwrap(),
    );
    h.ui.release(external).unwrap();
    assert_eq!(
        h.frame(None, &list(external), &mut text),
        Err(RenderError::ImageReleased(external))
    );
}

#[test]
fn straight_pixels_match_premultiplied_unorm_and_srgb_targets() {
    for (format, expected) in [
        (wgpu::TextureFormat::Rgba8Unorm, [100, 50, 25, 128]),
        (wgpu::TextureFormat::Rgba8UnormSrgb, [147, 72, 34, 128]),
    ] {
        let mut h = Harness::with_format(16, 16, 1.0, format);
        let pixels = ImagePixels::new(1, 1, vec![200, 100, 50, 128]).unwrap();
        let handle = h.ui.register_pixels(&pixels).unwrap();
        h.frame(Some([0.0; 4]), &list(handle), &mut text_system())
            .unwrap();
        near(h.read()[8 * 16 + 8], expected);
    }
}
