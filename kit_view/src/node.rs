use crate::scroll::ScrollSpec;
use crate::{
    CosmicText, Dismissal, Edit, FieldView, FocusReturn, Layer, Masking, MaxWidth, PortalAnchor,
    RichText, Shaped, Style, TextLayout, TextStyle,
};
use crate::{Image, ImageStyle};
use kit_core::kurbo::{self, Point, Size, Vec2};
use kit_core::peniko::Brush;
use kit_core::{Identity, Key, Modifiers, ShapeSpec, StableHasher, Surface, WidgetId};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use taffy as tf;

/// How a shape node is inked.
#[derive(Clone, PartialEq, Debug)]
pub enum Paint {
    Fill(Brush),
    Stroke {
        brush: Brush,
        stroke: kurbo::Stroke,
    },
    FillStroke {
        fill: Brush,
        brush: Brush,
        stroke: kurbo::Stroke,
    },
}

/// Layer clip relative to the layer's own box (the absolute `Clip` is produced at paint time).
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum ClipSpec {
    Unclipped,
    Bounds,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Availability {
    Enabled,
    /// Skipped by hit-testing (subtree inert), like a disabled `keel_ui` region.
    Disabled,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Role {
    Button,
    Slot,
    Bar,
    Panel,
    Label,
    /// An editable single-line field.
    TextBox,
    /// A two- or three-state check (`Toggle` is its checked state: `aria-checked`).
    CheckBox,
    /// A button that opens a list to choose one value from (`aria-haspopup="listbox"`).
    ComboBox,
    /// One choice in a list a combo box opened (`Toggle::On` is the chosen one: `aria-selected`).
    Option,
    /// A number picked from a range by pointer or arrow keys; [`A11y::Ranged`] announces its value and bounds.
    Slider,
}

/// Whether a control is a two-state toggle, and which state it is in. Most controls are not toggles (`NotToggle`
/// announces nothing); a tool or panel button that can be on says so either way, so "off" is not the same as "absent".
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Toggle {
    NotToggle,
    Off,
    On,
    /// Neither: a check box over a group whose members differ (egui's `indeterminate`, ARIA "mixed").
    Mixed,
}

#[derive(Clone, PartialEq, Debug)]
pub enum A11y {
    Hidden,
    Labeled {
        role: Role,
        label: String,
        toggle: Toggle,
    },
    /// A [`Role::Slider`]: its value and bounds are part of what it says (`aria-valuenow/min/max`).
    Ranged {
        label: String,
        value: RangeValue,
    },
}

impl A11y {
    /// A labelled control that is not a toggle (most of them).
    pub fn named(role: Role, label: impl Into<String>) -> Self {
        A11y::Labeled {
            role,
            label: label.into(),
            toggle: Toggle::NotToggle,
        }
    }
    /// A labelled control in a toggle state (a check box, a selected option).
    pub fn toggled(role: Role, label: impl Into<String>, toggle: Toggle) -> Self {
        A11y::Labeled {
            role,
            label: label.into(),
            toggle,
        }
    }
}

/// Where a ranged control stands: `now` within `min..=max`.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct RangeValue {
    pub now: f64,
    pub min: f64,
    pub max: f64,
}

/// A key bound to a control: pressing it while the control is enabled activates it. `Unbound` is the meaning of most
/// controls, so it is named rather than an `Option`.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Shortcut {
    Unbound,
    Key(Key),
}

/// Hover help attached to a control. Absence is named: a control with nothing to say has `Tooltip::None`.
#[derive(Clone, PartialEq, Debug)]
pub enum Tooltip {
    None,
    Text(String),
    /// Text plus the thing it is about, in the same app-defined `kind`/`value` terms a drag carries, so a host can
    /// act on the hover (preview a weapon's range) without a side table from control to subject.
    About {
        text: String,
        subject: DragPayload,
    },
}
impl Tooltip {
    /// `None` is the meaning: a tooltip-less control.
    pub fn text(&self) -> Option<&str> {
        match self {
            Tooltip::None => None,
            Tooltip::Text(text) | Tooltip::About { text, .. } => Some(text),
        }
    }
    pub fn subject(&self) -> Option<DragPayload> {
        match self {
            Tooltip::None | Tooltip::Text(_) => None,
            Tooltip::About { subject, .. } => Some(*subject),
        }
    }
}

