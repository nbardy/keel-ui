// Ported from egui 0.36.2 `crates/egui/src/widgets/drag_value.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Numeric stepper. A press (or Enter/Space) steps at once; holding repeats on kit wake-ups (`Act::wake`): once at
//! 400 ms, every 150 ms to 1.6 s, then every 50 ms (task policy). The count comes from the time held, so a late
//! frame catches up. The hold ends on release or at the bound.
use crate::common::{ink, raised};
use kit_core::{Theme, Time, WidgetId};
use kit_state::{Act, Cx, Host, Ident, Intent};
use kit_view::{A11y, Bindings, Node, PointerAt, PointerPhase as Ph, Role, Style, Weight};
use kit_view::{disabled, interactive, panel, row, text};
use std::time::Duration;

pub struct Stepper<'a, K> {
    pub theme: &'a Theme,
    pub key: K,
    pub label: &'a str,
    pub initial: i64,
    pub min: i64,
    pub max: i64,
    pub step: i64,
}
/// `Repeat` is a wake-up, or a pointer move while held (both catch up with the time held).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Do {
    Hold,
    Repeat,
    Release,
}
#[derive(Clone, Debug, PartialEq)]
struct Ev(Do, i64, i64, i64);
#[derive(Clone, Copy)]
enum Held {
    Released,
    Since(Time, u128),
}
/// Steps owed after `ms` held, and when (ms after the press) the next one falls due.
fn ticks(ms: u128) -> (u128, u128) {
    let owed = 1 + u128::from(ms >= 400) + ms.saturating_sub(400).min(1200) / 150;
    let next = match ms {
        ..400 => 400,
        400..1600 => ms + 150 - (ms - 400) % 150,
        1600.. => ms + 50 - (ms - 1600) % 50,
    };
    (owed + ms.saturating_sub(1600) / 50, next)
}

/// Panics for reversed bounds, an out-of-range initial value, or a nonpositive step.
pub fn stepper<A: Host, K: Ident<A>>(cx: &Cx<'_, A>, p: Stepper<'_, K>) -> Node<Intent<A>> {
    let ok = p.min <= p.initial && p.initial <= p.max && p.step > 0;
    assert!(ok, "stepper: invalid range, initial value or step");
    let state = cx.state(
        &p.key,
        || (p.initial, Held::Released),
        |s: &mut (i64, Held), _: &K, Ev(what, delta, min, max): Ev, app: &A| {
            let held = match (what, s.1) {
                (Do::Hold, _) => Held::Since(app.now(), 0),
                (Do::Repeat, held) => held,
                (Do::Release, _) => Held::Released,
            };
            let Held::Since(at, done) = held else {
                s.1 = Held::Released;
                return Act::cancel_wake();
            };
            let (owed, next) = ticks(app.now().since(at).as_millis());
            let steps = i64::try_from(owed - done).expect("a hold owes few steps");
            s.0 =
                s.0.saturating_add(delta.saturating_mul(steps))
                    .clamp(min, max);
            s.1 = Held::Since(at, owed);
            // At the bound the button is disabled and hears no release: the hold ends here.
            match s.0 == if delta > 0 { max } else { min } {
                true => {
                    s.1 = Held::Released;
                    Act::cancel_wake()
                }
                false => {
                    let due = Time(at.0 + Duration::from_millis(next as u64));
                    Act::wake(due, Ev(Do::Repeat, delta, min, max))
                }
            }
        },
    );
    let (th, sender, (min, max)) = (p.theme, state.sender(), (p.min, p.max));
    let button = |plus, enabled, delta, symbol, verb| {
        let (sender, id) = (sender.clone(), WidgetId::of(&(&p.key, plus)));
        let ev = move |what| sender.send(Ev(what, delta, min, max));
        let ink = match enabled {
            true => ink(th.type_scale.label, th.colors.text, Weight::BOLD),
            false => ink(th.type_scale.label, th.colors.text_disabled, Weight::BOLD),
        };
        let style = Style::row().padded_xy(th.spacing.md, th.spacing.sm);
        let (face, a11y) = (
            panel(style, raised(th), vec![text(symbol, ink)]),
            A11y::named(Role::Button, format!("{} {verb}", p.label)),
        );
        // Enter/Space is a press and release at once.
        let keys = Intent::All(vec![ev(Do::Hold), ev(Do::Release)]);
        let on = Bindings::activate(keys).on_pointer(move |at: PointerAt| match at.phase {
            Ph::Press => ev(Do::Hold),
            Ph::Drag => ev(Do::Repeat),
            Ph::Release | Ph::Cancel => ev(Do::Release),
        });
        match enabled {
            true => interactive(id, on, a11y, face),
            false => disabled(id, a11y, face),
        }
    };
    let value = text(
        format!("{}: {}", p.label, state.0),
        ink(th.type_scale.label, th.colors.text, Weight::NORMAL),
    );
    let (minus, plus) = (
        button(false, state.0 > p.min, -p.step, "−", "decrease"),
        button(true, state.0 < p.max, p.step, "+", "increase"),
    );
    row([minus, value, plus])
        .gap(th.spacing.sm)
        .align_items(kit_view::taffy::AlignItems::CENTER)
}
