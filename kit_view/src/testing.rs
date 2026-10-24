//! Helpers so kit crates (and games) can test routing and painting with explicit rects and the bundled font.
use crate::{CosmicText, LaidOut, Node, Placed, Shown};
use kit_core::Viewport;
use kit_core::kurbo::{Rect, Size};
use kit_text::{TextConfig, inter};

/// The bundled UI font (`kit_text::inter`) as a text system: what every kit test lays text out with, so tests measure
/// what ships.
pub fn text_system() -> CosmicText {
    CosmicText::new(inter::source(), TextConfig::default()).expect("the bundled Inter loads")
}

/// Assemble a tree from explicit pre-order rects (all visible). Panics are impossible by contract: a wrong count is
/// returned as the typed `LayoutError`, which tests unwrap.
pub fn laid_out<Msg>(root: Node<Msg>, rects: &[Rect]) -> LaidOut<Msg> {
    let placed: Vec<Placed> = rects
        .iter()
        .map(|&rect| Placed {
            rect,
            shown: Shown::Visible,
        })
        .collect();
    let viewport = Viewport::new(Size::new(1000.0, 1000.0), 1.0);
    LaidOut::assemble(root, viewport, &placed, &mut text_system())
        .expect("test rects match node count")
}

/// The control a screen reader would announce as `label` with `role`: tests address controls by what they are, not
/// by a number. First match in pre-order.
pub fn control<'a, Msg>(
    laid: &'a LaidOut<Msg>,
    role: crate::Role,
    label: &str,
) -> Option<&'a crate::LaidNode<Msg>> {
    laid.root
        .descendants()
        .into_iter()
        .find(|node| match &node.kind {
            crate::LaidKind::Interactive {
                a11y:
                    crate::A11y::Labeled {
                        role: r, label: l, ..
                    },
                ..
            } => *r == role && l == label,
            crate::LaidKind::Interactive {
                a11y: crate::A11y::Ranged { label: l, .. },
                ..
            } => role == crate::Role::Slider && l == label,
            crate::LaidKind::TextField { label: l, .. } => {
                role == crate::Role::TextBox && l == label
            }
            crate::LaidKind::Interactive {
                a11y: crate::A11y::Hidden,
                ..
            }
            | crate::LaidKind::Stack { .. }
            | crate::LaidKind::Panel { .. }
            | crate::LaidKind::Text { .. }
            | crate::LaidKind::Shape { .. }
            | crate::LaidKind::Scroll { .. }
            | crate::LaidKind::Layer { .. }
            | crate::LaidKind::World { .. }
            | crate::LaidKind::Portal { .. } => false,
        })
}
