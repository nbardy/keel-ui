# Keel UI kit

**React-style UI in pure Rust, drawn by wgpu inside your own render pass.**

A declarative, functional-reactive UI library for games and GPU apps: components are plain functions, local state
works in loops and branches, events are values, and the UI records into *your* wgpu command encoder, in the same frame
as your scene. No web runtime, no second renderer, no compositing pass. Layout is [taffy] (flexbox and grid) and text
is [cosmic-text], both used through their own types. Runs natively (Metal, Vulkan, DX12) and in the browser (wasm +
WebGPU) from the same code.

![The kit's HUD drawn over the game's 3D sea in one frame: buttons, hull bar, weapon tray, and health bars anchored to ships](docs/hud-over-sea.jpg)

*Every pixel of UI above is the kit, recorded into the game's own encoder after the 3D scene. The health bars and the
range ring follow the ships in world space. Captured by the game's headless-Chrome WebGPU test.*

## Example

A shop list where each row has its own pin (local state) and a Buy button (a request to the server):

```rust
/// A row's identity is the item's id, and it lives exactly as long as the server lists the item.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Row(u32);
impl Ident<Shop> for Row {
    fn life(&self, shop: &Shop) -> Life {
        match shop.items.iter().any(|item| item.id == self.0) {
            true => Life::Live,
            false => Life::Over,
        }
    }
}

/// A component is a plain function: app data and props in, a value tree out. It re-runs every frame.
fn shop(cx: &Cx<'_, Shop>, theme: &Theme) -> Node<Intent<Shop>> {
    let ui = Ui::new(theme);
    column(cx.app().items.iter().map(|item| {
        let key = Row(item.id);
        // Local state inside a loop, keyed by data (not call order). The reducer cannot capture anything.
        let pinned = cx.state(
            &key,
            || false,
            |pinned: &mut bool, key: &Row, event: RowEvent, _: &Shop| match event {
                RowEvent::Pin => {
                    *pinned = !*pinned;
                    Act::none()
                }
                RowEvent::Buy => Act::ask(Buy(key.0)),
            },
        );
        row([
            switch(cx, Switch { theme, key: key.clone(), label: item.name, on: *pinned,
                                press: Press::Sends(pinned.send(RowEvent::Pin)) }),
            ui.labeled_button(COIN, format!("Buy {}", item.name))
                .on_press(pinned.send(RowEvent::Buy)) // a press is a value: testable, replayable
                .into(),
        ])
    }))
}
```

Insert an item at the top and every pin stays with its own item. When the server drops an item, its row's state is
collected. `Shop` is your app model, with the server's data in it. That whole program, including the `Shop` model,
is a test: [`kit_widgets/tests/components/readme.rs`](kit_widgets/tests/components/readme.rs). It drives the
list with real clicks, and checks were confirmed to fail when state is keyed by position or never collected.

## Why use it

- **Your frame, your encoder.** The UI is the last thing you record into the game's own `wgpu::CommandEncoder`
  (`LoadOp::Load`, over your scene). There is no UI-owned window, renderer, offscreen layer or second hit-tester.
- **World-anchored UI built in.** `world(anchor, node)` places UI at a projected 3D point: nameplates, health bars,
  markers.
- **Functional reactive, no dependency lists.** Views are functions from app data to a value tree, re-run every frame.
  The game's 58-node HUD rebuilds and lays out in about 23 µs.
- **Local state without hook rules.** `cx.state(&key, init, reducer)` is keyed by data, so it works in loops, branches
  and reordered lists. The key's `life` (a function of your app data) decides when state is collected. An event that
  arrives for a dead key is refused and counted, never applied to the wrong row.
- **Server data is just your model.** Views read the latest server state directly. A press becomes an `Ask` your host
  sends over the network. Nothing caches a copy that can go stale.
- **Events are data.** Every press, key and edit is an `Intent` value. Tests drive real layouts with real clicks, and
  input can be replayed.
- **The standard libraries, unwrapped.** A node's style *is* a `taffy::Style`, and text *is* `cosmic-text`. There's
  no layout or text abstraction of the kit's own to learn.
- **No web runtime.** Pure Rust down to wgpu. The browser build adds a small host (`hosts/host_web`), used only on wasm.
- **Small, copyable components.** About 25 components (button, checkbox, switch, slider, text field, combo box,
  listbox, menu, modal, tabs, table, toast, tooltip, colour picker…), most 50–190 lines. The library is the
  infrastructure; components are meant to be copied into your crate and restyled.

**Pick it** for a Rust game or GPU app with your own wgpu loop that wants declarative, stateful UI inside its frame,
especially a multiplayer one where the server owns the truth. **Pick something else** if you use Bevy (Bevy UI or
bevy_egui), need a big ready-made widget set for tools (egui), or build a document-style web app (the DOM).

## Status and limits

