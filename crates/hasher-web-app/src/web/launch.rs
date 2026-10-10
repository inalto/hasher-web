//! File aperti dal sistema con la PWA installata ("Apri con Hasher", `file_handlers` del
//! manifest): `web/boot.js` li accoda in `window.hasherLaunchFiles` ed emette
//! `hasher:launch-files`. La coda viene letta all'avvio e a ogni evento.

use super::{window, Bridge, InputOrigin};
use crate::engine::WebInput;
use js_sys::{Array, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Svuota `window.hasherLaunchFiles` e consegna i file all'app.
pub fn take_pending(bridge: &Bridge) {
    let Some(w) = window() else { return };
    let Ok(queue) = Reflect::get(&w, &JsValue::from_str("hasherLaunchFiles")) else {
        return;
    };
    let Ok(queue) = queue.dyn_into::<Array>() else { return };
    if queue.length() == 0 {
        return;
    }
    let taken: Vec<JsValue> = queue.iter().collect();
    queue.set_length(0);
    let inputs: Vec<WebInput> = taken
        .into_iter()
        .filter_map(|v| v.dyn_into::<web_sys::File>().ok())
        .map(|f| {
            let name = f.name();
            WebInput::new(f, name)
        })
        .collect();
    if inputs.is_empty() {
        return;
    }
    let bridge = bridge.clone();
    wasm_bindgen_futures::spawn_local(async move {
        bridge.deliver(inputs, None, InputOrigin::Launch, false).await;
    });
}

pub(super) fn install(bridge: &Bridge) -> Result<(), JsValue> {
    let Some(w) = window() else { return Ok(()) };
    let b = bridge.clone();
    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_e: web_sys::Event| take_pending(&b));
    w.add_event_listener_with_callback("hasher:launch-files", cb.as_ref().unchecked_ref())?;
    cb.forget();
    Ok(())
}
