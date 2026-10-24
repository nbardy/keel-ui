mod common;

use common::*;
use kit_core::kurbo::{Insets, Rect, Size, Vec2};
use kit_core::{Axis, Viewport, WidgetId};
use kit_layout::{Reuse, TaffyLayout};
use kit_view::taffy::{FlexDirection, FlexWrap};
use kit_view::*;

fn viewport(w: f64, h: f64) -> Viewport {
    Viewport::new(Size::new(w, h), 1.0)
}

fn taffy(root: Node<()>, vp: Viewport) -> LaidOut<()> {
    TaffyLayout::new()
        .layout(root, vp, &mut text_system(), &Plane)
        .expect("pre-order count matches")
}

/// Kit policy (`Style::row`/`column` set `align_content: FLEX_START`): wrapped lines pack at the start instead of
/// spreading over the container's free cross space (taffy's default).
#[test]
fn wrapped_flex_lines_pack_at_the_start() {
    let kids: Vec<Node<()>> = (0..6).map(|_| block(40.0, 20.0)).collect();
    let root = stack(flex(FlexDirection::Row, FlexWrap::Wrap, 10.0), kids);
    let all = pre_order(&taffy(root, viewport(100.0, 200.0)));
    // 40 + 10 + 40 fits in 100, a third does not: two per row, rows 20 + 10 apart from the top of a 200 px box.
    assert_eq!(
        (all[2].0.x0, all[3].0.x0, all[3].0.y0, all[5].0.y0),
        (50.0, 0.0, 30.0, 60.0)
    );
}

/// Percent trap: kit percents are 0..=100, taffy's are 0..=1. A `Percent` that skipped its conversion (or a raw taffy
/// `percent(25.0)`) makes this child 100x too wide; a parent left shrink-wrapped makes it 0 px.
#[test]
fn a_percent_width_child_of_a_fill_width_parent_gets_its_share_in_px() {
    let bar = rect(Style::column().sized(1.0, 12.0), white()).width(Percent(25.0));
    let holder = column(vec![bar]).fill_width();
    let root = column(vec![holder]).align_items(taffy::AlignItems::FLEX_START);
    let all = pre_order(&taffy(root, viewport(400.0, 100.0)));
    assert_eq!((all[1].0.width(), all[2].0.width()), (400.0, 100.0));
}

fn text_node(chars: usize) -> Node<()> {
    let style = TextStyle::new(10.0, kit_core::peniko::Color::from_rgba8(0, 0, 0, 255))
        .wrap(TextWrap::WordOrGlyph);
    text("x".repeat(chars), style)
}

/// Taffy measures text at the width it offers; `LaidOut::assemble` lays it out again at the final box width. The two
/// must agree, or the drawn text is taller than its box (or wraps differently from what was measured).
#[test]
fn text_height_grows_as_width_shrinks_and_the_final_layout_fits_its_box() {
    let heights: Vec<f64> = [300.0, 50.0, 20.0]
        .iter()
        .map(|w| {
            let laid = TaffyLayout::new()
                .layout(
                    column(vec![text_node(20)]),
                    viewport(*w, 400.0),
                    &mut text_system(),
                    &Plane,
                )
                .unwrap();
            let node = &laid.root.children()[0];
            let LaidKind::Text { layout, .. } = &node.kind else {
                panic!("text")
            };
            assert!(near(layout.size.height, node.rect.height()), "{w}");
            node.rect.height()
        })
        .collect();
    // One 12.5 px line (10 px * 1.25) when it fits, then more lines as the box narrows.
    assert!(
        near(heights[0], 12.5) && heights[0] < heights[1] && heights[1] < heights[2],
        "{heights:?}"
    );
}

#[test]
fn text_in_a_row_shrinks_and_wraps_instead_of_overflowing() {
    // Max-content is 100px but the row only offers 60: the flex item must shrink to the offer and re-measure taller.
    let laid = taffy(row(vec![text_node(20)]), viewport(60.0, 400.0));
    let r = laid.root.children()[0].rect;
    assert!(r.width() <= 60.0 + 1e-6, "{r:?}");
    assert!(r.height() > 12.5, "{r:?}");
}

fn safe_viewport() -> Viewport {
    let mut vp = viewport(400.0, 800.0);
    vp.safe_area = Insets::new(0.0, 47.0, 0.0, 34.0);
    vp
}

#[test]
fn safe_area_lifts_bottom_bar_above_home_indicator() {
    let bar = |safe| {
        let mut s =
            Style::column()
                .sized(400.0, 50.0)
                .anchored(AnchorH::Start, AnchorV::End, Vec2::ZERO);
        if let Placement::Anchored(a) = &mut s.placement {
            a.safe_area = safe;
        }
        rect(s, white())
    };
    let respect = taffy_rects(anchors(vec![bar(SafeArea::Respect)]), safe_viewport());
    let ignore = taffy_rects(anchors(vec![bar(SafeArea::Ignore)]), safe_viewport());
    assert_eq!(respect[1].y1, 800.0 - 34.0);
    assert_eq!(ignore[1].y1, 800.0);
}

