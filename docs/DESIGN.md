> Written while building the kit inside the Keel game (a private repository); paths outside this repo, and
> links to other notes, refer to that game.

# Keel UI: the outer API, the libraries under it, and why not the alternatives

Date 2026-10-23. Status: CURRENT. Index: [README.md](README.md). Companion: [KEEL_UI_CODEBASE.md](KEEL_UI_CODEBASE.md).

Legend used throughout: **BUILT** = exists in `keel/crates/kit/*` as read on 2026-10-23. **PLANNED** = designed here or in
FINAL.md, not in the tree. Anything not marked is a rule or a claim, not code.

## 1. Thesis, laws, decisions

**Thesis.** A screen is a plain function `fn(&Cx, props) -> Node<Msg>` that returns a value tree. State, behaviour,
derived values, server data and layout intent live together in that one function. State is declared inline where it is
used and stored by data key, not by call order. Everything a press can do is a value (`Intent`) that carries its own key
and its own (zero-sized) reducer, so events stay data and can be logged and replayed. The server is the only authority:
every command ends in a typed verdict, and the screen may preview the server's own rule but never override it.

### The laws

| # | Law | One-line test |
|---|---|---|
| L1 | **Components are functions of `(&Cx, props)` returning a value tree.** No lifecycle traits, no dependency arrays, no hook-order rules. | A new widget is a `fn`; it adds no trait impl. |
| L2 | **Events are data.** An `Intent` is `{key, event, zero-sized reducer}` or an `Ask`; building one never runs anything. | `Debug`-print every intent a press produced. |
| L3 | **A key owns its life, and that is the only collector.** `Ident::life -> Live \| Leaving(t) \| Over` is asked after every event. `Over` removes state (and, once the ledger exists, withdraws unsent commands, aborts loads); anything addressed to a dead key is refused as `Late`, counted, visible. | Delete a row from the model; its state, drafts and pending edits vanish with no feature code. |
| L4 | **Every command ends.** `Confirmed`, `Refused(why)`, `Unknown`, or `Withdrawn`, decided by the server's processed-up-to watermark (protocol P1) and a stale fence (P3). | No row is "pending forever". |
| L5 | **A preview informs; committed state gates.** The preview is the server's own rule (`game_bships::economy::apply`) folded over the commands still in doubt. A control is disabled only by committed state. | Refused purchase: the ghost disappears, nothing was ever awarded. |
| L6 | **Reading is subscribing; time is an input.** `cx.read(fetch)` is how a screen sees a resource and keeps it loaded; `Host::now` is the only clock. | Close a panel; its fetch aborts without cleanup code. |

Plus the repo style, "One Clean Path": kinds are sum types, one dispatcher, one handler per variant, no silent
fallbacks. In this API that shows up as `Life`, `Phase`, `Answer` and `Gate` being closed enums with no default arm.

### Decisions made with the owner after FINAL.md

These override FINAL.md. They are listed once, here.

1. Components are plain functions; no `Machine` trait, no `state!`/`registry!` macros, no separate `impl` blocks.
2. State is declared inline: `cx.state(&key, init, |s, key, ev, app| ...)`. The closure must not capture (compile-time
   `const { assert!(size_of::<F>() == 0) }`). Declaration identity is the closure's `TypeId`; storage is keyed by
   `(TypeId, data key)`; the closure rides inside the `Intent`.
3. `cx.derive(&key, |app, key| ...)` follows the same rule and is recomputed every frame. Memoise only where profiling
   demands it.
4. `cx.read(resource)` is subscribing/loading; `cx.track::<Command>(key)` is the server's verdict (both BUILT 2026-10-25; differences below).
5. Kept from FINAL: key life, withdraw/abort/refuse-late from that one rule, every command ends with a watermark (P1)
   and stale fence (P3), preview = the server's rule over in-doubt commands (informs, never gates), presence as data.
6. Deferred: the full event log and timeline, logged demand, the memo engine, the 44-name vocabulary (target about 15).
7. Stay on our kit rather than egui or iced; use libraries' own types directly; delete mirror types and swap seams;
   accessibility and screen readers are out of scope (owner does not want them).
8. Verified bug to fix in the same program: `Client.sequence` restarts at 0 on reload while the server's
   `applied_ship` persists (see §9, R3).

## 2. The public API

### 2.1 What a screen author meets

About 15 names, grouped by job. BUILT unless marked.