/// What a drag carries. Apps define their own `kind` constants; the router only compares kinds.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct DragPayload {
    pub kind: u32,
    pub value: u64,
}
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct DragSpec {
    pub payload: DragPayload,
}
/// Delivered to `DropSpec::on_drop`.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Dropped {
    pub payload: DragPayload,
    pub source: WidgetId,
    pub target: WidgetId,
}
pub struct DropSpec<Msg> {
    /// Only payloads of this kind are accepted.
    pub accepts: u32,
    pub on_drop: Rc<dyn Fn(Dropped) -> Msg>,
}
impl<Msg> Clone for DropSpec<Msg> {
    fn clone(&self) -> Self {
        Self {
            accepts: self.accepts,
            on_drop: self.on_drop.clone(),
        }
    }
}
impl<Msg> fmt::Debug for DropSpec<Msg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DropSpec(accepts {})", self.accepts)
    }
}
/// Which moment of a captured pointer a [`PointerAt`] reports.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum PointerPhase {
    /// The press that began the capture (no slop: a slider jumps to it).
    Press,
    /// Every move while captured, wherever the pointer is (outside the node too).
    Drag,
    /// The pointer let go, wherever it is.
    Release,
    /// The capture ended without a release: Escape, the platform's cancel, window blur/resize, or an epoch change.
    /// `local`/`size` are the last ones reported.
    Cancel,
}
/// Whether a captured pointer ever left the press's slop circle (`GestureConfig::slop`). A component tells a click
/// from a drag with this instead of owning a slop constant of its own.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Travel {
    Still,
    Moved,
}
/// A captured pointer, relative to the bound node's laid rect: `local` is (0, 0) at its top-left and is NOT clamped
/// (a drag beyond the node reports coordinates outside `0..size`; clamping is the component's math).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct PointerAt {
    pub phase: PointerPhase,
    pub local: Point,
    pub size: Size,
    pub travel: Travel,
}
impl PointerAt {
    /// `local` as a fraction of `size` on each axis (unclamped; 0..1 inside the node).
    pub fn fraction(&self) -> Vec2 {
        Vec2::new(
            self.local.x / self.size.width,
            self.local.y / self.size.height,
        )
    }
}
/// Delivered by the router for every phase of a pointer captured by this node (see [`Bindings::on_pointer`]).
pub struct PointerSpec<Msg> {
    pub on: Rc<dyn Fn(PointerAt) -> Msg>,
}
impl<Msg> Clone for PointerSpec<Msg> {
    fn clone(&self) -> Self {
        Self {
            on: self.on.clone(),
        }
    }
}
impl<Msg> fmt::Debug for PointerSpec<Msg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PointerSpec")
    }
}
#[derive(Clone, PartialEq, Debug)]
pub struct HoverSpec<Msg> {
    pub enter: Msg,
    pub leave: Msg,
}

/// A key pressed while the bound control has keyboard focus (see [`Bindings::on_key`]).
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct KeyPress {
    pub key: Key,
    pub mods: Modifiers,
}
/// What a focused control did with a key: its message, or `Pass` (not its key: the router goes on to arrow-key focus
/// walking, Escape and global hotkeys, exactly as if the control bound no keys).
#[derive(Clone, PartialEq, Debug)]
pub enum KeyAnswer<Msg> {
    Handled(Msg),
    Pass,
}
pub struct KeySpec<Msg> {
    pub on: Rc<dyn Fn(KeyPress) -> KeyAnswer<Msg>>,
}
/// An activation that depends on the modifiers held at the press (shift-click extends, cmd-click toggles).
pub struct PressSpec<Msg> {
    pub on: Rc<dyn Fn(Modifiers) -> Msg>,
}
/// A context gesture (secondary-button press and release on the control): the message, built from where it was
/// released in screen px. Separate from `activate`, which a secondary button never fires.
pub struct ContextSpec<Msg> {
    pub on: Rc<dyn Fn(Point) -> Msg>,
}
macro_rules! rc_spec {
    ($($spec:ident),*) => {$(
        impl<Msg> Clone for $spec<Msg> {
            fn clone(&self) -> Self {
                Self { on: self.on.clone() }
            }
        }
        impl<Msg> fmt::Debug for $spec<Msg> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, stringify!($spec))
            }
        }
    )*};
}
rc_spec!(KeySpec, PressSpec, ContextSpec);

