//! `RasterUi`: the CPU renderer. One thin dispatcher (`Frame::draw`), one handler per `Draw` variant.
use crate::convert;
use crate::glyph;
use crate::layers::{Layer, LayerStack, blank, blank_mask};
use crate::shadow::{self, DropShadow};
use kit_core::kurbo::{Affine, Point, Rect, RoundedRect, RoundedRectRadii, Shape as _, Stroke};
use kit_core::peniko::{Brush, Color};
use kit_core::{Border, Clip, Shadow, Shape};
use kit_paint::{Draw, DrawList, RenderError, TOFU_STROKE, caret_bar, em, layout_glyphs, tofu_box};
use kit_view::kit_text::{GlyphRaster, PhysicalGlyph};
use kit_view::{CosmicText, TextLayoutId};
use tiny_skia::{Mask, Pixmap, PixmapPaint, Transform};

pub struct RasterUi {
    /// Device px per logical px.
    scale: f32,
    images: crate::image::Images,
}

impl Default for RasterUi {
    fn default() -> Self {
        Self::new()
    }
}

impl RasterUi {
    pub fn new() -> Self {
        Self::with_scale(1.0)
    }
    pub fn with_scale(scale: f32) -> Self {
        Self {
            scale,
            images: crate::image::Images::default(),
        }
    }

    /// Register immutable straight-alpha sRGB RGBA8 pixels. Repeated live registration is idempotent.
    pub fn register_pixels(
        &mut self,
        pixels: &kit_view::ImagePixels,
    ) -> Result<kit_view::Image, RenderError> {
        self.images.register(pixels)
    }

    /// Release this backend's bytes; this handle remains a tombstone and cannot be registered again.
    pub fn release(&mut self, image: kit_view::Image) -> Result<(), RenderError> {
        self.images.release(image)
    }

    /// Draws `list` over the existing contents of `target`. All-or-nothing: on `Err` the target is untouched.
    pub fn render(
        &mut self,
        list: &DrawList,
        text: &mut CosmicText,
        target: &mut Pixmap,
    ) -> Result<(), RenderError> {
        let mut frame = Frame {
            images: &self.images,
            scale: self.scale,
            size: (target.width(), target.height()),
            text,
            stack: LayerStack::over(target),
        };
        for item in &list.items {
            frame.draw(item)?;
        }
        let done = frame.stack.finish()?;
        target.data_mut().copy_from_slice(done.data());
        Ok(())
    }
}

struct Frame<'a> {
    images: &'a crate::image::Images,
    scale: f32,
    size: (u32, u32),
    text: &'a mut CosmicText,
    stack: LayerStack,
}

