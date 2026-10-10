//! Collegamento con la pagina: trascinamento di file e cartelle, dialoghi `<input type="file">`,
//! download, `localStorage`, scorciatoie, installazione della PWA, file aperti dal sistema e
//! appunti.
//!
//! Tutti i listener scrivono in code condivise ([`Bridge`]) e chiedono un repaint: l'app le svuota
//! nel frame successivo, così il codice delle viste resta identico a quello del desktop (azioni
//! accodate ed eseguite a fine frame).

pub mod clipboard;
pub mod dnd;
pub mod install;
pub mod keys;
pub mod launch;
pub mod picker;
pub mod storage;

use crate::engine::WebInput;
use egui::Modifiers;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::Path;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Id del canvas su cui eframe disegna l'interfaccia (`web/index.html`).
pub const CANVAS_ID: &str = "hasher_canvas";

/// File di checksum più grandi di così vengono trattati come file normali (non letti in memoria).
const MAX_CHECKSUM_BYTES: f64 = 64.0 * 1024.0 * 1024.0;

/// Provenienza di un gruppo di file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputOrigin {
    /// Trascinati sulla pagina.
    Drop,
    /// Dialogo "Apri file".
    OpenFiles,
    /// Dialogo "Apri cartella".
    OpenFolder,
    /// Dialogo "Apri file di checksum": il file è sempre interpretato come checksum.
    OpenChecksum,
    /// Aperti dal sistema con la PWA installata (`file_handlers` / `launchQueue`).
    Launch,
}

/// Contenuto di un file di checksum fra gli input (già letto: è piccolo).
#[derive(Debug)]
pub struct ChecksumBytes {
    pub rel_path: String,
    pub bytes: Result<Vec<u8>, String>,
}

/// File scelti dall'utente in un'unica operazione (rilascio, dialogo, apertura dal sistema).
#[derive(Debug)]
pub struct InputBatch {
    /// File già espansi (le cartelle sono state esplorate), nell'ordine di presentazione.
    pub inputs: Vec<WebInput>,
    /// Modificatori al momento del rilascio (`None` per i dialoghi: valgono quelli correnti).
    pub mods: Option<Modifiers>,
    pub origin: InputOrigin,
    /// Fra gli elementi c'erano cartelle.
    pub had_dirs: bool,
    /// File di checksum riconosciuti fra gli input, con il contenuto.
    pub checksums: Vec<ChecksumBytes>,
}

/// Opzioni di esplorazione delle cartelle (dalle impostazioni correnti).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkOptions {
    pub recurse: bool,
    pub include_hidden: bool,
}

/// Stato condiviso fra i listener della pagina e l'app.
#[derive(Clone)]
pub struct Bridge {
    ctx: egui::Context,
    /// Gruppi di file pronti per l'app.
    pub batches: Rc<RefCell<VecDeque<InputBatch>>>,
    /// Esplorazioni di cartelle in corso (stato "Ricerca dei file…").
    pub exploring: Rc<Cell<u32>>,
    /// Testo letto dagli appunti con il pulsante "Incolla" (`Err` se il browser lo nega).
    pub pasted: Rc<RefCell<Option<Result<String, String>>>>,
    /// Opzioni di esplorazione aggiornate dall'app a ogni frame.
    pub walk: Rc<Cell<WalkOptions>>,
    /// Modificatori durante il trascinamento (`dragover`): durante un drag & drop il browser
    /// non invia eventi di tastiera, quindi egui non li conosce.
    pub drag_mods: Rc<Cell<Modifiers>>,
}

impl Bridge {
    /// Installa tutti i listener della pagina. Da chiamare nel costruttore dell'app (prima che
    /// eframe registri i propri listener sul canvas).
    pub fn install(ctx: &egui::Context, walk: WalkOptions) -> Result<Bridge, JsValue> {
        let bridge = Bridge {
            ctx: ctx.clone(),
            batches: Rc::default(),
            exploring: Rc::default(),
            pasted: Rc::default(),
            walk: Rc::new(Cell::new(walk)),
            drag_mods: Rc::default(),
        };
        let canvas = canvas().ok_or_else(|| JsValue::from_str("#hasher_canvas non trovato"))?;
        dnd::install(&bridge, &canvas)?;
        keys::install()?;
        launch::install(&bridge)?;
        install::install(ctx)?;
        Ok(bridge)
    }