/// Programmatic focus as data on a node. Edge-triggered like a scroll reveal: `Router::frame` focuses the node once
/// per distinct ticket, so the player can move focus away afterwards. `Nothing` is the meaning of most nodes.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum FocusClaim {
    Nothing,
    Ticket(u64),
}
impl FocusClaim {
    /// A claim renewed whenever `stamp` changes (an edit session counter, a selection revision).
    pub fn of(stamp: &impl Hash) -> Self {
        let mut hasher = StableHasher::new();
        stamp.hash(&mut hasher);
        FocusClaim::Ticket(hasher.finish())
    }
}

/// Typed message bindings. `None` here means "no such binding", which is the meaning.
#[derive(Clone, Debug)]
pub struct Bindings<Msg> {
    pub activate: Option<Msg>,
    /// Activates the control from the keyboard. Only meaningful with `activate`.
    pub shortcut: Shortcut,
    pub drag: Option<DragSpec>,
    pub drop: Option<DropSpec<Msg>>,
    pub hover: Option<HoverSpec<Msg>>,
    pub pointer: Option<PointerSpec<Msg>>,
    /// Keys delivered while this control has focus (after the router's Tab/Enter/Space, before global hotkeys).
    pub key: Option<KeySpec<Msg>>,
    /// Builds the activation from the modifiers held; `activate` is then its unmodified message.
    pub press: Option<PressSpec<Msg>>,
    /// Secondary-button click (press and release on this control). Never fires `activate`.
    pub context: Option<ContextSpec<Msg>>,
    pub claim: FocusClaim,
}
impl<Msg> Default for Bindings<Msg> {
    fn default() -> Self {
        Self::none()
    }
}
impl<Msg> Bindings<Msg> {
    pub fn none() -> Self {
        Self {
            activate: None,
            shortcut: Shortcut::Unbound,
            drag: None,
            drop: None,
            hover: None,
            pointer: None,
            key: None,
            press: None,
            context: None,
            claim: FocusClaim::Nothing,
        }
    }
    pub fn activate(msg: Msg) -> Self {
        Self {
            activate: Some(msg),
            ..Self::none()
        }
    }
    pub fn with_shortcut(mut self, key: Key) -> Self {
        self.shortcut = Shortcut::Key(key);
        self
    }
    pub fn with_drag(mut self, payload: DragPayload) -> Self {
        self.drag = Some(DragSpec { payload });
        self
    }
    pub fn with_drop(mut self, accepts: u32, on_drop: impl Fn(Dropped) -> Msg + 'static) -> Self {
        self.drop = Some(DropSpec {
            accepts,
            on_drop: Rc::new(on_drop),
        });
        self
    }
    pub fn with_hover(mut self, enter: Msg, leave: Msg) -> Self {
        self.hover = Some(HoverSpec { enter, leave });
        self
    }
    /// Capture the pointer from press to release and turn its positions into messages (a slider, a colour square,
    /// a numeric drag). The binding owns the press: the router starts tracking at once (no slop), `activate` then fires
    /// only from the keyboard (Enter/Space while focused), and a `drag` payload on the same node is never carried.
    pub fn on_pointer(mut self, on: impl Fn(PointerAt) -> Msg + 'static) -> Self {
        self.pointer = Some(PointerSpec { on: Rc::new(on) });
        self
    }
    /// While this control has keyboard focus, every key press (after the router's own Tab, and Enter/Space when
    /// `activate` is bound) is offered to `on`; `KeyAnswer::Pass` hands it on to arrow-key focus walking, Escape and
    /// global hotkeys. A key-bound control is focusable (Tab reaches it) even with nothing to activate.
    pub fn on_key(mut self, on: impl Fn(KeyPress) -> KeyAnswer<Msg> + 'static) -> Self {
        self.key = Some(KeySpec { on: Rc::new(on) });
        self
    }
    /// A secondary-button click on this control sends `on(point)`, the screen position of the release (where a
    /// context menu opens). It is independent of `activate`: a right-click never fires the primary action, and a
    /// left-click never fires this. A long-press is not recognised (see the kit_input README section).
    pub fn on_context(mut self, on: impl Fn(Point) -> Msg + 'static) -> Self {
        self.context = Some(ContextSpec { on: Rc::new(on) });
        self
    }
    /// Focus this control once per distinct `stamp` (see [`FocusClaim`]).
    pub fn claim_focus(mut self, stamp: &impl Hash) -> Self {
        self.claim = FocusClaim::of(stamp);
        self
    }
    /// A press can start a gesture only if something would fire from it.
    pub fn captures_press(&self) -> bool {
        self.activate.is_some()
            || self.drag.is_some()
            || self.pointer.is_some()
            || self.key.is_some()
    }
    /// Tab can land on it: it can be pressed, or it answers a context click (so a menu it opens has an opener focus
    /// can return to).
    pub fn is_focusable(&self) -> bool {
        self.captures_press() || self.context.is_some()
    }
}
impl<Msg: Clone> Bindings<Msg> {
    /// `activate`, built from the modifiers held at the press (shift-click, cmd-click). `activate` is set to the
    /// unmodified message, so a host or mirror that activates without modifiers still has one.
    pub fn activate_with(on: impl Fn(Modifiers) -> Msg + 'static) -> Self {
        Self {
            activate: Some(on(Modifiers::default())),
            press: Some(PressSpec { on: Rc::new(on) }),
            ..Self::none()
        }
    }
    /// What an activation with `mods` held sends: the modifier-aware message when one is bound.
    pub fn activation(&self, mods: Modifiers) -> Option<Msg> {
        match &self.press {
            Some(press) => Some((press.on)(mods)),
            None => self.activate.clone(),
        }
    }
}
impl<Msg: 'static> Bindings<Msg> {
    pub fn map<M2: 'static>(self, f: impl Fn(Msg) -> M2 + 'static) -> Bindings<M2> {
        self.map_rc(&(Rc::new(f) as Rc<dyn Fn(Msg) -> M2>))
    }
    pub(crate) fn map_rc<M2: 'static>(self, f: &Rc<dyn Fn(Msg) -> M2>) -> Bindings<M2> {
        Bindings {
            activate: self.activate.map(|m| f(m)),
            shortcut: self.shortcut,
            drag: self.drag,
            drop: self.drop.map(|d| {
                let (inner, f) = (d.on_drop, f.clone());
                DropSpec {
                    accepts: d.accepts,
                    on_drop: Rc::new(move |x| f(inner(x))),
                }
            }),
            hover: self.hover.map(|h| HoverSpec {
                enter: f(h.enter),
                leave: f(h.leave),
            }),
            pointer: self.pointer.map(|p| {
                let (inner, f) = (p.on, f.clone());
                PointerSpec {
                    on: Rc::new(move |x| f(inner(x))),
                }
            }),
            key: self.key.map(|k| {
                let (inner, f) = (k.on, f.clone());
                KeySpec {
                    on: Rc::new(move |x| match inner(x) {
                        KeyAnswer::Handled(msg) => KeyAnswer::Handled(f(msg)),
                        KeyAnswer::Pass => KeyAnswer::Pass,
                    }),
                }
            }),
            press: self.press.map(|p| {
                let (inner, f) = (p.on, f.clone());
                PressSpec {
                    on: Rc::new(move |x| f(inner(x))),
                }
            }),
            context: self.context.map(|c| {
                let (inner, f) = (c.on, f.clone());
                ContextSpec {
                    on: Rc::new(move |at| f(inner(at))),
                }
            }),
            claim: self.claim,
        }
    }
}

