use crate::{Draw, DrawList};
use kit_core::kurbo::{Affine, Ellipse, Point, Rect, RoundedRect, RoundedRectRadii, Vec2};
use kit_core::peniko::{Brush, Color};
use kit_core::{Border, Clip, MotionPref, Redraw, Shadow, Shape, ShapeSpec, Theme, Time};
use kit_input::{AnimKey, Focus, UiMemory};
use kit_view::{
    ClipSpec, FieldGeometry, FieldStyle, FieldView, LaidKind, LaidNode, LaidOut, Layer, Paint,
    Shown, Stacking, TextLayout,
};

/// Default painter for the built-in nodes. Pure given `(laid, memory, theme, now)`.
pub fn paint<Msg>(laid: &LaidOut<Msg>, memory: &UiMemory, theme: &Theme, now: Time) -> DrawList {
    let mut p = Painter {
        laid,
        memory,
        theme,
        now,
        out: Vec::new(),
        redraw: Redraw::Idle,
    };
    p.node(&laid.root, Vec2::ZERO);
    p.portals();
    p.drag_ghosts();
    DrawList {
        items: p.out,
        redraw: p.redraw,
    }
}

struct Painter<'a, Msg> {
    laid: &'a LaidOut<Msg>,
    memory: &'a UiMemory,
    theme: &'a Theme,
    now: Time,
    out: Vec<Draw>,
    redraw: Redraw,
}

const CARET_BLINK_MS: u128 = 530;
const GHOST_OPACITY: f32 = 0.8;

