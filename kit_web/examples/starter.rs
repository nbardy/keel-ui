//! The kit's starter in a browser tab: the same app as `kit_winit`'s starter (a scene with the kit UI over it, one wgpu
//! frame), on a WebGPU `<canvas>`. wasm only; on native this example is empty.
//!
//! ```sh
//! kit_web/build.sh serve       # build, then serve http://localhost:8080 (needs a WebGPU browser)
//! ```
//!
//! The app is `examples_shared/starter_app.rs`, shared with the native starter. This file is the browser host's
//! wiring: it implements [`kit_web::App`] for the shared `Starter` and exposes a probe for tests.
#![cfg(target_arch = "wasm32")]
#[path = "../../examples_shared/starter_app.rs"]
mod app;

use app::{SCENE_CLEAR, Starter};
use kit_web::{App, FrameInput, FrameOutput, Running};
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

/// The shared app plus what the probe reports about the last frame.
struct Page {
    starter: Starter,
    /// Where each labelled control was drawn (CSS px), so a test can click it, as `kit_test` does natively.
    controls: Vec<kit_view::Semantic>,
}

impl App for Page {
    fn frame(&mut self, input: FrameInput<'_>) -> Result<FrameOutput, String> {
        let drawn = self
            .starter
            .frame(
                input.events,
                input.viewport,
                input.view,
                input.size,
                input.dt,
                input.clipboard,
            )
            .map_err(|error| error.to_string())?;
        self.controls = kit_view::semantics(&drawn.laid);
        Ok(FrameOutput {
            text_target: drawn.text_target,
            paste_ticket: drawn.paste_ticket,
            redraw: drawn.redraw,
            clipboard_failures: drawn.clipboard_failures,
        })
    }
}

thread_local! {
    /// The running host lives as long as the page; `probe` reads it.
    static RUNNING: RefCell<Option<Running<Page>>> = const { RefCell::new(None) };
}

/// Start on the page's `<canvas id="kit">`. Called by the page's script once the module is loaded.
#[wasm_bindgen]
pub async fn run() -> Result<(), JsValue> {
    kit_web::log_panics();
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or("no document")?;
    let canvas = document
        .get_element_by_id("kit")
        .ok_or("the page has no <canvas id=\"kit\">")?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;
    let running = kit_web::start(canvas, |gpu, viewport, platform| Page {
        starter: Starter::new(
            gpu.device.clone(),
            gpu.queue.clone(),
            gpu.format,
            viewport,
            platform,
        ),
        controls: Vec::new(),
    })
    .await?;
    RUNNING.with(|slot| *slot.borrow_mut() = Some(running));
    Ok(())
}

/// For tests: `{frames, callsign, angle, colour, speed, clear, controls}` as JSON. `frames` stops rising when the page
/// is idle; `controls` lists `{role, label, x, y}` with the centre of each labelled control in CSS px.
#[wasm_bindgen]
pub fn probe() -> String {
    RUNNING.with(|slot| match &*slot.borrow() {
        None => "{\"started\":false}".to_owned(),
        Some(running) => running.with_app(|page| {
            let controls = page
                .controls
                .iter()
                .map(|c| {
                    let at = c.rect.center();
                    format!(
                        "{{\"role\":\"{:?}\",\"label\":{:?},\"x\":{},\"y\":{}}}",
                        c.role, c.label, at.x, at.y
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let app = &page.starter.app;
            format!(
                "{{\"started\":true,\"frames\":{},\"callsign\":{:?},\"angle\":{},\"colour\":{},\"speed\":{},\"clear\":{:?},\"controls\":[{}]}}",
                running.frames(),
                app.callsign,
                app.spin.angle(),
                app.spin.colour,
                app.spin.speed,
                SCENE_CLEAR,
                controls,
            )
        }),
    })
}
