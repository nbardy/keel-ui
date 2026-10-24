//! Fixture pictures, built through the real pipeline: `kit_view` builders -> `LaidOut` -> `kit_paint::paint`, with
//! the bundled Inter (`kit_view::testing::text_system`) as the font. A [`Fixture`] is the backend-neutral result (draw list + text + backdrop): `kit_raster`
//! turns it into the golden PNGs and `kit_wgpu`'s parity test draws the very same fixtures on the GPU. Pointer-dependent state (hover, focus, scroll, drag) comes from routing
//! real `InputEvent`s through `kit_input::Router`, not from poking `UiMemory`.
//!
//! Rects are supplied explicitly in pre-order (there is no layout engine in this crate), via the small `Built`
//! combinators below. These fixtures call `LaidOut::assemble` directly (like `kit_view::testing::laid_out`) because
//! they keep the text system for drawing.
use crate::RasterUi;
use kit_core::kurbo::{BezPath, Point, Rect, RoundedRectRadii, Size, Stroke, Vec2};
use kit_core::peniko::{Brush, Color};
use kit_core::{
    Axis, Border, ColorVision, Epoch, MotionPref, Shadow, ShapeSpec, Surface, Theme, Time,
    Viewport, WidgetId,
};
use kit_input::{
    DeltaUnit, InputEvent, Phase, PointerEvent, PointerId, PointerKind, Router, UiMemory,
};
use kit_paint::{DrawList, paint};
use kit_view::testing::text_system;
use kit_view::{
    A11y, Bindings, CosmicText, DragPayload, EditorState, LaidOut, Node, Paint, Placed, Role,
    Shown, Style, TextStyle, Toggle, column, interactive, layer, panel, rect, rounded, scroll,
    shape, stack, text, text_field,
};
use tiny_skia::Pixmap;

type Msg = ();

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Scene {
    HudBar,
    NestedLayers,
    ClippedScroll,
    DragGhost,
    Images,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct UnknownScene(pub String);

impl Scene {
    pub const ALL: [Scene; 5] = [
        Scene::HudBar,
        Scene::NestedLayers,
        Scene::ClippedScroll,
        Scene::DragGhost,
        Scene::Images,
    ];
    /// Also the golden file stem.
    pub fn name(self) -> &'static str {
        match self {
            Scene::HudBar => "hud_bar",
            Scene::NestedLayers => "nested_layers",
            Scene::ClippedScroll => "clipped_scroll",
            Scene::DragGhost => "drag_ghost",
            Scene::Images => "images",
        }
    }
    pub fn from_name(name: &str) -> Result<Scene, UnknownScene> {
        Scene::ALL
            .into_iter()
            .find(|s| s.name() == name)
            .ok_or_else(|| UnknownScene(name.to_owned()))
    }
    pub fn fixture(self) -> Fixture {
        match self {
            Scene::HudBar => hud_bar(),
            Scene::NestedLayers => nested_layers(),
            Scene::ClippedScroll => clipped_scroll(),
            Scene::DragGhost => drag_ghost(),
            Scene::Images => images(),
        }
    }
    pub fn render(self) -> Pixmap {
        self.fixture().render()
    }
}

/// A scene as data: draw it with `RasterUi` or the GPU renderer.
pub struct Fixture {
    pub list: DrawList,
    pub pixels: Vec<kit_view::ImagePixels>,
    pub text: CosmicText,
    /// What the scene is drawn over.
    pub backdrop: Color,
    /// Device px (the scale factor is 1).
    pub size: (u32, u32),
}

impl Fixture {
    /// The CPU picture.
    pub fn render(&mut self) -> Pixmap {
        let mut ui = RasterUi::new();
        for pixels in &self.pixels {
            ui.register_pixels(pixels).expect("fixture pixels");
        }
        let mut pixmap = Pixmap::new(self.size.0, self.size.1).expect("scene size is non-zero");
        pixmap.fill(crate::convert::color(self.backdrop));
        ui.render(&self.list, &mut self.text, &mut pixmap)
            .expect("scene draw list renders");
        pixmap
    }
}

/// Render a fixture by name.
pub fn scene(name: &str) -> Result<Pixmap, UnknownScene> {
    Scene::from_name(name).map(Scene::render)
}

// ---- tree building -------------------------------------------------------------------------------------------

