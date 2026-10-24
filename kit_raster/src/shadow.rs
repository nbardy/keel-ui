//! Drop shadows, approximated: the (spread, offset) rounded rect is filled into a scratch pixmap and blurred with
//! three box passes (which converge on a Gaussian). Not pixel-identical to a GPU SDF shadow; it is the same shape,
//! softness and falloff to within a few levels, which is what a golden test of layout and colour needs.
use crate::convert::{self, unsupported};
use crate::layers::blank;
use kit_core::kurbo::{Affine, RoundedRect, RoundedRectRadii, Vec2};
use kit_core::peniko::Color;
use kit_paint::RenderError;
use tiny_skia::{Paint, Pixmap, PixmapPaint, Shader, Transform};

/// CSS convention: the blur radius is two standard deviations.
const SIGMA_PER_BLUR: f64 = 0.5;
const PASSES: usize = 3;

pub struct DropShadow {
    pub offset: Vec2,
    pub blur: f64,
    pub spread: f64,
    pub color: Color,
}

fn grow(r: &RoundedRect, spread: f64, offset: Vec2) -> RoundedRect {
    let rect = r.rect();
    let radii = r.radii();
    let g = |v: f64| (v + spread).max(0.0);
    RoundedRect::from_rect(
        kit_core::kurbo::Rect::new(
            rect.x0 - spread + offset.x,
            rect.y0 - spread + offset.y,
            rect.x1 + spread + offset.x,
            rect.y1 + spread + offset.y,
        ),
        RoundedRectRadii::new(
            g(radii.top_left),
            g(radii.top_right),
            g(radii.bottom_right),
            g(radii.bottom_left),
        ),
    )
}

/// Box radius whose `PASSES`-fold application has the variance of a Gaussian with `sigma` (device px).
fn box_radius(sigma: f64) -> usize {
    let ideal_width = (12.0 * sigma * sigma / PASSES as f64 + 1.0).sqrt();
    ((ideal_width - 1.0) / 2.0).round() as usize
}

/// One running-sum box blur along rows or columns of premultiplied RGBA8; outside the pixmap is transparent.
fn box_pass(data: &mut [u8], width: usize, height: usize, radius: usize, horizontal: bool) {
    let (lines, len, step, line_step) = match horizontal {
        true => (height, width, 4, width * 4),
        false => (width, height, width * 4, 4),
    };
    let div = (2 * radius + 1) as u32;
    let mut out = vec![0u8; len];
    for line in 0..lines {
        for channel in 0..4 {
            let at = |i: usize| line * line_step + i * step + channel;
            let mut sum: u32 = (0..=radius.min(len - 1))
                .map(|i| u32::from(data[at(i)]))
                .sum();
            for (i, o) in out.iter_mut().enumerate() {
                *o = ((sum + div / 2) / div) as u8;
                let entering = i + radius + 1;
                if entering < len {
                    sum += u32::from(data[at(entering)]);
                }
                if i >= radius {
                    sum -= u32::from(data[at(i - radius)]);
                }
            }
            for (i, o) in out.iter().enumerate() {
                data[at(i)] = *o;
            }
        }
    }
}

/// Draws the shadow of `shape` (logical px) under whatever is drawn next. `scale` is device px per logical px.
pub fn draw(
    target: &mut Pixmap,
    shape: &RoundedRect,
    shadow: &DropShadow,
    scale: f32,
) -> Result<(), RenderError> {
    let s = f64::from(scale);
    let radius = box_radius(shadow.blur * SIGMA_PER_BLUR * s);
    let pad = (radius * PASSES) as f64 + 2.0;
    let casting = grow(shape, shadow.spread, shadow.offset);
    let device = Affine::scale(s).transform_rect_bbox(casting.rect());
    let (x0, y0) = (
        (device.x0 - pad).floor() as i32,
        (device.y0 - pad).floor() as i32,
    );
    let (x1, y1) = (
        (device.x1 + pad).ceil() as i32,
        (device.y1 + pad).ceil() as i32,
    );
    let (w, h) = ((x1 - x0).max(1) as u32, (y1 - y0).max(1) as u32);

    let mut scratch = blank(w, h)?;
    let ink = Paint {
        shader: Shader::SolidColor(convert::color(shadow.color)),
        ..Paint::default()
    };
    if let Some(path) = convert::path(&kit_core::kurbo::Shape::to_path(&casting, 0.05)) {
        let to_scratch = Affine::translate((-f64::from(x0), -f64::from(y0))) * Affine::scale(s);
        scratch.fill_path(
            &path,
            &ink,
            convert::fill_rule(),
            convert::transform(to_scratch),
            None,
        );
    }
    let (wu, hu) = (w as usize, h as usize);
    for _ in 0..PASSES {
        box_pass(scratch.data_mut(), wu, hu, radius, true);
        box_pass(scratch.data_mut(), wu, hu, radius, false);
    }
    if scratch.width() != w || scratch.height() != h {
        return Err(unsupported("shadow scratch size drifted"));
    }
    target.draw_pixmap(
        x0,
        y0,
        scratch.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
    Ok(())
}
