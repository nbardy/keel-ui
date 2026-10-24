// Ported from egui 0.36.2 `crates/egui/src/containers/area.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Caller-owned FIFO toast queue. Keep timestamps fixed per id and retain items through their exit.
use crate::common::{Press, ink, pressable, surface};
use core::time::Duration;
use kit_core::kurbo::Vec2;
use kit_core::{Border, Shadow, Theme, Time, WidgetId};
use kit_state::{
    Act, Cx, Disclosure, DisclosureEv, Easing, Host, Ident, Intent, Life, Phase, Presence, Since,
};
use kit_view::{A11y, Anchor, AnchorH, AnchorV, Layer, Node, PortalAnchor, Role, SafeArea};
use kit_view::{Style, Toggle, Weight, column, layer, panel, portal, text};

pub struct Toast<'a, K> {
    pub key: K,
    pub label: &'a str,
    pub born: Time,
    pub expires: Time,
}
pub struct ToastStack<'a, K> {
    pub theme: &'a Theme,
    pub items: &'a [Toast<'a, K>],
    /// Oldest first; queued items keep their original deadlines. Zero hides all.
    pub max_visible: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Lease<K>(K, Duration, Duration);
impl<A: Host, K: Ident<A>> Ident<A> for Lease<K> {
    fn life(&self, app: &A) -> Life {
        let end = Time(self.1 + self.2);
        if app.now() >= end {
            return Life::Over;
        }
        match self.0.life(app) {
            Life::Live if app.now().0 < self.1 => Life::Live,
            Life::Live => Life::Leaving(end),
            Life::Leaving(until) => Life::Leaving(until.min(end)),
            Life::Over => Life::Over,
        }
    }
}

/// Panics if expiration precedes creation. Ids and timestamps must remain stable across frames.
/// Click dismissal stays remembered until the lease ends; hosts call `Store::collect` as time advances.
pub fn toast<A: Host, K: Ident<A>>(cx: &Cx<'_, A>, p: ToastStack<'_, K>) -> Node<Intent<A>> {
    let th = p.theme;
    let nodes = p
        .items
        .iter()
        .filter_map(|t| item(cx, th, t))
        .take(p.max_visible);
    let anchor = Anchor {
        h: AnchorH::End,
        v: AnchorV::Start,
        offset: Vec2::new(-th.spacing.md, th.spacing.md),
        safe_area: SafeArea::Respect,
    };
    let body = column(nodes).gap(th.spacing.sm);
    portal(Layer::Toast, PortalAnchor::Viewport(anchor), body)
}

fn item<A: Host, K: Ident<A>>(
    cx: &Cx<'_, A>,
    th: &Theme,
    t: &Toast<'_, K>,
) -> Option<Node<Intent<A>>> {
    assert!(t.expires >= t.born, "toast: expiry before birth");
    let (c, fade) = (&th.colors, th.motion.fast());
    let presence = Presence {
        enter: fade,
        exit: fade,
    };
    let key = Lease(t.key.clone(), t.expires.0, fade);
    let life = key.life(cx.app());
    if life == Life::Over {
        return None;
    }
    let initial = Disclosure::Open {
        since: Since::At(cx.now()),
    };
    let state = cx.state(
        &key,
        || initial,
        |s: &mut Disclosure, _: &Lease<K>, ev: DisclosureEv, app: &A| {
            s.step(ev, app.now());
            Act::none()
        },
    );
    let disclosure = match (life, *state) {
        (Life::Leaving(until), Disclosure::Open { .. }) => Disclosure::Closed {
            since: Since::At(Time(until.0.saturating_sub(fade))),
        },
        (Life::Live | Life::Leaving(_), value) => value,
        (Life::Over, _) => unreachable!("over leases were filtered"),
    };
    let (opacity, press) = match disclosure.phase(presence, cx.now()) {
        Phase::Entering(x) => (
            Easing::OutQuad.at(x),
            Press::Sends(state.send(DisclosureEv::Close)),
        ),
        Phase::Shown => (1.0, Press::Sends(state.send(DisclosureEv::Close))),
        Phase::Exiting(x) => (1.0 - Easing::OutQuad.at(x), Press::Disabled),
        Phase::Gone => return None,
    };
    let style = Style::row().padded_xy(th.spacing.md, th.spacing.sm);
    let frame = surface(
        c.surface_raised,
        th.radii.control,
        Border::None,
        Shadow::None,
    );
    let caption = text(t.label, ink(th.type_scale.body, c.text, Weight::NORMAL));
    let face = panel(style, frame, vec![caption]);
    let a11y = A11y::Labeled {
        role: Role::Button,
        label: t.label.into(),
        toggle: Toggle::NotToggle,
    };
    let node = pressable(WidgetId::of(&key), press, a11y, face);
    Some(layer(opacity, node))
}
