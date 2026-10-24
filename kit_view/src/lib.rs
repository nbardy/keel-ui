//! Seam: the app-facing view tree. `Node<Msg>` is plain data built every frame by `view(&State)`; `Style` is taffy's
//! own style plus screen anchors; text is `kit_text` (cosmic-text types, re-exported here for node builders);
//! `LaidOut<Msg>` is what `kit_layout::TaffyLayout` produces and what paint/input consume.

mod builders;
mod image;
mod laid;
mod node;
mod portal;
mod screen;
mod scroll;
mod semantics;
mod style;

pub use builders::*;
pub use image::{
    Crop, Filter, Fit, Image, ImageKey, ImagePixels, ImageRegion, ImageRegionError, ImageSize,
    ImageSizeError, ImageSource, ImageStyle, PixelsError, image,
};
pub use kit_text::{
    Affinity, Align as TextAlign, Applied, Caret, Composition, CosmicText, Cursor, Edit,
    EditorState, FieldGeometry, FieldView, Goal, Masking, MaxWidth, Mode, Motion, Numerals,
    Overflow, Rev, RichText, Selection, Shaped, Span, SpanColor, SpanStyle, SpanWeight, TextLayout,
    TextLayoutId, TextStyle, TextValue, Unit, Weight, Wrap as TextWrap,
};
pub use laid::{
    Control, LaidKind, LaidNode, LaidOut, LayoutError, NoWorld, Placed, Projection, Reach, Shown,
    WorldProjector,
};
pub use node::{
    A11y, Availability, Bindings, ClipSpec, ContextSpec, DragPayload, DragSpec, DropSpec, Dropped,
    FieldStyle, FocusClaim, HoverSpec, KeyAnswer, KeyPress, KeySpec, Node, Paint, PointerAt,
    PointerPhase, PointerSpec, PressSpec, RangeValue, Role, Shortcut, Toggle, Tooltip, Travel,
    WorldAnchor, WorldPoint,
};
pub use portal::{
    Dismissal, FocusReturn, Layer, ModalFloor, Parent, PortalAnchor, PortalRef, Side, Stacking,
    place_beside,
};
pub use screen::Screen;
pub use scroll::{Bars, Reveal, RevealAlign, RevealTarget, ScrollSpec, Stick};
pub use semantics::{
    Description, Semantic, SemanticState, SemanticValue, semantics, semantics_scrolled,
};
pub use style::{
    Anchor, AnchorH, AnchorV, Content, Percent, Placement, SafeArea, Style, px, taffy,
};

pub use kit_text;

pub mod testing;