#[test]
fn safe_area_pads_flow_content_at_the_root() {
    let laid = taffy(column(vec![block(100.0, 10.0)]), safe_viewport());
    assert_eq!(
        laid.root.rect.height(),
        800.0,
        "root still fills the viewport"
    );
    assert_eq!(laid.root.children()[0].rect.y0, 47.0);
}

fn taffy_rects(root: Node<()>, vp: Viewport) -> Vec<Rect> {
    pre_order(&taffy(root, vp))
        .into_iter()
        .map(|(r, _)| r)
        .collect()
}
fn world_label(x: f64, y: f64) -> Node<()> {
    world(
        WorldAnchor {
            point: WorldPoint([x, y, 0.0]),
            offset: Vec2::new(0.0, -10.0),
        },
        column(vec![block(20.0, 10.0), block(20.0, 10.0)]),
    )
}

#[test]
fn culled_world_anchor_culls_its_whole_subtree() {
    let tree = anchors(vec![world_label(-5.0, 50.0), world_label(100.0, 100.0)]);
    let laid = taffy(tree, viewport(400.0, 400.0));
    let all = pre_order(&laid);
    // root, [world, stack, 2 blocks] culled, [world, stack, 2 blocks] visible
    assert_eq!(all.len(), 1 + 4 + 4);
    assert!(all[1..5].iter().all(|(r, _)| *r == Rect::ZERO));
    assert_eq!(all[1].1, Some(Shown::Culled));
    let LaidKind::World { child, .. } = &laid.root.children()[0].kind else {
        panic!("world")
    };
    assert!(matches!(child.kind, LaidKind::Stack { .. }));
    assert_eq!(all[5].1, Some(Shown::Visible));
    // centred on the projected point plus the offset
    let visible = all[6].0;
    assert!(
        near(visible.center().x, 100.0) && near(visible.center().y, 90.0),
        "{visible:?}"
    );
    assert_eq!(all[5].0, visible, "World node shares its child's rect");
}

#[test]
fn scroll_child_is_not_clamped_by_the_scroll_viewport() {
    let content = |dir| {
        let kids: Vec<Node<()>> = (0..5).map(|_| block(100.0, 50.0)).collect();
        stack(flex(dir, FlexWrap::NoWrap, 0.0), kids)
    };
    let vertical = scroll(
        WidgetId::key("v"),
        Axis::Vertical,
        Style::column().sized(100.0, 100.0),
        content(FlexDirection::Column),
    );
    let horizontal = scroll(
        WidgetId::key("h"),
        Axis::Horizontal,
        Style::column().sized(100.0, 100.0),
        content(FlexDirection::Row),
    );
    let laid = taffy(column(vec![vertical, horizontal]), viewport(400.0, 600.0));
    let all = pre_order(&laid);
    // root, scroll v, content v, 5 blocks, scroll h, content h, 5 blocks
    let (sv, cv, sh, ch) = (all[1].0, all[2].0, all[8].0, all[9].0);
    assert_eq!(
        (sv.width(), sv.height()),
        (100.0, 100.0),
        "viewport keeps its style size"
    );
    assert_eq!(cv.height(), 250.0);
    assert_eq!((sh.width(), sh.height()), (100.0, 100.0));
    assert_eq!(ch.width(), 500.0);
    // The blocks keep their size instead of shrinking into the viewport.
    assert!(all[3..8].iter().all(|(r, _)| r.height() == 50.0));
}

fn cell(column: taffy::Line<taffy::GridPlacement>) -> Style {
    let mut s = tall(20.0);
    s.layout.grid_column = column;
    s
}

/// A grid row/column outside a grid is silently ignored by taffy; the kit rejects the tree instead.
#[test]
fn a_grid_cell_outside_a_grid_is_a_typed_error() {
    let tree = || -> Node<()> { column(vec![rect(cell(taffy::style_helpers::line(2)), white())]) };
    let vp = viewport(100.0, 100.0);
    let result = TaffyLayout::new().layout(tree(), vp, &mut text_system(), &Plane);
    assert!(matches!(
        result,
        Err(LayoutError::CellOutsideGrid { node: 1 })
    ));
}

/// Regression guard for list layout cost. Taffy asks a leaf for its size many times per solve (flex passes, every
/// ancestor row and column). Each ask through `CosmicText::layout` builds and hashes a full cache key even on a hit;
/// the bench list (rows of three text leaves) made 46 calls per row per frame and spent ~24 us/row there. Layout must
/// remember each leaf's answers so the text system is asked about the same room once.
#[test]
fn text_system_is_asked_a_bounded_number_of_times_per_text_leaf() {
    let style = TextStyle::new(13.0, kit_core::peniko::Color::WHITE);

    let rows = 50;
    let root = column::<()>((0..rows).map(|i| {
        row([
            text(format!("Item {i}"), style.clone()),
            row([text("On", style.clone())]),
            row([row([text("Buy", style.clone())])]),
        ])
    }));
    let mut fonts = text_system();
    let before = fonts.layout_calls();
    TaffyLayout::new()
        .layout(
            root,
            Viewport::new(Size::new(800.0, 600.0), 1.0),
            &mut fonts,
            &NoWorld,
        )
        .unwrap();
    let per_leaf = (fonts.layout_calls() - before) as f64 / (3 * rows) as f64;
    assert!(per_leaf <= 2.0, "{per_leaf} layout calls per text leaf");
}

