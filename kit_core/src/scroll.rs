use kurbo::{Size, Vec2};

/// A scroll area as the input router last measured it, for components that build from it (virtual lists).
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum ScrollView {
    /// The router has not laid this area out yet (its first frame), or no router feeds this tree.
    Unseen,
    /// `offset` in logical px from the content's start; `viewport` is the area's box, `content` its child's.
    Seen {
        offset: Vec2,
        viewport: Size,
        content: Size,
    },
}