/// How a text field is drawn and shaped. What it contains lives in the app's `EditorState`; the node carries a
/// [`FieldView`] copy of it for the frame.
#[derive(Clone, PartialEq, Debug)]
pub struct FieldStyle {
    pub text: TextStyle,
    pub masking: Masking,
    /// Space between the field's box and its text, logical px. The text is shaped for the box width less this.
    pub inset: Vec2,
}
impl FieldStyle {
    /// Shape what the field shows (value, masking, composition) for `max_width`.
    pub fn shape(
        &self,
        text: &mut CosmicText,
        view: &FieldView,
        max_width: MaxWidth,
    ) -> (Shaped, TextLayout) {
        let shown = Shaped::new(view, self.masking);
        let layout = text.layout(&RichText::plain(shown.text()), &self.text, max_width);
        (shown, layout)
    }
}

/// A point in the game world; projection to the screen belongs to the game (`WorldProjector`).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct WorldPoint(pub [f64; 3]);
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct WorldAnchor {
    pub point: WorldPoint,
    /// Screen-space offset applied after projection (e.g. float above a unit's head).
    pub offset: Vec2,
}

pub enum Node<Msg> {
    Stack {
        style: Style,
        children: Vec<Node<Msg>>,
    },
    /// Container with a drawn background (added to the sketch: fill/border/shadow need a draw primitive of their own).
    Panel {
        style: Style,
        surface: Surface,
        children: Vec<Node<Msg>>,
    },
    /// No `id` (sketch had `Option<WidgetId>`): wrap in `Interactive`/`Scroll` when a text node needs identity.
    Text {
        style: Style,
        text: RichText,
        text_style: TextStyle,
    },
    Image {
        style: Style,
        image: Image,
        image_style: ImageStyle,
    },
    Shape {
        style: Style,
        shape: ShapeSpec,
        paint: Paint,
    },
    Interactive {
        identity: Identity,
        on: Bindings<Msg>,
        a11y: A11y,
        tooltip: Tooltip,
        availability: Availability,
        child: Box<Node<Msg>>,
    },
    /// `style` sizes the viewport; the child is the content and may be larger along `spec.axis`.
    Scroll {
        style: Style,
        identity: Identity,
        spec: ScrollSpec,
        child: Box<Node<Msg>>,
    },
    TextField {
        style: Style,
        identity: Identity,
        /// Accessible name (screen readers, tests); not drawn.
        label: String,
        view: FieldView,
        on_edit: Rc<dyn Fn(Edit) -> Msg>,
        field: FieldStyle,
        claim: FocusClaim,
    },
    Layer {
        opacity: f32,
        clip: ClipSpec,
        child: Box<Node<Msg>>,
    },
    World {
        anchor: WorldAnchor,
        child: Box<Node<Msg>>,
    },
    /// Drawn above the normal tree on `layer`, placed against `anchor`; takes no space in its parent.
    Portal {
        layer: Layer,
        anchor: PortalAnchor,
        dismissal: Dismissal<Msg>,
        returns: FocusReturn,
        child: Box<Node<Msg>>,
    },
}