/// A subtree plus the pre-order rects of its nodes.
struct Built {
    node: Node<Msg>,
    rects: Vec<Rect>,
}
fn leaf(rect: Rect, node: Node<Msg>) -> Built {
    Built {
        node,
        rects: vec![rect],
    }
}
fn parent(rect: Rect, kids: Vec<Built>, make: impl FnOnce(Vec<Node<Msg>>) -> Node<Msg>) -> Built {
    let mut rects = vec![rect];
    let mut nodes = Vec::with_capacity(kids.len());
    for kid in kids {
        rects.extend(kid.rects);
        nodes.push(kid.node);
    }
    Built {
        node: make(nodes),
        rects,
    }
}
fn wrap(rect: Rect, child: Built, make: impl FnOnce(Node<Msg>) -> Node<Msg>) -> Built {
    let mut rects = vec![rect];
    rects.extend(child.rects);
    Built {
        node: make(child.node),
        rects,
    }
}
fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::new(x, y, x + w, y + h)
}
fn solid(c: Color) -> Brush {
    Brush::Solid(c)
}
fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}
fn radius(v: f64) -> RoundedRectRadii {
    RoundedRectRadii::from_single_radius(v)
}
fn theme() -> Theme {
    Theme::new(ColorVision::Normal, MotionPref::Full)
}
fn surface(fill: Color, radii: f64, border: Border, shadow: Shadow) -> Surface {
    Surface {
        fill: solid(fill),
        radii: radius(radii),
        border,
        shadow,
    }
}
fn labelled(role: Role, label: &str) -> A11y {
    A11y::Labeled {
        role,
        label: label.to_owned(),
        toggle: Toggle::NotToggle,
    }
}
fn label(at: Rect, s: &str, size: f32, color: Color) -> Built {
    leaf(at, text(s, TextStyle::new(size, color)))
}

struct Laid {
    laid: LaidOut<Msg>,
    text: CosmicText,
}
fn lay(tree: Built, size: Size) -> Laid {
    let mut text = text_system();
    let placed: Vec<Placed> = tree
        .rects
        .iter()
        .map(|&rect| Placed {
            rect,
            shown: Shown::Visible,
        })
        .collect();
    let laid = LaidOut::assemble(tree.node, Viewport::new(size, 1.0), &placed, &mut text)
        .expect("scene rects match node count");
    Laid { laid, text }
}

fn pointer(l: &Laid, mem: &mut UiMemory, id: u64, phase: Phase, at: Point) {
    let event = PointerEvent {
        id: PointerId(id),
        kind: PointerKind::Mouse,
        phase,
        at,
        pressure: 1.0,
    };
    Router::default().route(&l.laid, mem, InputEvent::Pointer(event), Time::default());
}

/// Paint `l` with the default painter (`edit` may append hand-built items the painter cannot emit).
fn picture(l: Laid, mem: &UiMemory, edit: impl FnOnce(&mut DrawList)) -> Fixture {
    let th = theme();
    let mut list = paint(&l.laid, mem, &th, Time::default());
    edit(&mut list);
    let size = l.laid.viewport.size;
    Fixture {
        pixels: Vec::new(),
        list,
        text: l.text,
        backdrop: th.colors.surface_inset,
        size: (size.width as u32, size.height as u32),
    }
}

fn memory(size: Size) -> UiMemory {
    UiMemory::new(Viewport::new(size, 1.0))
}

// ---- scenes --------------------------------------------------------------------------------------------------

fn star() -> BezPath {
    let mut p = BezPath::new();
    for i in 0..10_u32 {
        let angle = -std::f64::consts::FRAC_PI_2 + f64::from(i) * std::f64::consts::PI / 5.0;
        let rad = if i.is_multiple_of(2) { 11.0 } else { 4.6 };
        let at = Point::new(12.0 + rad * angle.cos(), 12.0 + rad * angle.sin());
        match i {
            0 => p.move_to(at),
            _ => p.line_to(at),
        }
    }
    p.close_path();
    p
}

