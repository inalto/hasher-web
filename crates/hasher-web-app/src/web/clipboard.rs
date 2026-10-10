//! Pulsante "Incolla" del pannello Verifica: sul web `ViewportCommand::RequestPaste` non esiste,
//! si legge il testo con `navigator.clipboard.readText()` (il browser può chiedere il permesso).
//! Ctrl+V invece passa dall'evento `paste`, gestito da eframe.

use super::{js_error_text, window, Bridge};

pub fn request_paste(bridge: &Bridge) {
    let Some(w) = window() else { return };
    let promise = w.navigator().clipboard().read_text();
    let bridge = bridge.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let result = match wasm_bindgen_futures::JsFuture::from(promise).await {
            Ok(v) => Ok(v.as_string().unwrap_or_default()),
            Err(e) => Err(js_error_text(&e)),
        };
        *bridge.pasted.borrow_mut() = Some(result);
        bridge.request_repaint();
    });
}