impl<Msg> Node<Msg> {
    /// Box style that governs how the parent places this node (wrappers defer to their child).
    pub fn style(&self) -> &Style {
        match self {
            Node::Stack { style, .. }
            | Node::Panel { style, .. }
            | Node::Text { style, .. }
            | Node::Image { style, .. }
            | Node::Shape { style, .. }
            | Node::Scroll { style, .. }
            | Node::TextField { style, .. } => style,
            Node::Interactive { child, .. }
            | Node::Layer { child, .. }
            | Node::World { child, .. }
            | Node::Portal { child, .. } => child.style(),
        }
    }
    pub fn children(&self) -> &[Node<Msg>] {
        match self {
            Node::Stack { children, .. } | Node::Panel { children, .. } => children,
            Node::Interactive { child, .. }
            | Node::Scroll { child, .. }
            | Node::Layer { child, .. }
            | Node::World { child, .. }
            | Node::Portal { child, .. } => core::slice::from_ref(child),
            Node::Image { .. }
            | Node::Text { .. }
            | Node::Shape { .. }
            | Node::TextField { .. } => &[],
        }
    }
    /// Focus this node once per distinct `stamp` ([`FocusClaim`]): a text field opened by a click, the row a
    /// keyboard selection moved to. Panics on a node that cannot take focus (only a control or a text field can).
    pub fn claim_focus(self, stamp: &impl Hash) -> Self {
        match self {
            Node::Interactive {
                identity,
                on,
                a11y,
                tooltip,
                availability,
                child,
            } => Node::Interactive {
                identity,
                on: on.claim_focus(stamp),
                a11y,
                tooltip,
                availability,
                child,
            },
            Node::TextField {
                style,
                identity,
                label,
                view,
                on_edit,
                field,
                ..
            } => Node::TextField {
                style,
                identity,
                label,
                view,
                on_edit,
                field,
                claim: FocusClaim::of(stamp),
            },
            Node::Stack { .. }
            | Node::Panel { .. }
            | Node::Text { .. }
            | Node::Image { .. }
            | Node::Shape { .. }
            | Node::Scroll { .. }
            | Node::Layer { .. }
            | Node::World { .. }
            | Node::Portal { .. } => {
                panic!("claim_focus: only a control or a text field can take focus")
            }
        }
    }
    /// Number of nodes in pre-order (the length of the `Placed` list a layout engine must return).
    pub fn count(&self) -> usize {
        1 + self.children().iter().map(Node::count).sum::<usize>()
    }
    /// Replace the box style of the node that governs placement (wrappers forward to their child).
    pub fn styled(self, new: Style) -> Self {
        match self {
            Node::Stack { children, .. } => Node::Stack {
                style: new,
                children,
            },
            Node::Panel {
                surface, children, ..
            } => Node::Panel {
                style: new,
                surface,
                children,
            },
            Node::Text {
                text, text_style, ..
            } => Node::Text {
                style: new,
                text,
                text_style,
            },
            Node::Image {
                image, image_style, ..
            } => Node::Image {
                style: new,
                image,
                image_style,
            },
            Node::Shape { shape, paint, .. } => Node::Shape {
                style: new,
                shape,
                paint,
            },
            Node::Scroll {
                identity,
                spec,
                child,
                ..
            } => Node::Scroll {
                style: new,
                identity,
                spec,
                child,
            },
            Node::TextField {
                identity,
                label,
                view,
                on_edit,
                field,
                claim,
                ..
            } => Node::TextField {
                style: new,
                identity,
                label,
                view,
                on_edit,
                field,
                claim,
            },
            Node::Interactive {
                identity,
                on,
                a11y,
                tooltip,
                availability,
                child,
            } => Node::Interactive {
                identity,
                on,
                a11y,
                tooltip,
                availability,
                child: Box::new(child.styled(new)),
            },
            Node::Layer {
                opacity,
                clip,
                child,
            } => Node::Layer {
                opacity,
                clip,
                child: Box::new(child.styled(new)),
            },
            Node::World { anchor, child } => Node::World {
                anchor,
                child: Box::new(child.styled(new)),
            },
            Node::Portal {
                layer,
                anchor,
                dismissal,
                returns,
                child,
            } => Node::Portal {
                layer,
                anchor,
                dismissal,
                returns,
                child: Box::new(child.styled(new)),
            },
        }
    }
}