/// Panel with health bar, vector icon, gold counter and hovered button (row 1); dashed cooldown ring,
/// slots (one disabled by a 40% layer) and a focused text field with caret (row 2).
fn hud_bar() -> Fixture {
    let c = theme().colors;
    let size = Size::new(360.0, 128.0);
    let text_color = c.text;

    let button = wrap(
        r(300.0, 16.0, 44.0, 32.0),
        parent(
            r(300.0, 16.0, 44.0, 32.0),
            vec![label(r(310.0, 24.0, 24.0, 16.0), "GO", 13.0, text_color)],
            |k| {
                panel(
                    Style::column(),
                    surface(c.accent, 6.0, Border::None, Shadow::None),
                    k,
                )
            },
        ),
        |n| {
            interactive(
                WidgetId::key("go"),
                Bindings::activate(()),
                labelled(Role::Button, "Go"),
                n,
            )
        },
    );
    let hud = parent(
        r(8.0, 8.0, 344.0, 48.0),
        vec![
            label(r(20.0, 22.0, 20.0, 18.0), "HP", 14.0, text_color),
            leaf(
                r(44.0, 24.0, 96.0, 12.0),
                rounded(Style::column(), radius(6.0), solid(c.surface_inset)),
            ),
            leaf(
                r(44.0, 24.0, 64.0, 12.0),
                rounded(Style::column(), radius(6.0), solid(c.success)),
            ),
            label(r(148.0, 22.0, 50.0, 18.0), "64/96", 14.0, text_color),
            leaf(
                r(206.0, 18.0, 24.0, 24.0),
                shape(
                    Style::column(),
                    ShapeSpec::Path {
                        path: star(),
                        view_box: r(0.0, 0.0, 24.0, 24.0),
                    },
                    Paint::FillStroke {
                        fill: solid(c.warning),
                        brush: solid(text_color),
                        stroke: Stroke::new(1.5),
                    },
                ),
            ),
            label(r(238.0, 22.0, 56.0, 18.0), "1250 g", 14.0, text_color),
            button,
        ],
        |k| {
            panel(
                Style::row(),
                surface(
                    c.surface_raised,
                    8.0,
                    Border::Solid {
                        width: 1.0,
                        color: c.field_border,
                    },
                    Shadow::Drop {
                        offset: Vec2::new(0.0, 3.0),
                        blur: 8.0,
                        spread: 0.0,
                        color: Color::from_rgba8(0, 0, 0, 150),
                    },
                ),
                k,
            )
        },
    );
    let ring = leaf(
        r(16.0, 72.0, 44.0, 44.0),
        shape(
            Style::column(),
            ShapeSpec::Ellipse,
            Paint::Stroke {
                brush: solid(c.warning),
                stroke: Stroke::new(3.0).with_dashes(0.0, [7.0, 5.0]),
            },
        ),
    );
    let core = leaf(
        r(26.0, 82.0, 24.0, 24.0),
        shape(
            Style::column(),
            ShapeSpec::Ellipse,
            Paint::Fill(solid(c.friend.multiply_alpha(0.7))),
        ),
    );
    let slot = |x: f64, tint: Color, n: &str| {
        parent(
            r(x, 72.0, 44.0, 44.0),
            vec![
                leaf(
                    r(x + 8.0, 80.0, 28.0, 28.0),
                    rounded(Style::column(), radius(5.0), solid(tint)),
                ),
                label(r(x + 16.0, 86.0, 12.0, 16.0), n, 14.0, rgb(255, 255, 255)),
            ],
            |k| {
                panel(
                    Style::column(),
                    surface(
                        c.surface_raised,
                        6.0,
                        Border::Solid {
                            width: 2.0,
                            color: c.accent,
                        },
                        Shadow::None,
                    ),
                    k,
                )
            },
        )
    };
    let dimmed = wrap(r(180.0, 72.0, 44.0, 44.0), slot(180.0, c.foe, "3"), |n| {
        layer(0.4, n)
    });
    let field = leaf(
        r(236.0, 76.0, 116.0, 36.0),
        text_field(
            WidgetId::key("name"),
            "Name",
            &EditorState::single_line("keel"),
            TextStyle::new(14.0, c.text),
            |_| (),
        ),
    );
    let root = parent(
        r(0.0, 0.0, 360.0, 128.0),
        vec![
            hud,
            ring,
            core,
            slot(76.0, c.friend, "1"),
            slot(128.0, c.warning, "2"),
            dimmed,
            field,
        ],
        column,
    );

    let l = lay(root, size);
    let mut mem = memory(size);
    pointer(&l, &mut mem, 0, Phase::Move, Point::new(322.0, 32.0)); // hover the button
    pointer(&l, &mut mem, 1, Phase::Down, Point::new(294.0, 94.0)); // focus the field (caret)
    pointer(&l, &mut mem, 1, Phase::Up, Point::new(294.0, 94.0));
    picture(l, &mem, |_| {})
}