    pub fn request_repaint(&self) {
        self.ctx.request_repaint();
    }

    /// Prepara un gruppo di file (legge i file di checksum) e lo consegna all'app.
    pub(crate) async fn deliver(
        &self,
        inputs: Vec<WebInput>,
        mods: Option<Modifiers>,
        origin: InputOrigin,
        had_dirs: bool,
    ) {
        let mut checksums = Vec::new();
        for input in &inputs {
            let forced = origin == InputOrigin::OpenChecksum && inputs.len() == 1;
            let named = hasher_core::checksum_file::is_checksum_file(Path::new(&input.rel_path));
            if (forced || named) && input.file.size() <= MAX_CHECKSUM_BYTES {
                let bytes = read_all(&input.file).await;
                checksums.push(ChecksumBytes {
                    rel_path: input.rel_path.clone(),
                    bytes,
                });
            }
        }
        if inputs.is_empty() && checksums.is_empty() {
            self.request_repaint();
            return;
        }
        self.batches.borrow_mut().push_back(InputBatch {
            inputs,
            mods,
            origin,
            had_dirs,
            checksums,
        });
        self.request_repaint();
    }
}

/// Legge per intero un file piccolo (`File.arrayBuffer()`).
pub(crate) async fn read_all(file: &web_sys::File) -> Result<Vec<u8>, String> {
    let buf = wasm_bindgen_futures::JsFuture::from(file.array_buffer())
        .await
        .map_err(|e| js_error_text(&e))?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

pub fn window() -> Option<web_sys::Window> {
    web_sys::window()
}

pub fn document() -> Option<web_sys::Document> {
    window()?.document()
}

pub fn canvas() -> Option<web_sys::HtmlCanvasElement> {
    document()?.get_element_by_id(CANVAS_ID)?.dyn_into().ok()
}

/// Dà il focus al canvas: senza focus eframe ignora tastiera e `paste`.
pub fn focus_canvas() {
    if let Some(c) = canvas() {
        let _ = c.focus();
    }
}

/// Testo leggibile di un errore JavaScript.
pub fn js_error_text(e: &JsValue) -> String {
    if let Some(s) = e.as_string() {
        return s;
    }
    if let Some(err) = e.dyn_ref::<js_sys::Error>() {
        let name = String::from(err.name());
        let msg = String::from(err.message());
        return if name.is_empty() || name == "Error" {
            msg
        } else {
            format!("{name}: {msg}")
        };
    }
    format!("{e:?}")
}

/// Offre `bytes` come download con il nome dato (`Blob` + `<a download>`).
pub fn download(name: &str, mime: &str, bytes: &[u8]) -> Result<(), String> {
    let doc = document().ok_or("document non disponibile")?;
    let parts = js_sys::Array::new();
    parts.push(&js_sys::Uint8Array::from(bytes));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(|e| js_error_text(&e))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(|e| js_error_text(&e))?;
    let anchor: web_sys::HtmlAnchorElement = doc
        .create_element("a")
        .map_err(|e| js_error_text(&e))?
        .dyn_into()
        .map_err(|_| "elemento <a> non valido")?;
    anchor.set_href(&url);
    anchor.set_download(name);
    let _ = anchor.style().set_property("display", "none");
    if let Some(body) = doc.body() {
        let _ = body.append_child(&anchor);
    }
    anchor.click();
    anchor.remove();
    // Il browser ha già avviato il download: l'URL può essere revocato poco dopo.
    let revoke = Closure::once_into_js(move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    if let Some(w) = window() {
        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 30_000);
    }
    focus_canvas();
    Ok(())
}
