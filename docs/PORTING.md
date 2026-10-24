> Written while building the kit inside the Keel game, where several agents ported components in parallel; the
> "coordinator" below is whoever owns the tree. Commands and paths are this repository's.

# Keel UI: porting an egui component to `kit_widgets` (contract, 2026-10-23)

You port ONE component into ONE file. The infrastructure carries the weight (state, input, focus, popups, text,
scroll, animation), so a component is short. Start from `kit_widgets/src/template.rs` (about 50 lines)
and its test `kit_widgets/tests/components/template.rs`.

**Infrastructure still landing (as of 2026-10-23): overlay + focus (portal, focus manager), text editing core,
scroll core.** Use the API as it exists in the tree when you start; read the crate (`kit_state/src/cx.rs`,
`kit_view/src/builders.rs`, `kit_input/src/router.rs`) instead of trusting this table's names. If the primitive you
need is missing or awkward, STOP and report it; do not rebuild it inside your file and do not edit the crate.

## Keep from egui / drop

| Keep | Drop |
|---|---|
| Interaction logic: what a press, drag, release, double-click, Escape, Enter, arrows do | `Ui`, `Response`, `Sense`, `Memory`, `Context`, `Id` plumbing |
| Math: value-from-pointer mapping, snapping, rounding, clamping, step/range logic, easing constants | `epaint` shapes, `Painter`, `Galley`, tessellation |
| Keyboard behaviour (which key does what, modifiers, repeat, focus order) | Layout plumbing (`allocate_space`, `Layout`, `Placer`, `desired_size`, `Rect` bookkeeping): use taffy `Style` |
| Edge cases and clamps egui added after bug reports (read the git blame of the source) | `Visuals`/`Style` structs: use `Theme` tokens only |
| Names of props that users of egui expect | Immediate-mode idioms (`if ui.button(..).clicked()`) |

## egui concept -> kit concept