impl Frame<'_> {
    fn draw(&mut self, item: &Draw) -> Result<(), RenderError> {
        match item {
            Draw::Image { image, rect, style } => crate::image::draw(
                self.stack.top(),
                self.images.get(*image)?,
                *rect,
                *style,
                self.scale,
            ),
            Draw::Fill {
                shape,
                brush,
                transform,
            } => self.fill(shape, brush, *transform),
            Draw::Stroke {
                shape,
                stroke,
                brush,
                transform,
            } => self.stroke(shape, stroke, brush, *transform),
            Draw::RoundRect {
                rect,
                radii,
                fill,
                border,
                shadow,
            } => self.round_rect(*rect, *radii, fill, border, shadow),
            Draw::Glyphs {
                layout,
                origin,
                brush,
            } => self.glyphs(*layout, *origin, brush),
            Draw::Caret { rect, brush, width } => self.caret(*rect, brush, *width),
            Draw::PushLayer { opacity, clip } => self.push_layer(*opacity, clip),
            Draw::PopLayer => self.stack.pop(),
        }
    }

    fn to_device(&self, a: Affine) -> Affine {
        Affine::scale(f64::from(self.scale)) * a
    }

    fn scale_transform(&self) -> Transform {
        Transform::from_scale(self.scale, self.scale)
    }

    fn fill(&mut self, shape: &Shape, brush: &Brush, transform: Affine) -> Result<(), RenderError> {
        let paint = convert::paint(brush)?;
        let t = convert::transform(self.to_device(transform));
        if let Some(path) = convert::path(&convert::bez(shape)) {
            self.stack
                .top()
                .fill_path(&path, &paint, convert::fill_rule(), t, None);
        }
        Ok(())
    }

    fn stroke(
        &mut self,
        shape: &Shape,
        stroke: &Stroke,
        brush: &Brush,
        transform: Affine,
    ) -> Result<(), RenderError> {
        let paint = convert::paint(brush)?;
        let style = convert::stroke(stroke)?;
        let t = convert::transform(self.to_device(transform));
        if let Some(path) = convert::path(&convert::bez(shape)) {
            self.stack.top().stroke_path(&path, &paint, &style, t, None);
        }
        Ok(())
    }

    /// Order: shadow, fill, then the border drawn inside the rect's edge (stroke centred half a border-width in), so
    /// the border never changes the outer size.
    fn round_rect(
        &mut self,
        rect: Rect,
        radii: RoundedRectRadii,
        fill: &Brush,
        border: &Border,
        shadow: &Shadow,
    ) -> Result<(), RenderError> {
        let shape = RoundedRect::from_rect(rect, radii);
        match shadow {
            Shadow::None => {}
            Shadow::Drop {
                offset,
                blur,
                spread,
                color,
            } => shadow::draw(
                self.stack.top(),
                &shape,
                &DropShadow {
                    offset: *offset,
                    blur: *blur,
                    spread: *spread,
                    color: *color,
                },
                self.scale,
            )?,
        }
        self.fill(&Shape::RoundRect(shape), fill, Affine::IDENTITY)?;
        match border {
            Border::None => Ok(()),
            Border::Solid { width, color } => self.border(rect, radii, *width, *color),
        }
    }

    fn border(
        &mut self,
        rect: Rect,
        radii: RoundedRectRadii,
        width: f64,
        color: Color,
    ) -> Result<(), RenderError> {
        let h = width / 2.0;
        let inner = Rect::new(rect.x0 + h, rect.y0 + h, rect.x1 - h, rect.y1 - h);
        let r = |v: f64| (v - h).max(0.0);
        let inner_radii = RoundedRectRadii::new(
            r(radii.top_left),
            r(radii.top_right),
            r(radii.bottom_right),
            r(radii.bottom_left),
        );
        self.stroke(
            &Shape::RoundRect(RoundedRect::from_rect(inner, inner_radii)),
            &Stroke::new(width),
            &Brush::Solid(color),
            Affine::IDENTITY,
        )
    }

    fn glyphs(
        &mut self,
        layout: TextLayoutId,
        origin: Point,
        brush: &Brush,
    ) -> Result<(), RenderError> {
        for (at, ink) in layout_glyphs(self.text, layout, origin, self.scale, brush)? {
            self.glyph(at, &ink)?;
        }
        Ok(())
    }

    fn glyph(&mut self, at: PhysicalGlyph, tint: &Brush) -> Result<(), RenderError> {
        match self.text.rasterize(at.cache_key) {
            GlyphRaster::Empty => Ok(()),
            GlyphRaster::Missing => self.tofu(at.x, at.y, em(&at), tint),
            GlyphRaster::Image(image) => {
                let (px, py) = (at.x + image.left, at.y + image.top);
                let mut ink = convert::paint(tint)?;
                // Brush coordinates are logical px; the tile's origin is (px, py) in device px.
                ink.shader.transform(
                    self.scale_transform()
                        .post_translate(-(px as f32), -(py as f32)),
                );
                let tile = glyph::tile(&image, &ink)?;
                self.stack.top().draw_pixmap(
                    px,
                    py,
                    tile.as_ref(),
                    &PixmapPaint::default(),
                    Transform::identity(),
                    None,
                );
                Ok(())
            }
        }
    }

    /// The text system has no image for this glyph: draw a visible hollow box (never skip silently).
    fn tofu(&mut self, x: i32, baseline: i32, em: f64, tint: &Brush) -> Result<(), RenderError> {
        let boxed = tofu_box(f64::from(x), f64::from(baseline), em);
        let mut ink = convert::paint(tint)?;
        ink.shader.transform(self.scale_transform());
        if let Some(path) = convert::path(&boxed.to_path(0.05)) {
            let style = tiny_skia::Stroke {
                width: TOFU_STROKE as f32,
                ..tiny_skia::Stroke::default()
            };
            self.stack
                .top()
                .stroke_path(&path, &ink, &style, Transform::identity(), None);
        }
        Ok(())
    }

    fn caret(&mut self, at: Rect, brush: &Brush, width: f64) -> Result<(), RenderError> {
        let bar = caret_bar(at, width, f64::from(self.scale));
        self.fill(&Shape::Rect(bar), brush, Affine::IDENTITY)
    }

    fn push_layer(&mut self, opacity: f32, clip: &Clip) -> Result<(), RenderError> {
        let (w, h) = self.size;
        let clip = match clip {
            Clip::Unclipped => None,
            Clip::ToRect(rect) => Some(self.mask(*rect, w, h)?),
        };
        self.stack.push(Layer {
            pixmap: blank(w, h)?,
            opacity,
            clip,
        });
        Ok(())
    }

    /// Anti-aliased coverage of `rect`; an empty rect yields an empty mask (clips everything).
    fn mask(&self, rect: Rect, w: u32, h: u32) -> Result<Mask, RenderError> {
        let mut mask = blank_mask(w, h)?;
        if let Some(path) = convert::path(&convert::bez(&Shape::Rect(rect))) {
            mask.fill_path(&path, convert::fill_rule(), true, self.scale_transform());
        }
        Ok(mask)
    }
}