/// Left: one 50% group of two overlapping squares (overlap is NOT double-composited). Middle: two separate 50% layers
/// (overlap shows through). Bottom: 50% inside 50% = 25% bar.
fn nested_layers() -> Fixture {
    let size = Size::new(320.0, 176.0);
    let (red, blue) = (rgb(220, 60, 60), rgb(60, 110, 230));
    let square = |at: Rect, c: Color| leaf(at, rect(Style::column(), solid(c)));
    let group = wrap(
        r(10.0, 10.0, 120.0, 120.0),
        parent(
            r(10.0, 10.0, 120.0, 120.0),
            vec![
                square(r(10.0, 10.0, 70.0, 70.0), red),
                square(r(60.0, 60.0, 70.0, 70.0), blue),
            ],
            |k| stack(Style::column(), k),
        ),
        |n| layer(0.5, n),
    );
    let separate = parent(
        r(150.0, 10.0, 120.0, 120.0),
        vec![
            wrap(
                r(150.0, 10.0, 70.0, 70.0),
                square(r(150.0, 10.0, 70.0, 70.0), red),
                |n| layer(0.5, n),
            ),
            wrap(
                r(200.0, 60.0, 70.0, 70.0),
                square(r(200.0, 60.0, 70.0, 70.0), blue),
                |n| layer(0.5, n),
            ),
        ],
        |k| stack(Style::column(), k),
    );
    let bar_rect = r(10.0, 140.0, 300.0, 24.0);
    let bar = wrap(
        bar_rect,
        wrap(bar_rect, square(bar_rect, rgb(255, 255, 255)), |n| {
            layer(0.5, n)
        }),
        |n| layer(0.5, n),
    );
    let root = parent(
        r(0.0, 0.0, 320.0, 176.0),
        vec![group, separate, bar],
        column,
    );
    let l = lay(root, size);
    picture(l, &memory(size), |_| {})
}

/// A scroll area scrolled by 40px inside a bordered frame: rows are cut cleanly at the top and bottom edges.
fn clipped_scroll() -> Fixture {
    let c = theme().colors;
    let size = Size::new(240.0, 180.0);
    let row = |i: u32| {
        let y = 16.0 + f64::from(i) * 34.0;
        let tint = if i.is_multiple_of(2) { c.friend } else { c.foe };
        let fill = if i.is_multiple_of(2) {
            c.surface_raised
        } else {
            c.surface
        };
        parent(
            r(20.0, y, 200.0, 30.0),
            vec![
                label(
                    r(28.0, y + 7.0, 60.0, 16.0),
                    &format!("Item {}", i + 1),
                    14.0,
                    c.text,
                ),
                leaf(
                    r(188.0, y + 7.0, 24.0, 16.0),
                    rounded(Style::column(), radius(4.0), solid(tint)),
                ),
            ],
            |k| {
                panel(
                    Style::row(),
                    surface(fill, 4.0, Border::None, Shadow::None),
                    k,
                )
            },
        )
    };
    let content = parent(
        r(20.0, 16.0, 200.0, 272.0),
        (0..8).map(row).collect(),
        column,
    );
    let area = wrap(r(20.0, 16.0, 200.0, 148.0), content, |n| {
        scroll(
            WidgetId::key("list"),
            Axis::Vertical,
            Style::column().sized(200.0, 148.0),
            n,
        )
    });
    let frame = parent(r(8.0, 8.0, 224.0, 164.0), vec![area], |k| {
        panel(
            Style::column(),
            surface(
                c.surface,
                8.0,
                Border::Solid {
                    width: 2.0,
                    color: c.accent,
                },
                Shadow::None,
            ),
            k,
        )
    });
    let l = lay(parent(r(0.0, 0.0, 240.0, 180.0), vec![frame], column), size);
    let mut mem = memory(size);
    Router::default().route(
        &l.laid,
        &mut mem,
        InputEvent::Wheel {
            at: Point::new(120.0, 90.0),
            delta: Vec2::new(0.0, 40.0),
            unit: DeltaUnit::Pixels,
        },
        Time::default(),
    );
    picture(l, &mem, |_| {})
}

