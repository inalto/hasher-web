//! Trascinamento di file e cartelle sul canvas.
//!
//! Il gestore `drop` di eframe usa solo `dataTransfer.files` (niente cartelle): questo modulo
//! registra listener in fase di capture sul canvas, prima di quelli di eframe. Al `drop` legge
//! subito (finché `dataTransfer` è accessibile) le voci con `webkitGetAsEntry()` e i modificatori,
//! ferma l'evento (`stopImmediatePropagation`) e invia al canvas un `dragleave` sintetico, così
//! eframe azzera `hovered_files` e la sovrimpressione scompare; poi esplora le cartelle in modo
//! asincrono (`FileSystemDirectoryReader.readEntries` finché restituisce 0 voci) rispettando le
//! impostazioni "ricorsione" e "file nascosti" e consegna i file all'app.
//!
//! Il `dragover` resta a eframe (che popola `hovered_files` per la sovrimpressione); qui se ne
//! leggono solo i modificatori, perché durante il trascinamento il browser non invia eventi di
//! tastiera.

use super::{focus_canvas, js_error_text, Bridge, InputOrigin, WalkOptions};
use crate::engine::WebInput;
use egui::Modifiers;
use js_sys::{Array, Promise};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{DragEvent, FileSystemDirectoryEntry, FileSystemEntry, FileSystemFileEntry, HtmlCanvasElement};

/// Elemento rilasciato al primo livello.
enum Top {
    File(web_sys::File),
    Dir(FileSystemDirectoryEntry),
}

/// Contenuto di una cartella, già ordinato per nome.
enum Child {
    File(WebInput),
    Dir(FileSystemDirectoryEntry),
}

/// Modificatori di un evento del mouse (anche `DragEvent`).
pub fn modifiers(e: &web_sys::MouseEvent) -> Modifiers {
    Modifiers {
        alt: e.alt_key(),
        ctrl: e.ctrl_key(),
        shift: e.shift_key(),
        mac_cmd: e.meta_key(),
        command: e.ctrl_key() || e.meta_key(),
    }
}

pub(super) fn install(bridge: &Bridge, canvas: &HtmlCanvasElement) -> Result<(), JsValue> {
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_capture(true);

    let b = bridge.clone();
    let on_dragover = Closure::<dyn FnMut(DragEvent)>::new(move |e: DragEvent| {
        b.drag_mods.set(modifiers(&e));
    });
    canvas.add_event_listener_with_callback_and_add_event_listener_options(
        "dragover",
        on_dragover.as_ref().unchecked_ref(),
        &opts,
    )?;
    on_dragover.forget();

    let b = bridge.clone();
    let c = canvas.clone();
    let on_drop = Closure::<dyn FnMut(DragEvent)>::new(move |e: DragEvent| handle_drop(&b, &c, &e));
    canvas.add_event_listener_with_callback_and_add_event_listener_options(
        "drop",
        on_drop.as_ref().unchecked_ref(),
        &opts,
    )?;
    on_drop.forget();
    Ok(())
}

fn handle_drop(bridge: &Bridge, canvas: &HtmlCanvasElement, e: &DragEvent) {
    let Some(dt) = e.data_transfer() else { return };
    let mut tops = Vec::new();
    let items = dt.items();
    for i in 0..items.length() {
        let Some(item) = items.get(i) else { continue };
        if item.kind() != "file" {
            continue;
        }
        match item.webkit_get_as_entry().ok().flatten() {
            Some(entry) if entry.is_directory() => tops.push(Top::Dir(entry.unchecked_into())),
            _ => {
                if let Ok(Some(f)) = item.get_as_file() {
                    tops.push(Top::File(f));
                }
            }
        }
    }
    if tops.is_empty() {
        // Browser senza `DataTransferItemList`: almeno i file semplici.
        if let Some(files) = dt.files() {
            for i in 0..files.length() {
                if let Some(f) = files.get(i) {
                    tops.push(Top::File(f));
                }
            }
        }
    }
    if tops.is_empty() {
        return; // nulla di utilizzabile (testo, link…): decide eframe
    }
    e.stop_immediate_propagation();
    e.prevent_default();
    let mods = modifiers(e);
    bridge.drag_mods.set(Modifiers::NONE);
    if let Ok(leave) = DragEvent::new("dragleave") {
        let _ = canvas.dispatch_event(&leave);
    }
    focus_canvas();

    let had_dirs = tops.iter().any(|t| matches!(t, Top::Dir(_)));
    let walk = bridge.walk.get();
    let bridge = bridge.clone();
    if had_dirs {
        bridge.exploring.set(bridge.exploring.get() + 1);
        bridge.request_repaint();
    }
    wasm_bindgen_futures::spawn_local(async move {
        let mut inputs = Vec::new();
        for top in tops {
            match top {
                Top::File(f) => {
                    let name = f.name();
                    inputs.push(WebInput::new(f, name));
                }
                Top::Dir(d) => walk_dir(d, walk, &mut inputs).await,
            }
        }
        if had_dirs {
            bridge.exploring.set(bridge.exploring.get().saturating_sub(1));
        }
        bridge.deliver(inputs, Some(mods), InputOrigin::Drop, had_dirs).await;
    });
}