- **One user so far:** it is the entire UI of a multiplayer browser/native game (sign-in, lobby, rooms, in-match HUD,
  four station consoles, shop, chat, settings). That game's DOM UI was deleted in its favour on 2026-10-23.
- **Not on crates.io yet:** depend on it by git or path. Licence: [WTFPL](LICENSE); ported egui code and the
  bundled Inter font keep their own licences ([THIRD_PARTY.md](THIRD_PARTY.md)).
- **No screen-reader support.** There is a semantics mirror, but it exists for tests and keyboard activation;
  accessibility is out of scope by owner decision.
- **Fewer widgets than egui,** and no built-in docking, plotting or rich-text editing.
- **Server-authority helpers are designed, not built.** That means tracking a sent command to its confirmed, refused
  or unknown outcome (`cx.track` / `cx.read`). Today you model pending requests in your own app data.
- **Requirements:** wgpu 30, Rust 1.97 (edition 2024). WebGPU only on the web; there is no WebGL2 path.

## Plug it into your frame

The whole per-frame loop. It is the same loop `kit_widgets/tests/components/support.rs` and the game's hosts run:

```rust
// once
let mut store = Store::<App>::new();          // every component's local state, keyed by data
let mut memory = UiMemory::new(viewport);     // hover, focus, scroll, gestures
let (mut layout, router) = (TaffyLayout::new(), Router::default());
let mut text = CosmicText::new(font_source, TextConfig::default())?;
let mut gpu_ui = WgpuUi::new(device.clone(), queue.clone(), surface_format);

// every frame
store.collect(&app);                          // server data changed: end the state of keys whose life is Over
wake_due(&mut app, &mut store);               // timers that reducers scheduled
let root = view(&Cx::routed(&app, &store, &memory), &theme);
let laid = layout.layout(root, viewport, &mut text, &camera)?; // `camera` projects world anchors; or `&NoWorld`
for event in window_events {                  // your winit/browser events converted to `InputEvent`
    match router.route(&laid, &mut memory, event.clone(), app.now()) {
        Routing::Handled(intents) => intents.into_iter().for_each(|i| dispatch(&mut app, &mut store, i)),
        Routing::Captured => {}               // the UI took it; the game must not see it
        Routing::Passed => game.input(event), // nothing in the UI wanted it
    }
}
router.frame(&laid, &mut memory, app.now());  // scroll momentum, focus requests
memory.end_frame();
let draws = paint(&laid, &memory, &theme, app.now());
// ... record your scene into `encoder`, then:
gpu_ui.render(&draws, &mut text, WgpuTarget { encoder: &mut encoder, surface: &ui_surface })?;
queue.submit([encoder.finish()]);
gpu_ui.frame_submitted();
```

Your app implements `Host` (a clock, a motion preference, and where `Ask`s go). The clock is an input, never read from
the OS, so tests and replays are deterministic.

## Compared with the alternatives

| | Bevy UI | egui | Dioxus / Freya | Xilem | **Keel kit** |
|---|---|---|---|---|---|
| UI is | ECS entities + systems | immediate calls, no tree | functions → tree | views as values | **functions → value tree** |
| Local state | components on entities | memory keyed by label/position `Id` | hooks, call-order rules | app data + lenses | **`cx.state(&key, ..)` keyed by data; fine in loops and branches** |
| Dependencies | queries | n/a | dependency lists | n/a | **none: views re-run per frame** |
| Cleanup | despawn by hand | GC of unseen ids | effect teardown | n/a | **the key's `life` collects state; late input is refused** |
| Events | observers | `if response.clicked()` | closures | callbacks | **data (`Intent`): testable, replayable** |
| Draws in your frame | no (Bevy owns it) | yes (egui-wgpu) | no (own renderer) | no (own window) | **yes (your encoder, world-anchored UI)** |

**Why not put Dioxus, Freya or Blitz inside a game?** They want to own the window and the renderer. To use one in a
game you either make the UI framework the host (the game becomes a texture inside it, which is backwards at 60 Hz),
or you layer it: render the UI offscreen, composite it over the game every frame, and arbitrate every input event
between two hit-testers.

| | Dioxus desktop | Dioxus native / Blitz | Freya | **Keel kit** |
|---|---|---|---|---|
| Renderer | system WebView (a web runtime) | its own HTML/CSS engine (Stylo + vello) | Skia (C++) | **the game's wgpu encoder** |
| Inside a game frame | no | offscreen layer + composite; vello's wgpu must match yours | GL/Metal/Vulkan texture interop with wgpu, or CPU readback | **directly: no second renderer, no composite, no input arbitration** |
| World-anchored UI | n/a | push projected positions into its state every frame | same | **native (`world(anchor, ..)`)** |
| Extra weight | a browser engine | a CSS engine | a C++ graphics library | **cosmic-text, taffy, lyon, etagere** |

