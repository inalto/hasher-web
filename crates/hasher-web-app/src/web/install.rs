//! Installazione come app (PWA): `web/boot.js` conserva l'evento `beforeinstallprompt` ed espone
//! `window.hasherCanInstall()` e `window.hasherInstall()`. La voce "Installa come app" compare
//! solo quando il browser lo consente (Chrome, Edge…).

use super::window;
use js_sys::{Function, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

fn global_fn(name: &str) -> Option<Function> {
    let w = window()?;
    Reflect::get(&w, &JsValue::from_str(name))
        .ok()?
        .dyn_into::<Function>()
        .ok()
}

/// `true` se il browser ha offerto l'installazione e l'app non è già installata.
pub fn can_install() -> bool {
    global_fn("hasherCanInstall")
        .and_then(|f| f.call0(&JsValue::NULL).ok())
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Mostra il prompt di installazione del browser (deve seguire un gesto dell'utente).
pub fn install_app(ctx: &egui::Context) {
    let Some(f) = global_fn("hasherInstall") else { return };
    let promise = match f.call0(&JsValue::NULL) {
        Ok(v) => v.dyn_into::<js_sys::Promise>().ok(),
        Err(e) => {
            log::warn!("hasherInstall: {}", super::js_error_text(&e));
            None
        }
    };
    let ctx = ctx.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if let Some(p) = promise {
            if let Err(e) = wasm_bindgen_futures::JsFuture::from(p).await {
                log::warn!("installazione: {}", super::js_error_text(&e));
            }
        }
        // La voce del menu va nascosta (o mostrata di nuovo) appena cambia lo stato.
        ctx.request_repaint();
    });
}

/// Ridisegna quando il browser rende disponibile (o completa) l'installazione.
pub(super) fn install(ctx: &egui::Context) -> Result<(), JsValue> {
    let Some(w) = window() else { return Ok(()) };
    for event in ["hasher:installable", "appinstalled"] {
        let ctx = ctx.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_e: web_sys::Event| ctx.request_repaint());
        w.add_event_listener_with_callback(event, cb.as_ref().unchecked_ref())?;
        cb.forget();
    }
    Ok(())
}