/// Esplora una cartella in profondità, fratelli in ordine di nome (come `walkdir` sul desktop).
async fn walk_dir(root: FileSystemDirectoryEntry, walk: WalkOptions, out: &mut Vec<WebInput>) {
    let mut stack: Vec<std::vec::IntoIter<Child>> = vec![read_dir(&root, walk).await.into_iter()];
    while let Some(frame) = stack.last_mut() {
        match frame.next() {
            None => {
                stack.pop();
            }
            Some(Child::File(input)) => out.push(input),
            Some(Child::Dir(dir)) => {
                let children = read_dir(&dir, walk).await;
                stack.push(children.into_iter());
            }
        }
    }
}

/// Voci di una cartella: file già risolti in `File`, sottocartelle solo con la ricorsione.
async fn read_dir(dir: &FileSystemDirectoryEntry, walk: WalkOptions) -> Vec<Child> {
    let reader = dir.create_reader();
    let mut entries: Vec<FileSystemEntry> = Vec::new();
    loop {
        let batch = Promise::new(&mut |resolve, reject| {
            if let Err(e) = reader.read_entries_with_callback_and_callback(&resolve, &reject) {
                let _ = reject.call1(&JsValue::NULL, &e);
            }
        });
        match JsFuture::from(batch).await {
            Ok(v) => {
                let arr: Array = v.unchecked_into();
                if arr.length() == 0 {
                    break;
                }
                entries.extend(arr.iter().map(|v| v.unchecked_into::<FileSystemEntry>()));
            }
            Err(e) => {
                log::warn!("cartella {} non leggibile: {}", dir.full_path(), js_error_text(&e));
                break;
            }
        }
    }
    let mut named: Vec<(String, FileSystemEntry)> = entries
        .into_iter()
        .map(|e| (e.name(), e))
        .filter(|(name, _)| walk.include_hidden || !name.starts_with('.'))
        .collect();
    named.sort_by(|a, b| a.0.cmp(&b.0));

    // Tutti i `File` della cartella in un'unica attesa (`Promise.allSettled`).
    let promises = Array::new();
    for (_, e) in named.iter().filter(|(_, e)| e.is_file()) {
        let fe: &FileSystemFileEntry = e.unchecked_ref();
        promises.push(&Promise::new(&mut |resolve, reject| {
            fe.file_with_callback_and_callback(&resolve, &reject);
        }));
    }
    let settled: Vec<JsValue> = match JsFuture::from(Promise::all_settled(&promises)).await {
        Ok(v) => v.unchecked_into::<Array>().iter().collect(),
        Err(e) => {
            log::warn!("file di {} non leggibili: {}", dir.full_path(), js_error_text(&e));
            Vec::new()
        }
    };
    let mut files = settled.into_iter();

    let mut out = Vec::with_capacity(named.len());
    for (name, e) in named {
        if e.is_file() {
            let Some(result) = files.next() else { continue };
            let value = js_sys::Reflect::get(&result, &JsValue::from_str("value")).unwrap_or(JsValue::UNDEFINED);
            match value.dyn_into::<web_sys::File>() {
                Ok(f) => {
                    let full = e.full_path();
                    let rel = full.trim_start_matches('/');
                    out.push(Child::File(WebInput::new(
                        f,
                        if rel.is_empty() { name } else { rel.to_owned() },
                    )));
                }
                Err(_) => {
                    let reason = js_sys::Reflect::get(&result, &JsValue::from_str("reason")).unwrap_or(JsValue::NULL);
                    log::warn!("file {} non leggibile: {}", e.full_path(), js_error_text(&reason));
                }
            }
        } else if e.is_directory() && walk.recurse {
            out.push(Child::Dir(e.unchecked_into()));
        }
    }
    out
}
