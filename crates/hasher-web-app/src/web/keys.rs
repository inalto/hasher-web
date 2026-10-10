//! Scorciatoie dell'app che il browser intercetterebbe (Ctrl+E cerca, Ctrl+L barra degli
//! indirizzi, F1 guida del browser, …): un listener `keydown` in fase di capture ne blocca
//! l'azione predefinita. L'evento prosegue verso il canvas, dove eframe lo passa a egui.
//! Ctrl+C / Ctrl+V / Ctrl+X non vengono toccati (servono gli eventi `copy`/`paste`).

use super::window;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// `true` se la combinazione appartiene all'app (vedi `ui::HasherApp::handle_input`).
fn is_app_shortcut(key: &str, command: bool) -> bool {
    if key == "F1" {
        return true;
    }
    command && matches!(key.to_ascii_lowercase().as_str(), "o" | "e" | "l" | ",")
}

pub(super) fn install() -> Result<(), JsValue> {
    let Some(w) = window() else { return Ok(()) };
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_capture(true);
    let on_keydown = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|e: web_sys::KeyboardEvent| {
        let command = e.ctrl_key() || e.meta_key();
        if !e.alt_key() && is_app_shortcut(&e.key(), command) {
            e.prevent_default();
        }
    });
    w.add_event_listener_with_callback_and_add_event_listener_options(
        "keydown",
        on_keydown.as_ref().unchecked_ref(),
        &opts,
    )?;
    on_keydown.forget();
    Ok(())
}