/// Style properties as chainable methods, so a build reads `row![..].gap(8.).pad(12.)` instead of a `Style` literal.
/// Each rewrites the style of the node that governs placement (see [`Node::styled`]).
impl<Msg> Node<Msg> {
    fn restyled(self, edit: impl FnOnce(Style) -> Style) -> Self {
        let style = edit(self.style().clone());
        self.styled(style)
    }
    pub fn gap(self, gap: f64) -> Self {
        self.restyled(|s| s.gap(gap))
    }
    /// Uniform padding.
    pub fn pad(self, all: f64) -> Self {
        self.restyled(|s| s.padded(all))
    }
    pub fn pad_xy(self, x: f64, y: f64) -> Self {
        self.restyled(|s| s.padded_xy(x, y))
    }
    pub fn size(self, width: f64, height: f64) -> Self {
        self.restyled(|s| s.sized(width, height))
    }
    /// `Percent(..)` or `px(..)`.
    pub fn width(self, width: impl Into<tf::Dimension>) -> Self {
        let width = width.into();
        self.restyled(|mut s| {
            s.layout.size.width = width;
            s
        })
    }
    /// Largest width this node may take (a wrapping row breaks lines at it). `Percent(..)` or `px(..)`.
    pub fn max_width(self, width: impl Into<tf::LengthPercentageAuto>) -> Self {
        let width = width.into();
        self.restyled(|mut s| {
            s.layout.max_size.width = width;
            s
        })
    }
    pub fn grow(self, grow: f32) -> Self {
        self.restyled(|s| s.grow(grow))
    }
    /// Take the full width the parent offers, instead of shrink-wrapping to the content. A column or screen region
    /// that holds fluid children (a gauge, a percent-width box) needs this: a percent of a shrink-wrapped box has
    /// nothing to resolve against and collapses to nothing.
    pub fn fill_width(self) -> Self {
        self.restyled(|s| s.align_self(tf::AlignSelf::STRETCH))
    }
    /// Let this row or column break onto further lines when its children do not fit its width.
    pub fn wrap(self) -> Self {
        self.restyled(|s| s.wrap())
    }
    /// Cross-axis alignment of this node's children.
    pub fn align_items(self, align: tf::AlignItems) -> Self {
        self.restyled(|s| s.align_items(align))
    }
    /// Main-axis distribution of this node's children.
    pub fn justify(self, justify: tf::JustifyContent) -> Self {
        self.restyled(|s| s.justify(justify))
    }
    /// How this node sits on its parent's cross axis (overrides the parent's `align_items`).
    pub fn align_self(self, align: tf::AlignSelf) -> Self {
        self.restyled(|s| s.align_self(align))
    }
}

