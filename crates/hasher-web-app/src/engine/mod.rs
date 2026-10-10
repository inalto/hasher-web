//! Motore di hashing del web: un gruppo di Web Worker (`web/worker.js` → `hasher-core` in wasm)
//! che leggono i file a blocchi con `File.slice(…).arrayBuffer()`; il file non viene mai caricato
//! per intero e niente esce dal browser.
//!
//! I tipi pubblici riproducono il contratto del motore del desktop (`EngineEvent`, `EngineHandle`,
//! `EngineConfig`), così l'interfaccia resta la stessa:
//! - gli eventi arrivano in una coda [`EventQueue`] (una per sessione) riempita dai callback
//!   `onmessage` dei worker, che chiedono anche un repaint; l'app la svuota in `App::logic`;
//! - gli input sono già espansi ([`WebInput`]: l'esplorazione delle cartelle avviene in
//!   `web::dnd` / `web::picker`), quindi `Discovered` arriva subito per tutti i file;
//! - gli id dei file ripartono da 1 a ogni sessione, come sul desktop.
//!
//! Protocollo con i worker: docs/PIANO.md §5.3. Pianificazione: con almeno tanti file quanti
//! worker ogni file è un task con tutti gli algoritmi; con meno file (in particolare uno solo) gli
//! algoritmi di ciascun file vengono ripartiti in più task su worker diversi (ognuno rilegge il
//! file, servito dalla cache del sistema) e i digest vengono riuniti; l'avanzamento del file è il
//! minimo fra i suoi task.

mod pool;

pub use hasher_core::result::{FileId, FileResult};
pub use pool::WebEngine;

use chrono::{DateTime, Local, TimeZone};
use hasher_core::algo::Algo;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

/// Un file scelto dall'utente: l'oggetto `File` del browser e il percorso mostrato
/// (nome del file, oppure percorso relativo alla cartella trascinata o aperta).
#[derive(Debug, Clone)]
pub struct WebInput {
    pub file: web_sys::File,
    pub rel_path: String,
}

impl WebInput {
    pub fn new(file: web_sys::File, rel_path: impl Into<String>) -> Self {
        let rel_path = rel_path.into();
        let rel_path = if rel_path.is_empty() { file.name() } else { rel_path };
        Self { file, rel_path }
    }

    /// Dimensione in byte dichiarata dal browser.
    pub fn size(&self) -> u64 {
        let s = self.file.size();
        if s.is_finite() && s > 0.0 {
            s as u64
        } else {
            0
        }
    }

    /// Data di modifica (`File.lastModified`, millisecondi Unix) nel fuso locale.
    pub fn modified(&self) -> Option<DateTime<Local>> {
        let ms = self.file.last_modified();
        if !ms.is_finite() || ms <= 0.0 {
            return None;
        }
        Local.timestamp_millis_opt(ms as i64).single()
    }

    pub fn path(&self) -> PathBuf {
        PathBuf::from(&self.rel_path)
    }
}

/// Parametri di una sessione di calcolo.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub algos: Vec<Algo>,
    /// Numero di Web Worker; 0 = automatico (`navigator.hardwareConcurrency`, da 1 a 8).
    pub parallel_files: usize,
    /// Esplorazione ricorsiva delle cartelle. Sul web gli input arrivano già espansi: l'opzione
    /// è applicata da `web::dnd` / `web::picker` e resta qui per il contratto del desktop.
    #[allow(dead_code)]
    pub recurse: bool,
    /// Include file e cartelle il cui nome inizia con `.` (come `recurse`).
    #[allow(dead_code)]
    pub include_hidden: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            algos: Algo::DEFAULT.to_vec(),
            parallel_files: 0,
            recurse: true,
            include_hidden: false,
        }
    }
}

/// Eventi inviati all'interfaccia (stesso contratto del desktop).
#[derive(Debug)]
pub enum EngineEvent {
    /// Un file è stato messo in coda. `path` è il percorso relativo.
    Discovered { id: FileId, path: PathBuf, size: u64 },
    /// Inizio del calcolo per il file.
    Started { id: FileId },
    /// Avanzamento (al massimo ~20 volte al secondo per task).
    Progress { id: FileId, bytes_done: u64 },
    /// Calcolo completato.
    Finished(Box<FileResult>),
    /// Errore (file non leggibile, modificato durante la lettura, worker non disponibile…).
    Failed { id: FileId, path: PathBuf, error: String },
    /// Tutti i file sono stati elaborati (o la sessione è stata annullata).
    AllDone {
        files_ok: usize,
        files_failed: usize,
        cancelled: bool,
        elapsed: Duration,
    },
}

/// Coda degli eventi di una sessione, condivisa fra i callback dei worker e l'app.
pub type EventQueue = Rc<RefCell<VecDeque<EngineEvent>>>;

/// Contatori della sessione (sul web tutto gira nel thread dell'interfaccia: bastano `Cell`).
#[derive(Debug, Default)]
pub(crate) struct Counters {
    pub cancel: Cell<bool>,
    pub running: Cell<bool>,
    pub bytes_done: Cell<u64>,
    pub bytes_total: Cell<u64>,
    pub files_total: Cell<u64>,
    pub files_done: Cell<u64>,
}

impl Counters {
    pub fn add(cell: &Cell<u64>, n: u64) {
        cell.set(cell.get().saturating_add(n));
    }
    pub fn sub(cell: &Cell<u64>, n: u64) {
        cell.set(cell.get().saturating_sub(n));
    }
}

/// Maniglia condivisa con l'interfaccia per annullare e leggere l'avanzamento complessivo.
#[derive(Clone, Default)]
pub struct EngineHandle {
    pub(crate) state: Rc<Counters>,
    pub(crate) engine: Weak<RefCell<pool::Inner>>,
    pub(crate) session: u64,
}

impl std::fmt::Debug for EngineHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineHandle")
            .field("session", &self.session)
            .field("state", &self.state)
            .finish()
    }
}

impl EngineHandle {
    /// Annulla la sessione: i worker ricevono `cancel` e in coda arriva subito
    /// `AllDone { cancelled: true }` (se la sessione non era già conclusa).
    pub fn cancel(&self) {
        self.state.cancel.set(true);
        if let Some(engine) = self.engine.upgrade() {
            pool::Inner::cancel_session(&engine, self.session);
        }
    }
    #[allow(dead_code)] // contratto del desktop
    pub fn is_cancelled(&self) -> bool {
        self.state.cancel.get()
    }
    #[allow(dead_code)] // contratto del desktop
    pub fn is_running(&self) -> bool {
        self.state.running.get()
    }
    pub fn bytes_done(&self) -> u64 {
        self.state.bytes_done.get()
    }
    pub fn bytes_total(&self) -> u64 {
        self.state.bytes_total.get()
    }
    pub fn files_total(&self) -> u64 {
        self.state.files_total.get()
    }
    pub fn files_done(&self) -> u64 {
        self.state.files_done.get()
    }
    /// Frazione completata 0..=1 (0 se il totale non è ancora noto).
    pub fn fraction(&self) -> f32 {
        let t = self.bytes_total();
        if t == 0 {
            0.0
        } else {
            (self.bytes_done() as f64 / t as f64).min(1.0) as f32
        }
    }
}
