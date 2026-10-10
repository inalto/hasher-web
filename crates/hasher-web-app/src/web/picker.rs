//! Dialoghi di apertura del browser: `<input type="file">` creato al volo e `click()`-ato
//! durante l'azione egui (l'attivazione transitoria del clic o del tasto è ancora valida).
//!
//! Con "Apri cartella" (`webkitdirectory`) il browser chiede conferma ("Caricare N file?"):
//! nonostante il testo, nulla viene inviato; i file restano nel browser (lo spiega la guida).

use super::{document, focus_canvas, Bridge, InputOrigin};
use crate::engine::WebInput;
use hasher_core::checksum_file::CHECKSUM_EXTENSIONS;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Tipo di dialogo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    Files,
    Folder,
    Checksum,
}

/// Apre il dialogo; i file scelti arrivano all'app come [`super::InputBatch`].
pub fn open(bridge: &Bridge, kind: PickKind) {
    if let Err(e) = try_open(bridge, kind) {
        log::error!("dialogo di apertura non disponibile: {}", super::js_error_text(&e));
    }
}

fn try_open(bridge: &Bridge, kind: PickKind) -> Result<(), JsValue> {
    let doc = document().ok_or_else(|| JsValue::from_str("document non disponibile"))?;
    let input: web_sys::HtmlInputElement = doc.create_element("input")?.dyn_into()?;
    input.set_type("file");
    match kind {
        PickKind::Files => input.set_multiple(true),
        PickKind::Folder => {
            input.set_multiple(true);
            input.set_webkitdirectory(true);
        }
        PickKind::Checksum => {
            let accept: Vec<String> = CHECKSUM_EXTENSIONS.iter().map(|e| format!(".{e}")).collect();
            input.set_accept(&accept.join(","));
        }
    }
    input.style().set_property("display", "none")?;
    if let Some(body) = doc.body() {
        body.append_child(&input)?;
    }

    let b = bridge.clone();
    let el = input.clone();
    let on_change = Closure::once_into_js(move |_e: web_sys::Event| {
        let inputs = collect(&el, kind, b.walk.get());
        el.remove();
        focus_canvas();
        let origin = match kind {
            PickKind::Files => InputOrigin::OpenFiles,
            PickKind::Folder => InputOrigin::OpenFolder,
            PickKind::Checksum => InputOrigin::OpenChecksum,
        };
        wasm_bindgen_futures::spawn_local(async move {
            b.deliver(inputs, None, origin, kind == PickKind::Folder).await;
        });
    });
    input.add_event_listener_with_callback("change", on_change.unchecked_ref())?;

    let el = input.clone();
    let on_cancel = Closure::once_into_js(move |_e: web_sys::Event| {
        el.remove();
        focus_canvas();
    });
    input.add_event_listener_with_callback("cancel", on_cancel.unchecked_ref())?;

    input.click();
    Ok(())
}

/// File scelti, con i filtri delle impostazioni per le cartelle (nascosti, ricorsione).
fn collect(input: &web_sys::HtmlInputElement, kind: PickKind, walk: super::WalkOptions) -> Vec<WebInput> {
    let Some(list) = input.files() else { return Vec::new() };
    let mut out: Vec<WebInput> = Vec::with_capacity(list.length() as usize);
    for i in 0..list.length() {
        let Some(f) = list.get(i) else { continue };
        let rel = if kind == PickKind::Folder {
            js_sys::Reflect::get(&f, &JsValue::from_str("webkitRelativePath"))
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default()
        } else {
            String::new()
        };
        if kind == PickKind::Folder && !rel.is_empty() {
            // `cartella/sub/file`: il primo componente è la cartella scelta.
            let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
            let below = parts.get(1..).unwrap_or_default();
            if !walk.include_hidden && below.iter().any(|p| p.starts_with('.')) {
                continue;
            }
            if !walk.recurse && below.len() > 1 {
                continue;
            }
        }
        out.push(WebInput::new(f, rel));
    }
    if kind == PickKind::Folder {
        // Ordine per percorso (componente per componente), come l'esplorazione del desktop.
        out.sort_by(|a, b| a.rel_path.split('/').cmp(b.rel_path.split('/')));
    }
    out
}