impl<Msg: 'static> Node<Msg> {
    /// Nest a child's messages into a parent enum: `child_view(..).map(Parent::Child)`.
    pub fn map<M2: 'static>(self, f: impl Fn(Msg) -> M2 + 'static) -> Node<M2> {
        self.map_rc(&(Rc::new(f) as Rc<dyn Fn(Msg) -> M2>))
    }
    pub(crate) fn map_rc<M2: 'static>(self, f: &Rc<dyn Fn(Msg) -> M2>) -> Node<M2> {
        match self {
            Node::Stack { style, children } => Node::Stack {
                style,
                children: map_all(children, f),
            },
            Node::Panel {
                style,
                surface,
                children,
            } => Node::Panel {
                style,
                surface,
                children: map_all(children, f),
            },
            Node::Text {
                style,
                text,
                text_style,
            } => Node::Text {
                style,
                text,
                text_style,
            },
            Node::Image {
                style,
                image,
                image_style,
            } => Node::Image {
                style,
                image,
                image_style,
            },
            Node::Shape {
                style,
                shape,
                paint,
            } => Node::Shape {
                style,
                shape,
                paint,
            },
            Node::Interactive {
                identity,
                on,
                a11y,
                tooltip,
                availability,
                child,
            } => Node::Interactive {
                identity,
                on: on.map_rc(f),
                a11y,
                tooltip,
                availability,
                child: Box::new(child.map_rc(f)),
            },
            Node::Scroll {
                style,
                identity,
                spec,
                child,
            } => Node::Scroll {
                style,
                identity,
                spec,
                child: Box::new(child.map_rc(f)),
            },
            Node::TextField {
                style,
                identity,
                label,
                view,
                on_edit,
                field,
                claim,
            } => {
                let f = f.clone();
                Node::TextField {
                    style,
                    identity,
                    label,
                    view,
                    on_edit: Rc::new(move |e| f(on_edit(e))),
                    field,
                    claim,
                }
            }
            Node::Layer {
                opacity,
                clip,
                child,
            } => Node::Layer {
                opacity,
                clip,
                child: Box::new(child.map_rc(f)),
            },
            Node::World { anchor, child } => Node::World {
                anchor,
                child: Box::new(child.map_rc(f)),
            },
            Node::Portal {
                layer,
                anchor,
                dismissal,
                returns,
                child,
            } => Node::Portal {
                layer,
                anchor,
                dismissal: dismissal.map_rc(f),
                returns,
                child: Box::new(child.map_rc(f)),
            },
        }
    }
}

fn map_all<Msg: 'static, M2: 'static>(
    children: Vec<Node<Msg>>,
    f: &Rc<dyn Fn(Msg) -> M2>,
) -> Vec<Node<M2>> {
    children.into_iter().map(|c| c.map_rc(f)).collect()
}
