//! `LaidOut::assemble` rejects a wrong `Placed` count. TaffyLayout must emit exactly one `Placed` per node, in
//! pre-order, for every node kind and nesting (wrappers, scroll, culled and visible world boxes, empty containers).
mod common;

use common::*;
use kit_core::kurbo::{Insets, Size, Vec2};
use kit_core::{Axis, Identity, Viewport};
use kit_layout::{Reuse, TaffyLayout};
use kit_view::taffy::style_helpers::{auto, fr, length, line, span};
use kit_view::*;

struct Lcg(u64);
impl Lcg {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }
    fn dim(&mut self) -> taffy::Dimension {
        match self.next(4) {
            0 => auto(),
            1 => Percent(self.next(120) as f64).into(),
            _ => px(self.next(80) as f64),
        }
    }
    fn limit(&mut self) -> taffy::LengthPercentageAuto {
        match self.next(4) {
            0 => auto(),
            1 => Percent(self.next(120) as f64).into(),
            _ => px(self.next(80) as f64),
        }
    }
}

/// `in_grid`: the parent is a grid container, the only place a grid row/column is valid (it is rejected elsewhere
/// with `LayoutError::CellOutsideGrid`).
fn style(g: &mut Lcg, in_grid: bool) -> Style {
    let mut s = match g.next(4) {
        0 => Style::row(),
        1 => Style::column(),
        2 => Style::anchors(),
        _ => Style::grid(vec![length(30.0), fr(1.0)], vec![auto()]),
    };
    s.layout.size = taffy::Size {
        width: g.dim(),
        height: g.dim(),
    };
    s.layout.min_size.width = g.limit();
    s.layout.max_size.height = g.limit();
    s = s.padded(g.next(6) as f64).margined(g.next(6) as f64);
    if g.next(5) == 0 {
        s.layout.aspect_ratio = Some(1.0 + g.next(3) as f32);
    }
    s.placement = match (g.next(4), in_grid) {
        (0, _) => Placement::Anchored(Anchor {
            h: [AnchorH::Start, AnchorH::Center, AnchorH::End][g.next(3) as usize],
            v: [AnchorV::Start, AnchorV::Center, AnchorV::End][g.next(3) as usize],
            offset: Vec2::new(g.next(9) as f64 - 4.0, g.next(9) as f64 - 4.0),
            safe_area: if g.next(2) == 0 {
                SafeArea::Respect
            } else {
                SafeArea::Ignore
            },
        }),
        (1, true) => {
            s.layout.grid_column = grid_line(g, 4, 3);
            s.layout.grid_row = grid_line(g, 3, 2);
            s.placement
        }
        _ => s.placement,
    };
    s
}

/// 0 starts = auto-placed, 1..=lines = that line; spans 1..=max_span.
fn grid_line(g: &mut Lcg, lines: u64, max_span: u64) -> taffy::Line<taffy::GridPlacement> {
    let start = match g.next(lines) {
        0 => auto(),
        n => line(n as i16),
    };
    taffy::Line {
        start,
        end: span(1 + g.next(max_span) as u16),
    }
}