impl<Msg> Painter<'_, Msg> {
    fn node(&mut self, n: &LaidNode<Msg>, off: Vec2) {
        let rect = n.rect + off;
        match &n.kind {
            LaidKind::Stack { children } => self.all(children, off),
            LaidKind::Panel { surface, children } => {
                self.out.push(Draw::RoundRect {
                    rect,
                    radii: surface.radii,
                    fill: surface.fill.clone(),
                    border: surface.border,
                    shadow: surface.shadow,
                });
                self.all(children, off);
            }
            LaidKind::Text { layout, color } => self.out.push(Draw::Glyphs {
                layout: layout.id,
                origin: rect.origin(),
                brush: Brush::Solid(*color),
            }),
            LaidKind::Image { image, image_style } => self.out.push(Draw::Image {
                image: *image,
                rect: image_style
                    .fit
                    .rect(image_style.crop.logical(image.size()), rect),
                style: *image_style,
            }),
            LaidKind::Shape { shape, paint } => self.shape(shape, paint, rect),
            LaidKind::Interactive { id, on, child, .. } => {
                self.node(child, off);
                self.interaction_overlay(*id, on.captures_press() || on.hover.is_some(), rect);
                self.focus_ring(*id, rect);
            }
            LaidKind::Scroll { id, spec, child } => {
                self.out.push(Draw::PushLayer {
                    opacity: 1.0,
                    clip: Clip::ToRect(rect),
                });
                self.node(child, off - self.memory.scroll_offset(*id));
                // scroll: bars inside the area's clip, over its content (scrollbar.rs).
                let (area, size, now) = ((*id, spec), child.rect.size(), self.now);
                let (memory, theme) = (self.memory, self.theme);
                let bars = crate::scrollbar::scrollbars(memory, theme, area, rect, size, now);
                self.redraw = self
                    .redraw
                    .earliest(memory.scrollbar_redraw(*id, spec, rect, size, now));
                self.out.extend(bars);
                self.out.push(Draw::PopLayer);
            }
            LaidKind::TextField {
                id,
                layout,
                view,
                geometry,
                field,
                ..
            } => self.text_field(*id, *layout, view, geometry, field, rect),
            LaidKind::Layer {
                opacity,
                clip,
                child,
            } => {
                let clip = match clip {
                    ClipSpec::Unclipped => Clip::Unclipped,
                    ClipSpec::Bounds => Clip::ToRect(rect),
                };
                self.out.push(Draw::PushLayer {
                    opacity: opacity.clamp(0.0, 1.0),
                    clip,
                });
                self.node(child, off);
                self.out.push(Draw::PopLayer);
            }
            LaidKind::World { shown, child, .. } => match shown {
                Shown::Culled => {}
                Shown::Visible => self.node(child, off),
            },
            // Painted by `portals`, above the whole normal tree and in layer order.
            LaidKind::Portal { .. } => {}
        }
    }

    /// Portals above the normal tree, bottom layer first (Popup, Modal, Toast, Tooltip; tree order within a layer). A
    /// modal dims everything painted before it with the theme scrim.
    fn portals(&mut self) {
        let screen = Rect::from_origin_size(Point::ZERO, self.laid.viewport.size);
        for portal in &Stacking::of(self.laid).portals {
            match portal.layer {
                Layer::Modal => self.out.push(Draw::Fill {
                    shape: Shape::Rect(screen),
                    brush: Brush::Solid(self.theme.colors.scrim),
                    transform: Affine::IDENTITY,
                }),
                Layer::Popup | Layer::Toast | Layer::Tooltip => {}
            }
            self.node(portal.child, Vec2::ZERO);
        }
    }

    fn all(&mut self, children: &[LaidNode<Msg>], off: Vec2) {
        for c in children {
            self.node(c, off);
        }
    }

    fn shape(&mut self, spec: &ShapeSpec, paint: &Paint, rect: Rect) {
        let (shape, transform) = match spec {
            ShapeSpec::Rect => (Shape::Rect(rect), Affine::IDENTITY),
            ShapeSpec::RoundRect(radii) => (
                Shape::RoundRect(RoundedRect::from_rect(rect, *radii)),
                Affine::IDENTITY,
            ),
            ShapeSpec::Ellipse => (Shape::Ellipse(Ellipse::from_rect(rect)), Affine::IDENTITY),
            ShapeSpec::Path { path, view_box } => {
                let fit = Affine::translate(rect.origin().to_vec2())
                    * Affine::scale_non_uniform(
                        rect.width() / view_box.width(),
                        rect.height() / view_box.height(),
                    )
                    * Affine::translate(-view_box.origin().to_vec2());
                (Shape::Path(path.clone()), fit)
            }
        };
        match paint {
            Paint::Fill(brush) => self.out.push(Draw::Fill {
                shape,
                brush: brush.clone(),
                transform,
            }),
            Paint::Stroke { brush, stroke } => {
                self.out.push(Draw::Stroke {
                    shape,
                    stroke: stroke.clone(),
                    brush: brush.clone(),
                    transform,
                });
            }
            Paint::FillStroke {
                fill,
                brush,
                stroke,
            } => {
                self.out.push(Draw::Fill {
                    shape: shape.clone(),
                    brush: fill.clone(),
                    transform,
                });
                self.out.push(Draw::Stroke {
                    shape,
                    stroke: stroke.clone(),
                    brush: brush.clone(),
                    transform,
                });
            }
        }
    }

    /// Eased 0..1 progress of an animation that started at `key`'s recorded time.
    fn progress(&mut self, id: kit_core::WidgetId, key: AnimKey) -> f32 {
        let Some(start) = self.memory.anim_start(id, key) else {
            return 1.0;
        };
        let total = self.theme.motion.fast();
        if total.is_zero() {
            return 1.0;
        }
        if self.now.since(start) < total {
            self.redraw = Redraw::Now;
        }
        let t = (self.now.since(start).as_secs_f32() / total.as_secs_f32()).clamp(0.0, 1.0);
        t * (2.0 - t) // ease-out quad
    }

    fn overlay(&mut self, rect: Rect, color: Color) {
        self.out.push(Draw::RoundRect {
            rect,
            radii: RoundedRectRadii::from_single_radius(self.theme.radii.control),
            fill: Brush::Solid(color),
            border: Border::None,
            shadow: Shadow::None,
        });
    }

    fn interaction_overlay(&mut self, id: kit_core::WidgetId, interactive: bool, rect: Rect) {
        if !interactive {
            return;
        }
        let colors = &self.theme.colors;
        if self.memory.is_pressed(id) {
            let (pressed, a) = (colors.pressed_overlay, self.progress(id, AnimKey::Press));
            self.overlay(rect, pressed.multiply_alpha(a));
        } else if self.memory.is_hovered(id) {
            let (hover, a) = (colors.hover_overlay, self.progress(id, AnimKey::Hover));
            self.overlay(rect, hover.multiply_alpha(a));
        }
    }

    /// Keyboard focus on a control (a field draws its own focused border): a ring just outside its box.
    fn focus_ring(&mut self, id: kit_core::WidgetId, rect: Rect) {
        if self.memory.focus() != Focus::On(id) {
            return;
        }
        const OUTSET: f64 = 2.0;
        let ring = RoundedRect::from_rect(
            rect.inflate(OUTSET, OUTSET),
            RoundedRectRadii::from_single_radius(self.theme.radii.control + OUTSET),
        );
        self.out.push(Draw::Stroke {
            shape: Shape::RoundRect(ring),
            stroke: kit_core::kurbo::Stroke::new(2.0),
            brush: Brush::Solid(self.theme.colors.focus_ring),
            transform: Affine::IDENTITY,
        });
    }

    /// A field: its box, then inside its text area (clipped, so a scrolled single line never spills): the selection
    /// behind the glyphs, the glyphs, the composition's underline, and the blinking caret.
    fn text_field(
        &mut self,
        id: kit_core::WidgetId,
        layout: TextLayout,
        view: &FieldView,
        geometry: &FieldGeometry,
        field: &FieldStyle,
        rect: Rect,
    ) {
        let focused = self.memory.focus() == Focus::On(id);
        let colors = self.theme.colors;
        self.out.push(Draw::RoundRect {
            rect,
            radii: RoundedRectRadii::from_single_radius(self.theme.radii.sm),
            fill: Brush::Solid(colors.field_fill),
            border: Border::Solid {
                width: if focused { 2.0 } else { 1.0 },
                color: if focused {
                    colors.focus_ring
                } else {
                    colors.field_border
                },
            },
            shadow: Shadow::None,
        });
        let origin = rect.origin() + field.inset;
        let ink = Brush::Solid(field.text.color);
        let placed = |r: Rect| r + origin.to_vec2();
        self.out.push(Draw::PushLayer {
            opacity: 1.0,
            clip: Clip::ToRect(rect.inset((-field.inset.x, 0.0))),
        });
        if focused {
            for r in geometry.selection_rects(view.selection.range()) {
                self.fill(placed(r), Brush::Solid(colors.selection));
            }
        }
        self.out.push(Draw::Glyphs {
            layout: layout.id,
            origin: origin - Vec2::new(f64::from(geometry.scroll_x()), 0.0),
            brush: ink.clone(),
        });
        for r in geometry.composition_rects() {
            let r = placed(r);
            self.fill(Rect::new(r.x0, r.y1 - 1.0, r.x1, r.y1), ink.clone());
        }
        if focused && self.caret_shown(id) {
            self.out.push(Draw::Caret {
                rect: placed(geometry.caret()),
                brush: ink,
                width: 1.5,
            });
        }
        self.out.push(Draw::PopLayer);
    }

    fn fill(&mut self, rect: Rect, brush: Brush) {
        self.out.push(Draw::Fill {
            shape: Shape::Rect(rect),
            brush,
            transform: Affine::IDENTITY,
        });
    }

    /// The caret blinks from its last edit or from gaining focus, and does not blink under reduced motion.
    fn caret_shown(&mut self, id: kit_core::WidgetId) -> bool {
        match self.theme.motion.pref {
            MotionPref::Reduced => true,
            MotionPref::Full => {
                let since = self
                    .memory
                    .anim_start(id, AnimKey::Caret)
                    .max(self.memory.anim_start(id, AnimKey::Focus))
                    .unwrap_or(self.now);
                let period = core::time::Duration::from_millis(CARET_BLINK_MS as u64);
                let elapsed = self.now.since(since);
                let next = Time(
                    self.now.0 + period
                        - core::time::Duration::from_nanos(
                            (elapsed.as_nanos() % period.as_nanos()) as u64,
                        ),
                );
                self.redraw = self.redraw.earliest(Redraw::At(next));
                (elapsed.as_millis() / CARET_BLINK_MS).is_multiple_of(2)
            }
        }
    }

    /// Dragged source re-painted under the pointer, above everything, translucent (parity with `HudBuilder::draw_drag`).
    fn drag_ghosts(&mut self) {
        for drag in self.memory.live_drags(self.laid.epoch) {
            let Some(source) = self.laid.find(drag.source) else {
                continue; // the source left the screen; nothing to ghost
            };
            self.out.push(Draw::PushLayer {
                opacity: GHOST_OPACITY,
                clip: Clip::Unclipped,
            });
            self.node(source, drag.at - source.rect.center());
            self.out.push(Draw::PopLayer);
        }
    }
}