/// Four slots; slot 2 is mid-drag, so its translucent copy follows the pointer above everything (and over slots 3, 4).
fn drag_ghost() -> Fixture {
    let c = theme().colors;
    let size = Size::new(280.0, 120.0);
    let slot = |i: u32, tint: Color| {
        let x = 16.0 + f64::from(i) * 64.0;
        wrap(
            r(x, 16.0, 56.0, 56.0),
            parent(
                r(x, 16.0, 56.0, 56.0),
                vec![
                    leaf(
                        r(x + 10.0, 26.0, 36.0, 36.0),
                        rounded(Style::column(), radius(18.0), solid(tint)),
                    ),
                    label(
                        r(x + 24.0, 36.0, 12.0, 16.0),
                        &(i + 1).to_string(),
                        14.0,
                        rgb(255, 255, 255),
                    ),
                ],
                |k| {
                    panel(
                        Style::column(),
                        surface(
                            c.surface_raised,
                            6.0,
                            Border::Solid {
                                width: 2.0,
                                color: c.field_border,
                            },
                            Shadow::None,
                        ),
                        k,
                    )
                },
            ),
            move |n| {
                interactive(
                    WidgetId::key("slot").index(u64::from(i)),
                    Bindings::none().with_drag(DragPayload {
                        kind: 1,
                        value: u64::from(i),
                    }),
                    A11y::Hidden,
                    n,
                )
            },
        )
    };
    let slots = (0..4)
        .map(|i| slot(i, [c.friend, c.warning, c.foe, c.accent][i as usize]))
        .collect();
    let mut l = lay(parent(r(0.0, 0.0, 280.0, 120.0), slots, column), size);
    l.laid = l.laid.with_epoch(Epoch([1, 0]));
    let mut mem = memory(size);
    pointer(&l, &mut mem, 0, Phase::Down, Point::new(108.0, 44.0));
    pointer(&l, &mut mem, 0, Phase::Move, Point::new(200.0, 84.0));
    picture(l, &mem, |_| {})
}

/// Pixel art, bilinear scaling, containment, tint, rounded corners, clipping and overlapping group opacity.
fn images() -> Fixture {
    use kit_view::{Filter, ImagePixels, ImageRegion, ImageStyle, image};
    let pixels = ImagePixels::new(
        4,
        2,
        vec![
            240, 60, 20, 255, 20, 220, 90, 255, 30, 80, 240, 128, 240, 220, 30, 255, 80, 20, 180,
            255, 240, 120, 30, 128, 40, 210, 220, 255, 250, 250, 250, 0,
        ],
    )
    .expect("pixels");
    let handle = pixels.image();
    let size = Size::new(320.0, 180.0);
    let nearest = leaf(
        r(10.0, 10.0, 140.0, 70.0),
        image(handle, ImageStyle::stretch().filter(Filter::Nearest)),
    );
    let smooth = leaf(
        r(170.0, 10.0, 140.0, 70.0),
        image(handle, ImageStyle::contain().rounded(12.0)),
    );
    let overlap = parent(
        r(0.0, 0.0, 320.0, 180.0),
        vec![
            leaf(
                r(10.0, 100.0, 150.0, 60.0),
                image(
                    handle,
                    ImageStyle::contain().tint(Color::from_rgba8(150, 210, 255, 180)),
                ),
            ),
            leaf(
                r(80.0, 120.0, 90.0, 40.0),
                image(handle, ImageStyle::stretch().filter(Filter::Nearest)),
            ),
        ],
        column,
    );
    let clipped = wrap(r(20.0, 95.0, 120.0, 60.0), overlap, |n| Node::Layer {
        opacity: 0.6,
        clip: kit_view::ClipSpec::Bounds,
        child: Box::new(n),
    });
    let contained = leaf(
        r(200.0, 95.0, 90.0, 75.0),
        image(handle, ImageStyle::contain().filter(Filter::Nearest)),
    );
    // Sprite-sheet regions: a stretched sub-rectangle and a contained one with rounded corners.
    let sprite = leaf(
        r(10.0, 160.0, 120.0, 16.0),
        image(
            handle,
            ImageStyle::stretch().region(ImageRegion::new(1, 0, 2, 2).expect("region")),
        ),
    );
    let sprite_contain = leaf(
        r(150.0, 150.0, 50.0, 25.0),
        image(
            handle,
            ImageStyle::contain()
                .region(ImageRegion::new(2, 0, 2, 2).expect("region"))
                .rounded(4.0),
        ),
    );
    let root = parent(
        r(0.0, 0.0, 320.0, 180.0),
        vec![nearest, smooth, clipped, contained, sprite, sprite_contain],
        column,
    );
    let mut fx = picture(lay(root, size), &memory(size), |_| {});
    fx.pixels.push(pixels);
    fx
}
