use crate::convert;
use kit_core::kurbo::{Affine, Rect, RoundedRect, Shape as _};
use kit_paint::RenderError;
use kit_view::{Filter, Image, ImageKey, ImagePixels, ImageRegion, ImageSource, ImageStyle};
use std::collections::BTreeMap;
use tiny_skia::{FilterQuality, Paint, Pattern, Pixmap, SpreadMode, Transform};

#[derive(Default)]
pub(crate) struct Images(BTreeMap<ImageKey, Option<ImagePixels>>);

impl Images {
    pub fn register(&mut self, pixels: &ImagePixels) -> Result<Image, RenderError> {
        let image = pixels.image();
        match self.0.entry(image.key()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(pixels.clone()));
            }
            std::collections::btree_map::Entry::Occupied(entry) => {
                if entry.get().is_none() {
                    return Err(RenderError::ImageReleased(image));
                }
            }
        }
        Ok(image)
    }
    pub fn release(&mut self, image: Image) -> Result<(), RenderError> {
        let entry = self
            .0
            .get_mut(&image.key())
            .ok_or(RenderError::ImageNotRegistered(image))?;
        entry.take().ok_or(RenderError::ImageReleased(image))?;
        Ok(())
    }
    pub fn get(&self, image: Image) -> Result<&ImagePixels, RenderError> {
        match image.source() {
            ImageSource::Texture(_) => Err(RenderError::ExternalTextureOnCpu(image)),
            ImageSource::Pixels(key) => self
                .0
                .get(&key)
                .ok_or(RenderError::ImageNotRegistered(image))?
                .as_ref()
                .ok_or(RenderError::ImageReleased(image)),
        }
    }
}

/// `texels` is the region the GPU samples too (`kit_paint::image_texels`): only those texels are tinted into the
/// tile, so the CPU and the GPU sample the same pixels and clamp at the same edges.
pub(crate) fn draw(
    target: &mut Pixmap,
    pixels: &ImagePixels,
    texels: ImageRegion,
    rect: Rect,
    style: ImageStyle,
    scale: f32,
) -> Result<(), RenderError> {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return Ok(());
    }
    let rw = texels.width() as usize;
    let mut tile = Pixmap::new(texels.width(), texels.height())
        .ok_or_else(|| RenderError::Backend("image allocation failed".into()))?;
    let [tr, tg, tb, ta] = style.tint.components;
    let stride = pixels.size().width() as usize * 4;
    let first_column = texels.x() as usize * 4;
    for (row, dst_row) in tile.data_mut().chunks_exact_mut(rw * 4).enumerate() {
        let start = (texels.y() as usize + row) * stride + first_column;
        let src_row = &pixels.rgba()[start..start + rw * 4];
        for (src, dst) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
            let a = f32::from(src[3]) / 255.0 * ta;
            for (i, tint) in [tr, tg, tb].into_iter().enumerate() {
                dst[i] = (f32::from(src[i]) * tint * a).round() as u8;
            }
            dst[3] = (255.0 * a).round() as u8;
        }
    }
    let to_rect = Transform::from_scale(
        rect.width() as f32 / texels.width() as f32,
        rect.height() as f32 / texels.height() as f32,
    )
    .post_translate(rect.x0 as f32, rect.y0 as f32);
    let quality = match style.filter {
        Filter::Linear => FilterQuality::Bilinear,
        Filter::Nearest => FilterQuality::Nearest,
    };
    let paint = Paint {
        shader: Pattern::new(tile.as_ref(), SpreadMode::Pad, quality, 1.0, to_rect),
        ..Paint::default()
    };
    let rounded = RoundedRect::from_rect(rect, style.radii);
    if let Some(path) = convert::path(&rounded.to_path(0.05)) {
        target.fill_path(
            &path,
            &paint,
            convert::fill_rule(),
            convert::transform(Affine::scale(f64::from(scale))),
            None,
        );
    }
    Ok(())
}
