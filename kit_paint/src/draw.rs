use kit_core::kurbo::{Affine, Point, Rect, RoundedRectRadii, Stroke};
use kit_core::peniko::Brush;
use kit_core::{Border, Clip, Redraw, Shadow, Shape};
use kit_view::{Image, ImageStyle, TextLayoutId};

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum RenderError {
    /// A gradient or image brush: the game paints solid colours only, so no backend draws anything else.
    UnsupportedBrush,
    ImageNotRegistered(Image),
    ImageReleased(Image),
    ExternalTextureOnCpu(Image),
    /// The list has a `PopLayer` without a `PushLayer`, or a layer left open.
    UnbalancedLayers,
    /// A `Glyphs` names a layout the text system no longer holds (evicted by `end_frame` before drawing).
    TextNotLive(TextLayoutId),
    /// Device lost, surface outdated, etc.; the string is the backend's own description.
    Backend(String),
}

#[derive(Clone, PartialEq, Debug)]
pub enum Draw {
    /// Image rect is already fitted by the painter; style carries tint, filtering and corner radii.
    Image {
        image: Image,
        rect: Rect,
        style: ImageStyle,
    },
    Fill {
        shape: Shape,
        brush: Brush,
        transform: Affine,
    },
    Stroke {
        shape: Shape,
        stroke: Stroke,
        brush: Brush,
        transform: Affine,
    },
    RoundRect {
        rect: Rect,
        radii: RoundedRectRadii,
        fill: Brush,
        border: Border,
        shadow: Shadow,
    },
    /// Every glyph of a `CosmicText` layout (`buffer(layout).layout_runs()`), with `origin` the layout's top-left.
    /// Glyphs without a colour of their own (`color_opt: None`) use `brush`. The renderer is handed the same
    /// `CosmicText`: the painter has no access to shaped glyphs, only to ids.
    Glyphs {
        layout: TextLayoutId,
        origin: Point,
        brush: Brush,
    },
    /// Text caret of a focused field: a zero-width, line-tall `rect` in logical px, drawn `width` wide with its left
    /// edge snapped to the device pixel grid by the backend (which knows the scale).
    Caret {
        rect: Rect,
        brush: Brush,
        width: f64,
    },
    /// Opens a compositing group (normal blend). Opacity is relative to the enclosing group (nesting multiplies); the
    /// group is scissored to `clip`. Every `PushLayer` is closed by exactly one `PopLayer`.
    PushLayer {
        opacity: f32,
        clip: Clip,
    },
    PopLayer,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct DrawList {
    pub items: Vec<Draw>,
    /// Demand from painter-owned transitions, caret blinking and scrollbar fading.
    pub redraw: Redraw,
}

impl DrawList {
    /// Total demand for this frame. Pass the completed view build, Store and UiMemory
    /// demands after layout and Router::frame. Input/app changes independently wake the host.
    pub fn frame_redraw(&self, view: Redraw, store: Redraw, input: Redraw) -> Redraw {
        self.redraw.earliest(view).earliest(store).earliest(input)
    }

    /// Layer nesting never goes negative and ends at zero.
    pub fn layers_balanced(&self) -> bool {
        let mut depth = 0usize;
        for item in &self.items {
            match item {
                Draw::PushLayer { .. } => depth += 1,
                Draw::PopLayer => match depth.checked_sub(1) {
                    Some(d) => depth = d,
                    None => return false,
                },
                Draw::Fill { .. }
                | Draw::Stroke { .. }
                | Draw::RoundRect { .. }
                | Draw::Glyphs { .. }
                | Draw::Image { .. }
                | Draw::Caret { .. } => {}
            }
        }
        depth == 0
    }

    /// For backends without offscreen groups: per item, the product of the opacities of all layers enclosing it.
    /// `PushLayer`/`PopLayer` items report the value outside their own layer.
    pub fn effective_opacities(&self) -> Vec<f32> {
        let mut stack: Vec<f32> = Vec::new();
        let mut out = Vec::with_capacity(self.items.len());
        for item in &self.items {
            let outside = stack.last().copied().unwrap_or(1.0);
            match item {
                Draw::PushLayer { opacity, .. } => {
                    out.push(outside);
                    stack.push(outside * opacity);
                }
                Draw::PopLayer => {
                    stack.pop();
                    out.push(stack.last().copied().unwrap_or(1.0));
                }
                Draw::Fill { .. }
                | Draw::Stroke { .. }
                | Draw::RoundRect { .. }
                | Draw::Glyphs { .. }
                | Draw::Image { .. }
                | Draw::Caret { .. } => out.push(outside),
            }
        }
        out
    }
}