/// `salt` changes only what the shape does not depend on: text lengths. The same seed with another salt is the same
/// tree shape and the same styles with different leaf content, as a label that counts up is from frame to frame.
fn tree(g: &mut Lcg, depth: u32, in_grid: bool, salt: u64) -> Node<()> {
    let pick = if depth == 0 {
        7 + g.next(4)
    } else {
        g.next(11)
    };
    let st = style(g, in_grid);
    let grid_kids = matches!(
        (&st.content, st.layout.display),
        (Content::Taffy, taffy::Display::Grid)
    );
    // A wrapper's child is placed by the wrapper's parent; a scroll or world child is not placed by a grid.
    let child = |g: &mut Lcg| Box::new(tree(g, depth - 1, in_grid, salt));
    let detached = |g: &mut Lcg| Box::new(tree(g, depth - 1, false, salt));
    let kids = |g: &mut Lcg| {
        (0..g.next(4))
            .map(|_| tree(g, depth - 1, grid_kids, salt))
            .collect::<Vec<_>>()
    };
    let ts = TextStyle::new(10.0, kit_core::peniko::Color::from_rgba8(0, 0, 0, 255));
    match pick {
        0 | 1 => Node::Stack {
            style: st,
            children: kids(g),
        },
        2 => Node::Panel {
            style: st,
            surface: surface(),
            children: kids(g),
        },
        3 => Node::Interactive {
            identity: Identity::Positional,
            on: Bindings::none(),
            a11y: A11y::Hidden,
            tooltip: Tooltip::None,
            availability: Availability::Enabled,
            child: child(g),
        },
        4 => Node::Scroll {
            style: st,
            identity: Identity::Positional,
            spec: ScrollSpec::new(
                [Axis::Vertical, Axis::Horizontal, Axis::Both][g.next(3) as usize],
            ),
            child: detached(g),
        },
        5 => Node::Layer {
            opacity: 1.0,
            clip: ClipSpec::Bounds,
            child: child(g),
        },
        6 => Node::World {
            anchor: WorldAnchor {
                point: WorldPoint([g.next(200) as f64 - 40.0, g.next(200) as f64, 0.0]),
                offset: Vec2::new(0.0, -5.0),
            },
            child: detached(g),
        },
        7 => Node::Text {
            style: st,
            text: RichText::plain("w".repeat(((g.next(30) + salt) % 30) as usize)),
            text_style: ts.clone().wrap(TextWrap::WordOrGlyph),
        },
        8 => Node::TextField {
            style: st,
            identity: Identity::Positional,
            label: "field".into(),
            view: EditorState::single_line("abc").view(),
            on_edit: std::rc::Rc::new(|_| ()),
            field: FieldStyle {
                text: ts,
                masking: Masking::Bullets,
                inset: Vec2::new(4.0, 2.0),
            },
            claim: kit_view::FocusClaim::Nothing,
        },
        9 => Node::Shape {
            style: st,
            shape: kit_core::ShapeSpec::Rect,
            paint: Paint::Fill(white()),
        },
        _ => Node::Stack {
            style: st,
            children: Vec::new(),
        },
    }
}

fn surface() -> kit_core::Surface {
    kit_core::Surface {
        fill: white(),
        radii: Default::default(),
        border: kit_core::Border::None,
        shadow: kit_core::Shadow::None,
    }
}

fn finite(r: kit_core::kurbo::Rect) -> bool {
    [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite())
}

#[test]
fn always_emit_one_placed_per_node_in_a_consistent_order() {
    let mut vp = Viewport::new(Size::new(320.0, 480.0), 2.0);
    vp.safe_area = Insets::new(0.0, 20.0, 0.0, 10.0);
    for seed in 0..400 {
        let n = tree(&mut Lcg(seed), 4, false, 0).count();
        let root = tree(&mut Lcg(seed), 4, false, 0);
        let laid = TaffyLayout::new()
            .layout(root, vp, &mut text_system(), &Plane)
            .unwrap_or_else(|e| panic!("seed {seed}: {e:?}"));
        let all = pre_order(&laid);
        assert_eq!(all.len(), n, "seed {seed}");
        assert!(all.iter().all(|(r, _)| finite(*r)), "seed {seed}");
    }
}

/// The layout keeps its taffy tree between frames. Whatever it keeps, a frame must come out exactly as if the layout
/// had never seen the previous one: same tree again, same shape with other text, another viewport, another shape.
#[test]
fn a_kept_layout_agrees_with_a_fresh_one_after_every_kind_of_change() {
    let mut fonts = text_system();
    let mut kept = TaffyLayout::new();
    let (mut rebuilt, mut patched) = (0, 0);
    for seed in 0..300 {
        // (salt, viewport width, safe-area top): identical, text only, viewport, both; then the next seed reshapes.
        for (salt, width, top) in [
            (0, 320.0, 20.0),
            (0, 320.0, 20.0),
            (7, 320.0, 20.0),
            (7, 200.0, 20.0),
            (11, 200.0, 0.0),
        ] {
            let mut vp = Viewport::new(Size::new(width, 480.0), 2.0);
            vp.safe_area = Insets::new(0.0, top, 0.0, 10.0);
            let tree = || tree(&mut Lcg(seed), 4, false, salt);
            let fresh = TaffyLayout::new()
                .layout(tree(), vp, &mut fonts, &Plane)
                .unwrap_or_else(|e| panic!("seed {seed}: {e:?}"));
            let again = kept
                .layout(tree(), vp, &mut fonts, &Plane)
                .unwrap_or_else(|e| panic!("seed {seed}: {e:?}"));
            assert_eq!(
                pre_order(&again),
                pre_order(&fresh),
                "seed {seed} salt {salt} width {width}"
            );
            match kept.last_reuse() {
                Reuse::Rebuilt { .. } => rebuilt += 1,
                Reuse::Patched { .. } => patched += 1,
            }
        }
    }
    // Both paths were exercised: trees that were patched in place and trees that were rebuilt.
    assert!(
        rebuilt > 0 && patched > 0,
        "rebuilt {rebuilt} patched {patched}"
    );
}