**Versus egui**, the closest fit: egui also draws into your wgpu pass and is far more mature. Choose the kit when you
want the UI as a value tree (testable by role and label, not pixels), state that follows data identity through
reordering and is collected by your app's own rules, and input routing that tells the game exactly what the UI did
not take (`Handled` / `Captured` / `Passed`).

## How it works

**State.** One `Store` holds every component's local state, keyed by (reducer type, data key). The reducer is an inline
closure that must not capture anything (`const { assert!(size_of::<F>() == 0) }`), so a capturing reducer is a
compile error and the reducer can travel inside the `Intent`. `cx.state` returns the value plus a sender, and
`sender.send(ev)` *builds* an intent without applying it. `dispatch` applies it later, runs the reducer, forwards any
`Ask` to your `Host`, and collects ended keys. `Life::Leaving(t)` keeps state inert until `t`, so exit animations
have their data. Also built on the same store: `cx.derive` (memoised values), `cx.animate` (tweens that respect
reduced motion) and timers (`Act::wake`). Interaction reads (`cx.hovered/pressed/focused/drag`, `cx.scroll`) come
from the router's memory.

**Layout.** Each node carries a `Style { layout: taffy::Style, placement, content }`, and the tree is rebuilt into
taffy every frame. Text is measured through cosmic-text inside taffy's measure function, so wrapping and flex sizing
agree. Placement adds safe-area anchors and world anchors. Portals (popups, modals, toasts, tooltips) lay out in
their own layers; dismissal is data, and a modal takes input from the world beneath it.

**Input.** You convert your window's events to `InputEvent`, and `Router::route` hit-tests the laid-out tree. It owns
gestures (press, drag, long-press), focus and Tab order, scroll (momentum, nested hand-off, scrollbars), text editing
(`EditorState`, with IME and clipboard bridged by the host) and keyboard activation. It answers with
`Routing::{Handled(intents), Captured, Passed}`, so the game knows exactly which input is still its own.

**Paint and render.** `paint` turns the laid-out tree into a `DrawList` (fills, strokes, rounded rects, glyph runs,
clip layers). `kit_wgpu` tessellates shapes with lyon (cached), packs glyphs into an etagere atlas, and records one
pass into your encoder. `kit_raster` is a tiny-skia CPU backend with the same `DrawList`, used for golden-image tests.

**Guarantees and tests.**
- The CPU and GPU backends agree per pixel on every scene fixture (`kit_wgpu/tests/parity.rs`), with a mutation check
  per draw kind.
- Every component has behaviour tests driven through the real router by role and label (`kit_widgets/tests/`), plus
  golden images.
- The game's HUD and screens are pinned by goldens and by headless-Chrome WebGPU stages that click the real canvas.
- Nothing degrades silently: unsupported brushes, cells outside a grid, missing fonts and late intents are typed
  errors or counted records.

**Size.** About 17k lines of shipped Rust across the crates below (measured 2026-10-23, `kit_raster` excluded). The
game that uses it, UI included, is 2.16 MB gzipped as wasm.

## Crates

| Crate | Job |
| --- | --- |
| `kit_core` | ids, time, viewport, theme tokens, shapes; re-exports `kurbo`, `peniko` |
| `kit_state` | `Cx`, `cx.state` / `derive` / `animate`, interaction reads, keys and life, `Intent` / `Act`, timers |
| `kit_view` | the `Node<Msg>` value tree and primitives (`panel`, `row`, `column`, `stack`, `text`, `shape`, `scroll_area`, `world`, `overlay`, `interactive`, `portal`), semantics, test helpers |
| `kit_input` | `InputEvent`, `UiMemory`, `Router` → `Routing::{Handled, Captured, Passed}`; focus, scroll, text editing |
| `kit_layout` | layout on taffy, plus safe-area and world anchors and portals |
| `kit_text` | text on cosmic-text, bounded caches, the bundled Inter font |
| `kit_paint` | `DrawList` and the painter |
| `kit_wgpu` | the GPU backend that records into your encoder (lyon, etagere) |
| `kit_raster` | CPU backend on tiny-skia for golden and parity tests (dev-only) |
| `kit_widgets` | the components; short, styled by theme tokens, meant to be copied |

## Try it

```sh
cargo test -p kit_widgets --test components   # every component, driven by clicks and keys, plus goldens
cargo test -p kit_wgpu                        # CPU-vs-GPU per-pixel parity (needs a GPU adapter)
cargo test --workspace                        # everything
```

## Further reading

- Design, API and the lineage (Om Next, Bonsai, Elm, React), with five competing designs and their critiques:
  [`notes/engine/ui/DESIGN.md`](docs/DESIGN.md)
- Codebase map and test layout: [`CODEBASE.md`](docs/CODEBASE.md)
- Porting a component from egui: [`PORTING.md`](docs/PORTING.md)

[taffy]: https://github.com/DioxusLabs/taffy
[cosmic-text]: https://github.com/pop-os/cosmic-text
