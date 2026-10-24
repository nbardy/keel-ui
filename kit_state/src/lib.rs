//! kit_state: a React-replacing state microlib above `kit_view`, renderer-agnostic.
//!
//! A component is a plain function `fn(&Cx<App>, props) -> Node<Intent<App>>`. State, behaviour, selectors and UI
//! live in that one function:
//!
//! ```text
//! let open = cx.state(&PanelKey, || Disclosure::CLOSED, |d, _, ev: DisclosureEv, app| { d.step(ev, app.now()); Act::none() });
//! ```
//!
//! There are no lifecycle traits, no dependency arrays and no hook-order rules. Identity comes from DATA keys
//! (`Ident`), not call order; a key's `life` is asked after every event and is the only collector (`Over` removes
//! the state, `Leaving` freezes it, intents to either are refused as `Late` and counted). A reducer's declaration
//! identity is its closure TYPE, so it must not capture (enforced at compile time, see [`Cx::state`]).
//!
//! Time is an input: `Host::now` is the only clock, so tests replay.

mod act;
mod anim;
mod cx;
mod ident;
mod input;
mod intent;
mod presence;
mod store;
mod text;

pub use act::{Act, Fault, Wake};
pub use cx::{Cx, Reducer, Sender, State, Zoom};
pub use ident::{Host, Ident, Life, Part};
pub use input::{Drag, Interaction, Unrouted};
pub use intent::{Intent, Late, LateWhy, dispatch, wake_due};
pub use presence::{Disclosure, DisclosureEv, Easing, Phase, Presence, Since};
pub use store::{LATE_LOG, Store};
pub use text::{Applied, Dir, History, Rev, Selection, TextValue, UNDO_DEPTH};