| Job | Names |
|---|---|
| Context | `Cx<'a, A>` (`app()`, `now()`, `state`, `derive`; PLANNED `read`, `track`) |
| State | `State<..>` handle (derefs to the value; `send(ev) -> Intent`; `zoom`), `Sender` |
| Identity and life | `Ident<App>` trait (`life`), `Life::{Live, Leaving(Time), Over}`, `Host` (the app model's `now` and `ask`) |
| Events | `Intent<A>::{Step, Ask, All}`, `Act` (`none`, `ask`, `fault`, `and`), `Late`, `LateWhy` |
| Time and presence | `Disclosure` + `DisclosureEv`, `Presence { enter, exit }`, `Phase::{Entering(f32), Shown, Exiting(f32), Gone}`, `Easing` |
| Server verdicts (PLANNED) | `Answer::{Confirmed, Refused, Unknown, Withdrawn}`, `Gate::{Open, Likely, Closed}` |
| View values (kit_view) | `Node<Msg>`, builders `row column stack panel text image shape spacer scroll text_field layer clipped world overlay interactive disabled blocker nothing`, `Screen`, `Style` |
| Widgets (kit_widgets) | `button`, `labeled_button`, `card`, `caption`, `panel`, `well`, `space`, `gauge` (only what `hud.rs` uses survives the slimming) |
| Routing | `Router::route -> Routing::{Handled(msgs), Captured, Passed}` (host side, not a screen concern) |

### 2.2 `Cx`

```rust
// BUILT: keel/crates/kit/kit_state/src/cx.rs
pub struct Cx<'a, A: Host> { app: &'a A, store: &'a Store<A> }
impl<'a, A: Host> Cx<'a, A> {
    pub fn app(&self) -> &'a A;
    pub fn now(&self) -> Time;                        // the only clock: A::now()
    pub fn state<S, K, E, F>(&self, key: &K, init: S, step: F) -> State<A, S, K, E, F>
    where S: Clone + 'static, K: Ident<A>, E: Clone + Debug + 'static,
          F: Fn(&mut S, &K, E, &A) -> Act<A::Ask> + Copy + 'static;      // ZST enforced by const assert
    pub fn derive<K, T, F>(&self, key: &K, f: F) -> T
    where F: Fn(&K, &Cx<'_, A>) -> T + Copy + 'static;                    // ZST enforced; recomputed every call
}
// PLANNED (kit_state, ~200 more lines):
//   pub fn read<F: Fetch>(&self, f: &F) -> Loaded<'a, F>;                 // reading = subscribing/loading
//   pub fn track<Q: Booked>(&self, key: &Q::Key) -> Track<'a, Q>;         // the server's verdict for this key
```

`state` semantics, all tested in `kit_state/tests/behaviour.rs`:

- `init` runs every frame but is stored only the first time a key is seen **while Live**. A key that is `Leaving` or
  `Over` on first sight is shown with `init` and never stored, so nothing can leak.
- The reducer closure's type **is** the declaration. One closure used for N keys is one declaration with N states; two
  closures with identical bodies are two declarations.
- A reducer returns `Act`: asks for the app, and counted faults. There is no other effect channel.
- `send(ev)` only builds an `Intent`. `dispatch(app, store, intent)` applies it, forwards asks to `Host::ask`, then runs
  `Store::collect` (asks each stored key for its life; `Over` is removed).

### 2.3 `Ident`, `Life`, and what "late" means

```rust
pub trait Ident<App>: Hash + Eq + Clone + Debug + 'static { fn life(&self, app: &App) -> Life; }   // no default
pub enum Life { Live, Leaving(Time), Over }
```

- `Live`: state stored, intents accepted.
- `Leaving(t)`: state kept (so an exit animation has data), intents refused as `Late(Leaving)`.
- `Over`: state removed at the next collect; intents refused as `Late(Gone)`.
- A refused intent is pushed to `Store::late()` with the `Debug` of the intent. A host turns that into a toast or a
  fault counter; nothing is dropped silently.

Keys are small data types whose `life` reads the app model: `TrayKey { boat, turn }` is `Live` while the plan turn is
current; `RowKey { list, id }` is `Leaving` while the server's list says it left; `DockKey { session }` is `Over` at
logout. The key table in FINAL.md §2.4 is still the right list of game keys.

### 2.4 Intent and Act

An `Intent` is what a node's binding carries as its `Msg`:

```rust
pub enum Intent<A: Host> { Step(Rc<dyn ErasedStep<A>>), Ask(A::Ask), All(Vec<Intent<A>>) }
```

`Step` holds `{key, event, F}` where `F` is the zero-sized reducer: no heap closure, `Clone` is an `Rc` bump, `Debug`
prints `Step(key, event)`. Because the reducer travels with the intent, the router needs no registry lookup and a test
can run an intent without a screen.

### 2.5 Node, builders, Screen, Router

Unchanged in spirit from the kit as built (see [KEEL_UI_CODEBASE.md](KEEL_UI_CODEBASE.md) §2): `Node<Msg>` is plain data
rebuilt every frame; builders are free functions; `Screen` gives corner/edge regions with safe area over a taffy grid;
`Router::route(&LaidOut, &mut UiMemory, InputEvent, Time) -> Routing<Msg>` answers every event with `Handled`,
`Captured` or `Passed` so the host never re-walks the tree to ask who owns a press. `kit_state` never names `Node`:
`Msg = Intent<A>` and the host maps `Routing::Handled(intents)` into `dispatch`.

### 2.6 Disclosure and Presence

Presence is a function of data, not an unmounted-but-alive node.

```rust
pub enum Disclosure { Open { since: Since }, Closed { since: Since } }    // BUILT; remembers WHEN it changed
pub struct Presence { pub enter: Duration, pub exit: Duration }
impl Presence { pub fn phase(&self, d: Disclosure, now: Time) -> Phase }  // Entering(x) | Shown | Exiting(x) | Gone
```

A closed panel is still a value whose `since` is recent, so `phase` returns `Exiting(0.4)` and the view draws it
inert; once the exit elapses it is `Gone`. For rows that leave a server list the same effect comes from
`Life::Leaving(t)`. Reduced motion is defined per phase: enter becomes `Shown` at once, exit holds `Shown` until `Gone`
(regression: a zero-length tween was "done" before it started, `kit_widgets/float.rs:91`; test
`zero_duration_presence_is_shown_and_gone_immediately_and_never_nan`).

## 3. A complete small example: the todo list, inline-closure style

The server owns the list. The client only asks. This ports `EXAMPLE-todo.md`. BUILT parts compile against `kit_state`
today (see `kit_state/tests/behaviour.rs::todo_component_renders_dispatches_and_rerenders` for a smaller real one).
Lines marked `// PLANNED` need the ledger or `read`/`track`.

```rust
// The shared rule: server and client call the same function (game crates own this; unchanged from EXAMPLE-todo §1).
pub fn apply<I: PartialEq + Clone>(l: &mut TodoList<I>, cmd: TodoCmd<I>, mint: I) -> Result<(), TodoRefusal>;

// The app model the screen runs against. Host supplies the clock and the sink for asks.
impl Host for App { type Ask = TodoAsk; fn now(&self) -> Time { self.clock } fn ask(&mut self, a: TodoAsk) { self.outbox.push(a) } }

// Keys: data identity that says how long it lives (L3).
#[derive(Clone, PartialEq, Eq, Hash, Debug)] struct ListKey { session: SessionNo }
#[derive(Clone, PartialEq, Eq, Hash, Debug)] struct RowKey { list: ListKey, id: TodoId }
impl Ident<App> for ListKey { fn life(&self, app: &App) -> Life { live_if(app.session == self.session) } }
impl Ident<App> for RowKey {
    fn life(&self, app: &App) -> Life {
        match app.listing(&self.list, self.id) {
            Listing::Here => Life::Live,
            Listing::Left { at } => Life::Leaving(at + ROW_EXIT),
            Listing::Absent => Life::Over,
        }
    }
}

enum ComposerEv { Edit(Splice), Submit }
enum EditEv { Begin, Edit(Splice), Commit, Cancel }

fn todo_list(cx: &Cx<App>, list: &ListKey) -> Node<Intent<App>> {
    // BUILT: reading is subscribing. Not reading it (panel closed) unsubscribes.
    let feed = cx.read(&TodoFeed(list.clone()));                       // Loaded::{Loading, Ready, Failed}

    // BUILT: state declared where it is used; the closure is the declaration; it captures nothing.
    let composer = cx.state(list, Composer::EMPTY, |c: &mut Composer, list, ev: ComposerEv, _app| match ev {
        ComposerEv::Edit(s) => { c.field.splice(s); Act::none() }
        ComposerEv::Submit => match Text::parse(c.field.text()) {
            Ok(text) => { c.sent = Sent::At(c.field.rev()); Act::ask(TodoAsk::Add { list: list.clone(), text }) }
            Err(_) => Act::fault("submit with an invalid draft"),       // the Add button was disabled; a hotkey got here
        },
    });
    let filter = cx.state(list, Show::All, |s: &mut Show, _, ev: Show, _| { *s = ev; Act::none() });

    // BUILT: derived value, recomputed every frame, same non-capturing rule.
    let shown = cx.derive(list, |list, cx| preview_list(cx.app(), list));   // committed list + in-doubt asks (L5)

    // BUILT (host-reported, no ledger): the verdict for the last add.
    let verdict = cx.track::<AddTodo>(list);

    column![
        composer_row(&composer, &add_gate(cx.app(), &shown, composer.field.text())),
        verdict_line(&verdict),
        column(shown.items.iter().filter(|t| filter.admits(t)).map(|t| row_view(cx, list, t))),
        footer(&filter, &shown),
    ].into()
}

fn row_view(cx: &Cx<App>, list: &ListKey, t: &Todo<ShownId>) -> Node<Intent<App>> {
    match t.id {                                                         // one dispatch on the id's TYPE (L3)
        ShownId::Pending(_) => ghost_row(t),                              // no TodoId, so no controls: cannot toggle what the server has not heard of
        ShownId::Saved(id) => {
            let key = RowKey { list: list.clone(), id };
            let edit = cx.state(&key, RowEdit::Viewing, |e: &mut RowEdit, key, ev: EditEv, app| match (&mut *e, ev) {
                (RowEdit::Viewing, EditEv::Begin) => { *e = RowEdit::Editing(TextValue::of(app.shown_text(key))); Act::none() }
                (RowEdit::Editing(f), EditEv::Edit(s)) => { f.splice(s); Act::none() }
                (RowEdit::Editing(f), EditEv::Commit) => {
                    let verb = match Text::parse(f.text()) { Ok(t) => RowVerb::Rename(t), Err(_) => RowVerb::Delete };
                    *e = RowEdit::Viewing;
                    Act::ask(TodoAsk::Row(RowOrder { row: key.clone(), verb }))
                }
                (RowEdit::Editing(_), EditEv::Cancel) => { *e = RowEdit::Viewing; Act::none() }
                (RowEdit::Viewing, _) | (RowEdit::Editing(_), EditEv::Begin) => Act::fault("edit event in the wrong state"),
            });
            // When the server drops this row: life = Leaving -> drawn inert while it fades -> Over -> `edit` is collected,
            // and an unsent rename addressed to it is refused as Late (PLANNED: also withdrawn from the socket queue).
            saved_row(t, &edit, cx.track::<RowOrder>(&key))
        }
    }
}
```

**BUILT: `cx.track` and `cx.read` (2026-10-25), and how they differ from the plan above.**
- `cx.track::<C>(key)` takes a `Command` marker type (`Why`, `WITHIN`), not the ask. The send is marked by
  `Act::ask_tracked::<C>(ask)` and recorded under the REDUCER's own key; the host reports the server's answer with
  `Store::settle`. `Verdict` is `Unsent | Pending | Confirmed | Refused(why) | Unknown`. No `Withdrawn`, no ledger,
  no watermark: the kit queues nothing, so there is nothing to withdraw, and `Unknown` is derived at read time from a
  deadline (no timer), with `redraw_at(deadline)` registered by the view that shows it.
- Flaw found in the plan: a verdict keyed only by (kind, key) cannot tell which of two in-flight sends an answer is
  for. Chosen: replies are matched to sends by order (one connection); only the reply to the newest send shows, the
  others return `Settled::Superseded`. An out-of-order server needs a key per send (`Part(key, n)`).
- A verdict for an ended key (`Gone`/`Leaving`) or with no send awaiting it (`Unsent`) is `Err(LateWhy)` AND counted in
  `Store::late()`.
- `cx.read(&R)`: `R: Resource` supplies `state(&app) -> Loaded<T, E>`, `want()` and `release()` asks. The fetch is the
  host's; the app holds the value. `sync_reads(app, store)` after each view build asks `want` for new reads and
  `release` for resources no view read that frame. It is not tied to a key life other than the resource key's own.

What the author did **not** write: a loading flag, an effect cleanup, an abort controller, a rollback, an "is this row
still there" check, a sequence number. Those are the laws' job.

## 4. A game example: the captain plan tray (E1)

Drag a maneuver token into one of N slots; per-slot gun toggles; the server validates the plan; each slot shows
Pending/Confirmed/Refused; a drag begun in an earlier turn is inert.

```rust
#[derive(Clone, PartialEq, Eq, Hash, Debug)] struct TrayKey { boat: BoatId, turn: PlanTurn }
impl Ident<App> for TrayKey {
    // Live only while this turn is current and I still captain this boat. A moved turn is Over: the draft, a drag in
    // flight and any unsent plan edit die together.
    fn life(&self, app: &App) -> Life { live_if(app.plan_turn(self.boat) == Some(self.turn)) }
}

enum TrayEv { Drop { slot: u8, token: PlanToken }, Gun { slot: u8, side: Side }, Clear { slot: u8 } }

fn plan_tray(cx: &Cx<App>, key: &TrayKey) -> Node<Intent<App>> {
    let tray = cx.state(key, PlanTray::EMPTY, |t: &mut PlanTray, key, ev: TrayEv, _app| match ev {
        TrayEv::Drop { slot, token } => { t.set(slot, token); Act::ask(Ask::Plan(key.clone(), t.to_plan())) }
        TrayEv::Gun { slot, side }   => { t.toggle(slot, side); Act::ask(Ask::Plan(key.clone(), t.to_plan())) }
        TrayEv::Clear { slot }       => { t.clear(slot); Act::ask(Ask::Plan(key.clone(), t.to_plan())) }
    });
    // The server's visible plan for this turn, folded with the edits still in doubt (display fold, L5).
    let visible = cx.derive(key, |key, cx| visible_plan(cx.app(), key));
    // PLANNED: per-slot verdicts from the ledger; Pending / Confirmed / Refused(OrderRefusal) / Unknown.
    let verdict = cx.track::<PlanOrder>(key);

    row((0..SLOTS).map(|slot| {
        let v = verdict.slot(slot);
        slot_tile(slot, visible.token(slot), v)                                  // Refused shows its reason; Pending shows a spinner
            .drop_target({ let tx = tray.sender(); move |token| tx.send(TrayEv::Drop { slot, token }) })   // builds an Intent; runs nothing
    })).into()
}
```

The shop (E2) follows the same shape: `cx.derive(&ShopKey, ..)` computes "gold after pending purchases" by running
`game_bships::economy::apply(Awards::Withhold)` over the in-doubt asks (a pending sell frees a slot but credits no gold,
because presentation may not award gold); a second fold with `Awards::Grant` yields `Gate::Likely(why)` badges.
`Gate::Closed` appears only when committed state, with nothing in doubt, says the server would refuse. The full sketches
are FINAL.md E1 (§4) and E2; they remain valid except for the declaration syntax.

## 5. The state layers (summary)

Four layers, each with exactly one writer. Detail and the data-flow diagram are in KEEL_UI_CODEBASE.md §3.

| Layer | Writer | Screens see it as |
|---|---|---|
| Server | validated commands | nothing directly; only what is published, fog-redacted per team |
| Client replica (`game_client::Client`) | `on_message` / delivery | `cx.app()` reads, `cx.read` (BUILT) |
| Prefs | one `PrefChange` ask | derived theme/keymap in the app |
| UI state | `kit_state::Store` via `dispatch`, plus `UiMemory` for presentational hover/press/scroll | `cx.state` handles |

## 6. Libraries: what we use, what we expose, what we do not wrap

Rule: a library type appears in our API when it is already the right vocabulary. We wrap a library only to add a
property it lacks (a bound, a unit conversion, a typed error), and the wrapper is then smaller than the property.

| Library | Used for | Types we expose directly | Deliberately not wrapped | Status |
|---|---|---|---|---|
| `taffy` | Flex/grid layout | `taffy::Style` on `Node` (target; today a `kit_view::Style` mirror with a 197-line translation) | No `LayoutEngine` trait; `TaffyLayout` is concrete. We keep `place_anchored` (safe-area anchor), `check_cells` (taffy ignores `grid_row` on a flex child) and one percent constructor (kit 0..=100 vs taffy 0..=1) | mirror BUILT; direct use PLANNED (audit step 4) |
| `cosmic-text` | Shaping, bidi, line breaking, fallback, text editing (`Editor`) | `Attrs`, `Metrics`, `Buffer`, `Cursor`, `LayoutRun`, `CacheKey` | No `TextSystem` swap seam (parley is not a plausible swap; five impls existed only for tests). Keep `resolve_segments` (cosmic `add_span` replaces whole Attrs, ours merges) and a bounded raster cache (SwashCache is unbounded) | BUILT behind a seam; direct types PLANNED (step 5) |
| `swash` | Glyph rasterisation | `CacheKey`-driven masks via `CosmicText` | Renderers never load fonts: glyph pixels come only from the text system | BUILT |
| `kurbo`, `peniko` | Geometry, colour, brushes | `Rect`, `Point`, `Affine`, `BezPath`, `Color`, `Brush` (re-exported by `kit_core`) | Nothing | BUILT |
| `lyon` | Path/stroke tessellation | none | Internal to `kit_wgpu` (`tess.rs`) | BUILT |
| `etagere` | Atlas rectangle allocation | none | Replaces our shelf packer inside `kit_wgpu` | dependency present in `kit_wgpu/Cargo.toml` |
| `wgpu` | GPU | `CommandEncoder` / target in the render call; host owns device, surface, pass order | We never own the device or the loop; UI records into the game's encoder (`Renderer::render_with`) | BUILT |
| `tiny-skia` | CPU reference raster for tests | `Pixmap` as the CPU render target | Never shipped to players | BUILT (dev-only) |
| `winit`, `web-sys` | Events, canvas | translated to `InputEvent` in hosts | Not in kit crates | BUILT in hosts |
| `usvg` | SVG icons | none | Icons are `Icon`/`PathEl` ops today (converted `keel_ui` ops); `usvg` only if artist SVGs arrive | not adopted |
| `accesskit` | Screen readers | none | Out of scope by owner decision. The web semantics mirror stays only as a **test surface** (`kit_mirror`), not as an accessibility feature | not adopted |
| `egui` / `iced` / `vello` | - | none | Not dependencies. egui was considered for devtools only; vello only if the GPU renderer is ever replaced | not adopted |

## 7. How this compares

Each entry: what it gets right, where it fails for a server-authoritative 60 Hz game, what we take, how we go past it.
Evidence is linked; a claim with no link is the author's, not a sourced fact.

### 7.0 Lessons taken, at a glance (moved here from the kit README, 2026-10-24)

| Lesson | From | In the kit | Status |
|---|---|---|---|
| Identity comes from data ids, not tree position | Om Next idents | `cx.state(&key, ..)` keyed by data; no hook-order rules | built |
| State is declared locally but stored centrally; retention is an explicit choice | Bonsai (`scope_model`) | one store keyed by (declaration, key); the key's type decides how long state lives | built |
| A key's end cleans everything attached to it | Bonsai, XState, DBSP | `Ident::life` collects state and refuses late input | built (unsent commands and loads: designed) |
| Events and effects are data | Elm, re-frame | `Intent`/`Act` values; reducers are non-capturing closures carried inside the event | built |
| A mutation has a local and a remote half; the server's answer is merged by identity | Om Next / Fulcro | every command ends Confirmed / Refused / Unknown / Withdrawn; previews run the server's own rule over what is still in doubt and only committed state disables a control | designed, not built |
| No silent fallbacks | (Om Next's parser returned `:not-found`) | closed enums, exhaustive matches, typed errors | built |

Common context: the HUD rebuilds and lays out in about 23 us (58 nodes) per frame today, game state changes every
frame, the server is the only writer of game facts, and the client must show commands that may be refused.

### 7.1 React and hooks

- **Right:** state declared where it is used (`useState`/`useReducer`); components are functions; `key` resets state.
- **Fails here:** hook order is identity, so a hook in a branch or loop is a bug; dependency arrays are a hand-kept
  invalidation list; a lost or stale `useEffect` is the usual source of leaks; no notion of "the server refused this".
  `useOptimistic`-style previews are ad hoc per feature.
- **We take:** locality (`cx.state` beside the view), `key`-reset semantics, `use()`'s lesson that a resource needs a
  stable identity (our fetch key).
- **We go past:** identity is a data key plus a declaration `TypeId`, so loops and branches are free; no dependency
  arrays at all (derive recomputes per frame); one `life` rule collects what React spreads over unmount, effect cleanup
  and Query GC; commands have a ledger with typed endings.
- Detail: [elm-react-modern.md §2](../ui-pattern-designs-2026-10-23/research/elm-react-modern.md); d5 critique-4 (React lens):
  [critique-4.md](../ui-pattern-designs-2026-10-23/d5-event-log/critique-4.md).

### 7.2 Elm (TEA)

- **Right:** purity; messages are data; effects are data; subscriptions are a function of state so removal cancels.
- **Fails here:** one global `update` and nested-TEA "lifting" tax; every popup needs a field in the model and a variant
  in `Msg`; no local-state story; no server-authority concept.
- **We take:** `view -> Node<Msg>` rebuilt per frame; typed messages; time as an input; `Sub` diffing as the model for
  `cx.read`.
- **We go past:** state is declared at the use site and routed by data key, so there is no lifting; the reducer closure
  rides in the intent, so no giant `Msg` enum is needed for local state (the `Ask` enum covers only what leaves the UI).
- Detail: [elm-react-modern.md §1](../ui-pattern-designs-2026-10-23/research/elm-react-modern.md); Elm lens critiques
  [d1 critique-1](../ui-pattern-designs-2026-10-23/d1-authority-machines/critique-1.md),
  [d5 critique-1](../ui-pattern-designs-2026-10-23/d5-event-log/critique-1.md).

### 7.3 Om Next and Fulcro

- **Right:** identity from data (`[table id]`); mutations are data with a local and a remote half; loading state is
  data; UI state keyed by entity ident dies with the entity.
- **Fails here:** co-located static queries force a leaf's load up through every ancestor; an unknown query key returns
  `:not-found` (a silent fallback); no rollback log for optimistic actions (marked UNVERIFIED in the brief); nothing
  models reconnect or a stage change.
- **We take:** data-keyed idents (the `Ident` trait), mutation-as-data (`Intent`/`Ask`), UI state that dies with its
  entity (`life`).
- **We go past:** keys carry their own epoch/life (turn, encounter, session), so a stage change is an `Over`; game facts
  are pessimistic by law and the preview is the server's rule, so there is nothing to roll back.
- Detail: [omnext-fulcro-reframe.md](../ui-pattern-designs-2026-10-23/research/omnext-fulcro-reframe.md);
  [d2 round-5](../ui-pattern-designs-2026-10-23/d2-normalized-queries/round-5.md) (the normalized-db design),
  [d1 critique-2](../ui-pattern-designs-2026-10-23/d1-authority-machines/critique-2.md) (Om lens).

### 7.4 re-frame

- **Right:** events and effects as data; coeffects (time, storage) as injected inputs; interceptors; subscription graph.
- **Fails here:** one flat app-db value and string-like event keys give no type-level state-machine structure; the
  subscription graph is a runtime-built memo layer that a 23 us HUD does not need.
- **We take:** effects as returned data (`Act`), time as an input (`Host::now`).
- **We go past:** typed events per declaration; no global handler registry (the closure is the handler).
- Detail: [omnext-fulcro-reframe.md §4](../ui-pattern-designs-2026-10-23/research/omnext-fulcro-reframe.md);
  [d4 round-0](../ui-pattern-designs-2026-10-23/d4-effects-as-data/round-0.md) (effects-as-data design, dropped after round 0).

### 7.5 Bonsai and Incremental (Jane Street)

- **Right:** "local declaration, central storage" (hook locality without call-order rules); state-machine primitives;
  named sums such as `Computation_status`; capability-passing (`local_ graph`) instead of ambient effects.
- **Fails here:** a static computation graph built once is the wrong default when inputs change every frame; inactive
  state is kept by default; `Effect` cannot be cancelled in general; `Dynamic_scope` falls back silently.
- **We take:** declare locally, store centrally; identity that is not position; `Cx` is a capability (it cannot be
  stored in a `'static` closure).
- **We go past:** views re-run each frame; only explicit derivations would be memoised; state is deleted when its key's
  life ends (retention is a longer-lived key, chosen in the type).
- Detail: [bonsai-incremental.md](../ui-pattern-designs-2026-10-23/research/bonsai-incremental.md);
  [d3 round-0](../ui-pattern-designs-2026-10-23/d3-static-graph/round-0.md) (the static-graph design, dropped);
  [d5 critique-3](../ui-pattern-designs-2026-10-23/d5-event-log/critique-3.md) (Bonsai lens).

### 7.6 Solid and Svelte signals

- **Right:** fine-grained reactivity means no component re-render; `createResource` ties fetch to reading.
- **Fails here:** a signal graph is the memo engine we are deferring; auto-tracking hides which data a screen depends on
  (hurts replay and tests); effects and cleanup are again manual; no concept of a command that may be refused.
- **We take:** read = subscribe (`cx.read`), as in `createResource`/`onBecomeObserved`.
- **We go past:** at HUD scale rebuilding the tree is cheaper than maintaining a graph; where a derivation is slow we
  would memoise that one function, not the screen.
- Detail: [elm-react-modern.md §3-4](../ui-pattern-designs-2026-10-23/research/elm-react-modern.md).

### 7.7 Xilem (and Masonry)

- **Right:** closest to our model: a view tree of values, `memoize`, Masonry's focus/pass structure; Linebender
  vocabulary (kurbo, peniko, parley) that we share.
- **Fails here:** pre-1.0; owns its own widget tree and event loop (Masonry), so it cannot be dropped into the game's
  encoder as a library; app state is one `&mut State` with closure callbacks, no server-ledger concept.
- **We take:** value trees; `lazy`-style memoisation as a later option; kurbo/peniko types.
- **We go past:** typed intents with data keys instead of closures over `&mut State`; the router works on a laid-out
  data tree, not on widget objects.
- Detail: [elm-react-modern.md §6](../ui-pattern-designs-2026-10-23/research/elm-react-modern.md);
  [END_STATE §3 and §11](../KEEL_UI_KIT_END_STATE_2026-10-22.md).

### 7.8 iced

- **Right:** Elm architecture in Rust with years of use; wgpu integration example; `view` functions port nearly line for
  line.
- **Fails here:** `Widget` trait (layout/draw/on_event method sets) for every custom widget, which is the lifecycle-trait
  surface the owner rejects; frameworks own the loop; no official mobile; global `Message` enum; costly custom widgets
  for a game look.
- **We take:** the model; iced remains the migration target if the kit is abandoned (END_STATE §11).
- **We go past:** new widgets are plain functions; the host owns the loop and device; local state needs no `Message`
  variant.
- Detail: [END_STATE §11](../KEEL_UI_KIT_END_STATE_2026-10-22.md) comparison table; [PLAN §1](../KEEL_UI_KIT_PLAN_2026-10-22.md).

### 7.9 egui (immediate mode)

- **Right:** nothing to synchronise; state is just locals; best developer ergonomics for tools; egui_kittest exists; its
  `web/text_agent.rs` and scroll physics are good prior art.
- **Fails here:** tool-looking default, costly restyle; immediate-mode hit-testing is one frame late; widget results
  are `Response` objects, not typed commands a server validates; its renderer owns its own pass; no CPU/GPU parity
  reference; state in `Memory` is keyed by a hashed id with no collector tied to server data.
- **We take:** immediate-feeling API (builders in a function), the text-bridge and scroll ideas (copied with licence
  headers when needed), the idea of a test driver.
- **We go past:** a press yields a typed `Intent`/`Msg`, a test asserts it without a frame; UI draws in the game's
  encoder; CPU-vs-GPU parity found three real GPU bugs; state has a life.
- Detail: [elm-react-modern.md §7](../ui-pattern-designs-2026-10-23/research/elm-react-modern.md);
  [END_STATE §11](../KEEL_UI_KIT_END_STATE_2026-10-22.md).

### 7.10 Dioxus and Leptos

- **Right:** Rust components that are functions; Dioxus uses taffy and hooks; Leptos has fine-grained signals and server
  functions.
- **Fails here:** both are DOM/webview-first renderers (native renderers exist but are not our encoder), hook order
  (Dioxus) or signal graphs (Leptos) bring the problems in 7.1 and 7.6; neither models a server-authoritative command
  ledger. Not studied in the briefs beyond this level; the comparison is from general knowledge and is the weakest in
  this section.
- **We take:** component-as-function in Rust; taffy as layout.
- **We go past:** data-keyed identity; draws in our encoder; one wasm module, one renderer.
- Detail: none in `research/`; nearest is [END_STATE §11 "HTML + webview overlay"](../KEEL_UI_KIT_END_STATE_2026-10-22.md).

### 7.11 Why this is better for this game (summary)

| Game need | What gives it | Who does not |
|---|---|---|
| Server may refuse any command | Ledger endings + preview as the server's own rule + "informs, never gates" | All of the above treat optimism as a per-feature choice |
| State must not outlive its subject (turn, encounter, row, login) | One `life` rule | Hooks: unmount; Fulcro: manual; egui: id hash never collected |
| 60 Hz, game state changes every frame | Rebuild every frame; derive without memo; no graph | Signals/Incremental pay for a graph the HUD does not need |
| UI inside the game's frame, same submit and depth target | Renderer records into the host's encoder | egui-wgpu/iced own a pass; DOM overlays are separate |
| Tests without a GPU or fonts, plus real-pixel proof | Label-based mirror, CPU-vs-GPU parity, goldens | egui_kittest is egui only |
| Small | ~7.5k lines shipped (audit) | A framework is 10x that |

## 8. What we give up

| Given up | Why acceptable | Cost to reverse |
|---|---|---|
| Screen readers / accessibility | Owner decision. A DOM mirror remains as a test surface | accesskit adapter over `Semantics`; the semantics extraction is kept |
| The event log, timeline, time travel, "what if the server refused" replay | Deferred; intents are already data, which keeps the door open | Log `Intent`s and server frames; FINAL §2.2 and d5 round-5 are the design |
| Memo engine / fine-grained incrementality | HUD is ~23 us per frame; menus are small | Memoise a single `derive` by allocation identity (FINAL §2.8) |
| Compile-time exhaustiveness over all intents | `Intent` erases the reducer behind `Rc<dyn ErasedStep>`; each reducer's own `match` over its event enum is exhaustive, but nothing checks that every `Ask` variant is handled by the host | A host `match` on `Ask` is exhaustive; add one test that dispatches every `Ask` variant |
| Static graph guarantees (Bonsai) | Identity is data + `TypeId`; call order is irrelevant but nothing detects an unreachable declaration | Debug-build probe listing declarations seen per frame |
| Swap seams (renderer, text, layout) | Only one real impl each; five text impls existed for tests | Re-add a trait when a second production impl exists |
| Ecosystem widgets and IME polish | We build only the widgets the HUD uses | Text input via cosmic `Editor` plus the copied eframe text agent when chat/lobby move onto the kit |

## 9. Open risks

| # | Risk | State | What closes it |
|---|---|---|---|
| R1 | **The closure-identity trick** (declaration = `TypeId` of a non-capturing closure) is unproven at scale. The const assert fires at monomorphization, so `cargo check` does not report it; only `cargo build`/`test` do. Closures in generic fns, or identical closures merged by LTO, are untested. `TypeId` of a closure is stable within a build only | kit_state compiles in the tree and its behaviour tests exist (2026-10-23 read); I did not run them (other agents hold the build lock); not yet used by any screen | Run `cargo test -p kit_state`; port the plan tray (audit step 6); build once for `wasm32` and check the declaration count |
| R2 | **Protocol dependency.** L4 needs P1 (processed-up-to `Mark`, ordering law: a refusal is written before any mark covering it) and P3 (encounter/incarnation stale fence). Without them: confirmation falls back to today's `applied` reconcile and a refusal can race the mark (FINAL §3.4 lists each degraded mode) | Not started | Server work in `game_room`; `PROTOCOL` bump; paired native/server/WASM |
| R3 | **`Client.sequence` restarts at 0 on reload** while server `applied_ship` persists. Verified: one-frame prediction rubber-band and plausibly a stale plan echo marking a new edit Saved. Orders are never dropped (server never compares sequences). Fix: monotonic base, e.g. `Seq { boot, n }` or persisted base. Refs: `game_client/src/lib.rs:226`, `room.rs:114,578-583,1020`, `bships_netcode.rs:179-191`. The claim "`delivery.clear()` on Welcome" was **not** a bug | Open | [delivery-bug-verify journal](../../../agent_notes/2026-10-23_delivery-bug-verify.md) |
| R4 | **Hidden-tab timers.** Browsers clamp background timers (Chrome intensive throttling: one wake per minute after five hidden minutes) while heartbeats are every 2 s and answer deadlines are measured from release | Unresolved; needs an owner decision on how long a hidden tab may hold a seat, or a worker timer | FINAL §9 item 4 |
| R5 | **Runtime, not compile-time, intent routing.** An intent to a dead key is caught at dispatch (`Late`), not by the type system; an unhandled `Ask` is caught by the host `match` only | Accepted | Counted in `Store::late()`; a test per screen that no `Late` occurs in a scripted session |
| R6 | **Per-key staleness.** An intent built against an old *value* of a live key is admitted (`Toggle` avoids the commonest case) | Accepted | FINAL §9 item 10 |
| R7 | **Preview and server drift.** Two interpreters of one rule (`economy::apply` on `State` and on a client `Market`). Mitigated because a preview is a badge, never a gate | Open until the shop is ported | Parity test on real `State` (FINAL §6.2 #4) |
| R8 | **Collection cost** is O(stored keys) per event. Fine at tens; not at thousands of keyed rows | Accepted | Collect only tables whose key type's epoch moved |
| R9 | **Slimming risks** (percent units 100x, `place_anchored`, `check_cells`, goldens move on cosmic upgrades) | Listed with mitigations | KEEL_UI_CODEBASE.md §7 |

## 10. Links to the prior investigations

| Topic | Read |
|---|---|
| Original plan and library vetting | [PLAN](../KEEL_UI_KIT_PLAN_2026-10-22.md) §5, §6 |
| Dogfood API journal (what the HUD port taught us) | [DOGFOOD](../KEEL_UI_KIT_DOGFOOD_2026-10-22.md) |
| Kit end state, alternatives table | [END_STATE](../KEEL_UI_KIT_END_STATE_2026-10-22.md) §3, §9, §11 |
| Slimming audit | [SLIM_AUDIT](../KEEL_UI_KIT_SLIM_AUDIT_2026-10-23.md) |
| Study brief and examples E1-E6 | [CONTEXT](../ui-pattern-designs-2026-10-23/CONTEXT.md) |
| Research (sourced) | [Om/Fulcro/re-frame](../ui-pattern-designs-2026-10-23/research/omnext-fulcro-reframe.md), [Bonsai/Incremental](../ui-pattern-designs-2026-10-23/research/bonsai-incremental.md), [Elm/React/modern](../ui-pattern-designs-2026-10-23/research/elm-react-modern.md) |
| Design rounds | [d1 machines](../ui-pattern-designs-2026-10-23/d1-authority-machines/round-5.md), [d2 queries](../ui-pattern-designs-2026-10-23/d2-normalized-queries/round-5.md), [d5 log](../ui-pattern-designs-2026-10-23/d5-event-log/round-5.md), [d3](../ui-pattern-designs-2026-10-23/d3-static-graph/round-0.md), [d4](../ui-pattern-designs-2026-10-23/d4-effects-as-data/round-0.md) |
| Judging | [judge-1](../ui-pattern-designs-2026-10-23/judging/judge-1.md), [judge-2](../ui-pattern-designs-2026-10-23/judging/judge-2.md), [judge-3](../ui-pattern-designs-2026-10-23/judging/judge-3.md) |
| Synthesis and its review | [FINAL](../ui-pattern-designs-2026-10-23/FINAL.md), [FINAL-critique](../ui-pattern-designs-2026-10-23/FINAL-critique.md) |
| Worked example in the old syntax | [EXAMPLE-todo](../ui-pattern-designs-2026-10-23/EXAMPLE-todo.md) |

What to take from FINAL.md unchanged: §2.6 (ledger types), §2.7 (shared rule, `Awards`, `Gate`, `Forecast`), §3.4
(P1-P5 with degraded modes), §5 (composition coverage table; re-read with `cx.state` for `state!`), §6.2 (the 21 tests
that earn their keep), §9 (weaknesses). What to ignore: §1 vocabulary count, §2.2-2.3 (log, model, fold), §2.5
(`Machine`, `state!`), §2.9 (logged demand), §2.8 `Memo`.