| egui | kit |
|---|---|
| `Response::clicked()` / `changed()` | an `Intent` built in the view: `Bindings::activate(state.send(Ev))`; the host dispatches it |
| `Response::hovered()` / `is_pointer_button_down_on()` / `drag_delta()` | `cx.hovered(&key)`, `cx.pressed(&key)`, `cx.drag(&key)` (key = the same data the node is keyed with) |
| `response.interact_pointer_pos()` / `drag_delta()` driving a value (slider, colour square, numeric drag) | `Bindings::none().on_pointer(\|p: PointerAt\| msg)`: Press/Drag/Release/Cancel with `local` (unclamped, relative to the node's laid rect), `size`, `travel` (click vs drag). The pointer is captured: drags beyond the node and the release outside it still arrive; Escape, epoch change, blur and resize send Cancel. These closures MAY capture (only reducers may not). See `slider.rs`. |
| `response.has_focus()` + `ui.input(\|i\| i.key_pressed(..))` (slider arrows, list navigation, type-ahead) | `Bindings::on_key(\|k: KeyPress\| ..)` returning `KeyAnswer::Handled(msg)` or `KeyAnswer::Pass`: delivered only while THAT control has focus, after the router's Tab and Enter/Space (when `activate` is bound), before arrow focus-walking, Escape and hotkeys; `Pass` means "not my key". A key-bound node is focusable. See `slider.rs`, `listbox.rs`. |
| `ui.input(\|i\| i.modifiers)` on a click (shift-click extends, cmd-click toggles) | `Bindings::activate_with(\|m: Modifiers\| msg)` (`activate` stays the unmodified message). A click sees the modifiers hosts report with `InputEvent::Modifiers` (every `Key` event updates them too); Enter/Space sees the key's own. |
| `response.gained_focus()` / `lost_focus()`, Escape in a `TextEdit`, `memory.request_focus(id)` | A field's `on_edit` hears `Edit::Focus`, `Edit::Blur` and `Edit::Escape` (Escape, then Blur) from ANY focus move (press, Tab, Escape, claim, host request). Focus a node from the view with `node.claim_focus(&stamp)` / `Bindings::claim_focus`: edge-triggered per stamp, applied by `Router::frame` (no host call). See `drag_value.rs` (claim on click, cancel on Escape, commit on Blur). |
| `ctx.request_repaint_after(d)`, a debounce or hold-repeat clock | A reducer returns `Act::wake(at, its_own_event)` (one per key; a later one replaces it; `Act::cancel_wake()` drops it). The host calls `kit_state::wake_due(app, &mut store)` once per frame; the frame's whole demand (`Redraw::{Now, At(t), Idle}`, from `DrawList::frame_redraw`) includes these wake-ups, so a host may sleep until it. Wake-ups die with the key's life. See `search_field.rs` (debounce), `stepper.rs` (hold-repeat). |
| `ui.memory(\|m\| m.data...)`, `ctx.data` | `cx.state(&key, init, \|s, k, ev, app\| ..)`: inline NON-capturing reducer (a capture does not compile; pass values in the event or the key) |
| `ctx.animate_bool(id, on)` / `animate_value_with_time` | `cx.animate_bool(&key, on, duration)` / `cx.animate(&Part(key, "knob"), target, d)` (several values per key: `Part`) |
| `Id`, `Id::new("x").with(i)` | a data key implementing `Ident<App>` (`Life` says when its state dies); node identity `WidgetId::of(&key)` / `.key(&key)` |
| `Area`, `Popup`, `Tooltip`, `Window`, `Modal`, `ComboBox` list | `portal(Layer::{Popup, Modal, Toast, Tooltip}, anchor, child)` (overlay agent) |
| `ScrollArea` | the scroll core (`kit_view::scroll` + `kit_input` scroll state keyed by data) |
| `TextEdit`, `TextBuffer`, `CCursor` | the text core (`TextValue` + `kit_text` editor, `text_field`) |
| `ui.horizontal / vertical / columns / Grid` | `row`, `column`, taffy `Style` (`gap`, `grow`, `align_items`) |
| `Visuals` colours, `Rounding`, `Stroke` | `theme.colors.*`, `theme.radii.*`, `Border::Solid { color: theme.colors.field_border, .. }` |
| `on_hover_text` | `Tooltip` on the interactive node |
| `Widget::ui(self, ui)` builder (`Button::new(..).small()`) | a props struct; builder methods only if the egui API is the point |

## File rules

- One file: `kit_widgets/src/<name>.rs`, plus its test `kit_widgets/tests/components/<name>.rs`
  and its goldens in `kit_widgets/tests/golden/<name>_*.png`.
- Edit NO shared file: not `lib.rs`, `common.rs`, `Cargo.toml`, `tests/components/main.rs`, `support.rs`, any other crate.
  The coordinator adds `pub mod <name>;` and `mod <name>;`. Until then you can compile by locally adding those two lines
  and reverting them, or by asking the coordinator. Using `crate::common::{ink, surface, ..}` helpers is fine.
- Public surface: a props struct and `pub fn <name><A: Host, K: Ident<A>>(cx: &Cx<'_, A>, props) -> Node<Intent<A>>`.
  Stateless components take no `cx`.
- Size targets: simple components <= 60 lines, popup/list components <= 120, text components <= 150 (the file, tests
  excluded, rustfmt-formatted). Missing the target means the infrastructure is missing something: report it.
- Attribution header (first two lines, kept verbatim except the path), because the logic is derived from egui:
  ```
  // Ported from egui 0.36.2 `crates/egui/src/<path of the source file>` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
  // Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
  ```
  Add the module doc (`//!`) after it. Logic you wrote yourself from behaviour alone does not need the header.

## One Clean Path rules

- Kinds are types: if behaviour differs by kind (checked/unchecked/mixed, open/closed), make an enum and match it ONCE
  per decision; each arm is straight-line. No `Option` meaning "undecided": name the cases.
- No silent fallbacks: invalid props are a typed error or a panic at construction named in the doc (see `Gauge::new`);
  never clamp-and-continue unless egui's behaviour IS the clamp and a test says so.
- Theme tokens only: no literal colours, no literal px except math constants taken from egui (comment their source).
- No clock: time is `cx.now()` or `cx.animate*`. No globals, no `thread_local`, no new lint suppression.
- Exhaustive matches; no `_ =>` on enums you own.

## Required tests (in `tests/components/<name>.rs`, module `<name>` listed by the coordinator)

Use `support::Handle` (see the template test): `Handle::new(app, (w, h), view)`, `.click(Role, label)`,
`.press_key(Key)`, `.type_text(s)`, `.toggle_of(Role, label)`, `.app()`, `.assert_golden(name)`.
1. ONE golden per visually distinct state (idle and the main active state are enough). CPU raster with the real Inter
   font. LOOK at the PNG before blessing (`KIT_BLESS=1 cargo test -p kit_widgets --test components`); a blank or clipped picture
   is a failure, not a golden.
2. ONE behaviour test that catches a real bug, named after it: the wrap, the clamp, the snap, the key that must not
   fire while disabled, the intent that must be refused after the key's life ends. Before you finish, break the code
   the test is about (mutant) and watch it fail. No tests that restate props or call wiring.

## Verify

```
cargo test --locked -p kit_widgets           # your tests + the rest
cargo test --locked -p kit_view
cargo clippy --locked -p kit_widgets --all-targets -- -D warnings
cargo fmt --all --check
```
CI (`.github/workflows/ci.yml`) also runs the whole workspace, a wasm build, and a check that no kit crate pulls
`wasm-bindgen`, `js-sys` or `web-sys` into a native build.

## Worked examples: the pilots (2026-10-23)

Read these before writing anything; copy their shape.

| Pilot | File | Lines (target) | Shows |
|---|---|---|---|
| checkbox | `kit_widgets/src/checkbox.rs` | 65 (60) | controlled value, `Toggle::{Off, On, Mixed}`, disabled, Space only when focused |
| combo box | `kit_widgets/src/combo_box.rs` | 107 (120) | open state in `cx.state` keyed by data, `Popup` portal anchored to itself, arrow/Enter/Escape, click-outside dismiss |
| text field / area | `kit_widgets/src/text_field.rs` | 137 (150) | `EditorState` in `cx.state`, Enter submits via `Act::ask`, stale edits counted as faults, area grows to `max_rows` then scrolls with `stick_to_end` |
| template | `kit_widgets/src/template.rs` | 61 | the minimal shape |

Tests: `kit_widgets/tests/components/{checkbox,combo_box,text_field}.rs`; goldens `tests/golden/{checkbox,combo_box,text_field}_*.png`.

Infrastructure added by the pilots (already in the tree):
- `Role::{CheckBox, ComboBox, Option}` and `Toggle::Mixed`, mirrored to ARIA in `host_web/src/kit_mirror.rs`.
- (2026-10-23, keys/fields/timers) `Bindings::on_key`, `Bindings::activate_with`, `InputEvent::Modifiers`,
  `Edit::{Focus, Blur, Escape}`, `FocusClaim`/`claim_focus`, `Act::wake`/`wake_due`/`Store::redraw`,
  `A11y::{named, toggled, Ranged}` and `Role::Slider` (mirrored as ARIA `slider` with `aria-valuenow/min/max`).
  Test hosts: `support::Handle::tick(|app| ..)` is one host frame (wake-ups, layout, `Router::frame`).
- Router: once a control has keyboard focus (only Tab gives a control focus; a pointer press never does), ArrowDown/ArrowUp
  move focus like Tab/Shift-Tab, which is how an open list is walked, unless the focused control's `on_key` takes them.
  With nothing focused, arrows stay the game's camera keys.

Checklist for every port:
1. Read the egui source named in your task, the pilots, and the APIs you use as they are in the tree now.
2. Write your one file; keep it compiling (write it whole, not half at a time: other agents compile the same crate).
3. Golden(s): bless with `KIT_BLESS=1`, then LOOK at each PNG. Behaviour test: break the code, watch it fail, restore.
4. Missed the size target? Say what infrastructure was missing; do not edit other crates.
5. If another component's file breaks the build, wait and retry; never edit it.
