use crate::{color::Output, gpu::Gpu};
use kit_core::peniko::Color;
use kit_paint::RenderError;
use kit_view::{Filter, Image, ImageKey, ImagePixels, ImageSize};
use std::collections::BTreeMap;

pub(crate) struct Texture {
    linear: wgpu::BindGroup,
    nearest: wgpu::BindGroup,
}

#[derive(Default)]
pub(crate) struct Images(BTreeMap<ImageKey, Option<Texture>>);

impl Images {
    fn insert(&mut self, gpu: &Gpu, image: Image, view: &wgpu::TextureView) {
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        self.0.insert(
            image.key(),
            Some(Texture {
                linear: gpu.texture_bind_group(view, &sampler),
                nearest: gpu.texture_bind_group(view, &gpu.nearest),
            }),
        );
    }
    pub fn register_pixels(
        &mut self,
        gpu: &Gpu,
        out: Output,
        pixels: &ImagePixels,
    ) -> Result<Image, RenderError> {
        let image = pixels.image();
        if self.0.contains_key(&image.key()) {
            self.get(image)?;
            return Ok(image);
        }
        let size = pixels.size();
        let format = match out {
            Output::Raw => wgpu::TextureFormat::Rgba8Unorm,
            Output::Srgb => wgpu::TextureFormat::Rgba8UnormSrgb,
        };
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("kit image"),
            size: wgpu::Extent3d {
                width: size.width(),
                height: size.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let bytes: Vec<u8> = pixels
            .rgba()
            .chunks_exact(4)
            .flat_map(|px| {
                let color = Color::from_rgba8(px[0], px[1], px[2], px[3]);
                let mut rgba = out.premul(color, 1.0);
                if out == Output::Srgb {
                    for c in &mut rgba[..3] {
                        *c = encode(*c);
                    }
                }
                rgba.map(|c| (c * 255.0).round() as u8)
            })
            .collect();
        gpu.queue.write_texture(
            texture.as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width() * 4),
                rows_per_image: Some(size.height()),
            },
            texture.size(),
        );
        self.insert(gpu, image, &texture.create_view(&Default::default()));
        Ok(image)
    }
    pub fn register_texture(
        &mut self,
        gpu: &Gpu,
        view: &wgpu::TextureView,
        size: ImageSize,
    ) -> Image {
        let image = Image::external(size);
        self.insert(gpu, image, view);
        image
    }
    pub fn release(&mut self, image: Image) -> Result<(), RenderError> {
        let entry = self
            .0
            .get_mut(&image.key())
            .ok_or(RenderError::ImageNotRegistered(image))?;
        entry.take().ok_or(RenderError::ImageReleased(image))?;
        Ok(())
    }
    pub fn get(&self, image: Image) -> Result<&Texture, RenderError> {
        self.0
            .get(&image.key())
            .ok_or(RenderError::ImageNotRegistered(image))?
            .as_ref()
            .ok_or(RenderError::ImageReleased(image))
    }
    pub fn bind(&self, key: ImageKey, filter: Filter) -> &wgpu::BindGroup {
        let texture = self.0[&key].as_ref().expect("planner validated image");
        match filter {
            Filter::Linear => &texture.linear,
            Filter::Nearest => &texture.nearest,
        }
    }
}

fn encode(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}