/// Regression guard for list layout cost: a frame that asks for the tree the last frame left must not hand taffy
/// anything. The tree used to be rebuilt and re-solved from scratch every frame (about 10 us per row at 100 rows, 70%
/// of it taffy's solver). Counted, not timed: `Reuse` says what happened to the kept tree, and the text system is
/// not asked at all: no measure ran, and `assemble` remembers each text leaf's layout.
#[test]
fn an_unchanged_frame_costs_taffy_nothing_and_an_edited_label_costs_one_leaf() {
    let style = TextStyle::new(13.0, kit_core::peniko::Color::WHITE);
    // One row is seven laid nodes: row, label, row, "On", row, row, "Buy".
    let nodes_per_row = 7;
    let rows = 50;
    let list = |rows: usize, first_label: &str| {
        column::<()>((0..rows).map(|i| {
            let label = match i {
                0 => first_label.to_string(),
                _ => format!("Item {i}"),
            };
            row([
                text(label, style.clone()),
                row([text("On", style.clone())]),
                row([row([text("Buy", style.clone())])]),
            ])
        }))
    };
    let vp = Viewport::new(Size::new(800.0, 600.0), 1.0);
    let mut fonts = text_system();
    let mut layout = TaffyLayout::new();
    let frame = |layout: &mut TaffyLayout, fonts: &mut kit_view::CosmicText, root| {
        let before = fonts.layout_calls();
        layout.layout(root, vp, fonts, &NoWorld).unwrap();
        (layout.last_reuse(), fonts.layout_calls() - before)
    };
    let nothing = Reuse::default();

    let (first, _) = frame(&mut layout, &mut fonts, list(rows, "Item 0"));
    assert_eq!(first.created, 1 + rows * nodes_per_row);

    let (same, calls) = frame(&mut layout, &mut fonts, list(rows, "Item 0"));
    assert_eq!(same, nothing);
    assert_eq!(calls, 0, "an unchanged frame asks the text system nothing");

    let (edited, _) = frame(&mut layout, &mut fonts, list(rows, "Item zero"));
    assert_eq!(
        edited,
        Reuse {
            remeasured: 1,
            ..nothing
        }
    );

    // Rows added or dropped at the end leave every row before them alone.
    let (longer, _) = frame(&mut layout, &mut fonts, list(rows + 2, "Item zero"));
    assert_eq!(
        longer,
        Reuse {
            created: 2 * nodes_per_row,
            ..nothing
        }
    );
    let (shorter, _) = frame(&mut layout, &mut fonts, list(rows - 1, "Item zero"));
    assert_eq!(
        shorter,
        Reuse {
            removed: 3 * nodes_per_row,
            ..nothing
        }
    );

    // Sizes a text system measured are only good for that system.
    let mut other = text_system();
    let all = 1 + (rows - 1) * nodes_per_row;
    let (swapped, _) = frame(&mut layout, &mut other, list(rows - 1, "Item zero"));
    assert_eq!(
        swapped,
        Reuse {
            created: all,
            removed: all,
            ..nothing
        }
    );
}

/// Regression: with the text cache bounded below the number of labels a frame shows, the layouts of every frame were
/// evicted by age and shaped again next frame, forever (the bound is 4096 by default, about 1,300 three-label rows).
/// And `assemble` no longer asks for layouts it remembers, so the cache must be told they are still in use or it ages
/// them out after `retain_frames` and the leaf asks again. Frames here run the real `begin_frame`/`end_frame` pacing.
#[test]
fn a_list_with_more_labels_than_the_text_cache_bound_shapes_nothing_when_unchanged() {
    let style = TextStyle::new(13.0, kit_core::peniko::Color::WHITE);
    let rows = 40;
    let list = || {
        column::<()>((0..rows).map(|i| {
            row([
                text(format!("Item {i}"), style.clone()),
                text("On", style.clone()),
                text(format!("Buy {i}"), style.clone()),
            ])
        }))
    };
    let mut fonts = kit_view::testing::text_system_bounded(10);
    let mut layout = TaffyLayout::new();
    let vp = viewport(800.0, 600.0);
    let mut frame = |fonts: &mut CosmicText| {
        let before = fonts.layout_calls();
        fonts.begin_frame();
        layout.layout(list(), vp, fonts, &NoWorld).unwrap();
        fonts.end_frame();
        fonts.layout_calls() - before
    };
    assert!(frame(&mut fonts) > 0);
    // "On" is one layout; the other labels are distinct: 2 * rows + 1, well over the bound of 10.
    let distinct = 2 * rows + 1;
    assert_eq!(fonts.cached_layouts(), distinct);
    for _ in 0..5 {
        assert_eq!(frame(&mut fonts), 0, "an unchanged frame shapes nothing");
        assert_eq!(fonts.cached_layouts(), distinct);
    }
}
