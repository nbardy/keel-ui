//! kurbo / peniko -> tiny-skia conversions. Pure functions; every unsupported input is a typed `RenderError`, never a
//! silently different picture.
use kit_core::Shape;
use kit_core::kurbo::{Affine, BezPath, Cap, Join, PathEl, Stroke};
use kit_core::peniko::Brush;
use kit_core::peniko::color::{AlphaColor, Srgb};
use kit_paint::RenderError;
use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Shader, StrokeDash, Transform,
};

/// Flattening tolerance (logical px) when kurbo shapes become bezier paths.
const TOLERANCE: f64 = 0.05;

pub fn unsupported(what: &str) -> RenderError {
    RenderError::Backend(format!("kit_raster: unsupported {what}"))
}

pub fn transform(a: Affine) -> Transform {
    let [a, b, c, d, e, f] = a.as_coeffs();
    Transform::from_row(a as f32, b as f32, c as f32, d as f32, e as f32, f as f32)
}

pub fn color(c: AlphaColor<Srgb>) -> tiny_skia::Color {
    let p = c.to_rgba8();
    tiny_skia::Color::from_rgba8(p.r, p.g, p.b, p.a)
}

pub fn fill_rule() -> FillRule {
    FillRule::Winding
}

pub fn bez(shape: &Shape) -> BezPath {
    shape.to_bez(TOLERANCE)
}

/// `None` means the geometry is empty (no segments): there is nothing to draw, which is the meaning, not an error.
pub fn path(bez: &BezPath) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for el in bez.elements() {
        match *el {
            PathEl::MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
            PathEl::LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
            PathEl::QuadTo(a, b) => pb.quad_to(a.x as f32, a.y as f32, b.x as f32, b.y as f32),
            PathEl::CurveTo(a, b, c) => pb.cubic_to(
                a.x as f32, a.y as f32, b.x as f32, b.y as f32, c.x as f32, c.y as f32,
            ),
            PathEl::ClosePath => pb.close(),
        }
    }
    pb.finish()
}

/// Solid colours only: the game has no gradient or image brush producer, so those are a typed error.
pub fn paint(brush: &Brush) -> Result<Paint<'static>, RenderError> {
    match brush {
        Brush::Solid(c) => Ok(Paint {
            shader: Shader::SolidColor(color(*c)),
            ..Paint::default()
        }),
        Brush::Gradient(_) | Brush::Image(_) => Err(RenderError::UnsupportedBrush),
    }
}

pub fn stroke(s: &Stroke) -> Result<tiny_skia::Stroke, RenderError> {
    if s.start_cap != s.end_cap {
        return Err(unsupported("stroke with different start and end caps"));
    }
    let line_cap = match s.start_cap {
        Cap::Butt => LineCap::Butt,
        Cap::Square => LineCap::Square,
        Cap::Round => LineCap::Round,
    };
    let line_join = match s.join {
        Join::Bevel => LineJoin::Bevel,
        Join::Miter => LineJoin::Miter,
        Join::Round => LineJoin::Round,
    };
    let dash = match s.dash_pattern.is_empty() {
        true => None,
        false => {
            let pattern = s.dash_pattern.iter().map(|d| *d as f32).collect();
            Some(
                StrokeDash::new(pattern, s.dash_offset as f32)
                    .ok_or_else(|| unsupported("invalid dash pattern"))?,
            )
        }
    };
    Ok(tiny_skia::Stroke {
        width: s.width as f32,
        miter_limit: s.miter_limit as f32,
        line_cap,
        line_join,
        dash,
    })
}
