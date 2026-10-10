//! Gruppo di Web Worker e pianificazione dei task (docs/PIANO.md §5.3).

use super::{Counters, EngineConfig, EngineEvent, EngineHandle, EventQueue, FileId, FileResult, WebInput};
use hasher_core::algo::Algo;
use hasher_core::fileinfo::FileInfo;
use hasher_core::worker_protocol::{cmd, decode_digest, ev};
use js_sys::{Array, Object, Reflect};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::rc::{Rc, Weak};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_time::Instant;

/// Script del worker, relativo alla pagina (funziona anche sotto un prefisso come `/hasher/`).
const WORKER_URL: &str = "worker.js";
/// Massimo numero di worker in automatico.
const AUTO_MAX_WORKERS: usize = 8;
/// Limite di sicurezza per valori fuori scala.
const MAX_WORKERS: usize = 16;
/// Sotto questa dimensione non conviene far rileggere il file a più worker.
const SPLIT_MIN_BYTES: u64 = 8 << 20;

/// Motore web: possiede il gruppo di worker (creato una volta e riusato fra le sessioni).
#[derive(Clone)]
pub struct WebEngine {
    inner: Rc<RefCell<Inner>>,
}

pub(crate) struct Inner {
    ctx: egui::Context,
    me: Weak<RefCell<Inner>>,
    slots: Vec<Slot>,
    /// Generazione del gruppo: i messaggi di worker sostituiti vengono ignorati.
    generation: u32,
    next_task: u32,
    next_session: u64,
    session: Option<Session>,
}

struct Slot {
    worker: web_sys::Worker,
    ready: bool,
    dead: bool,
    /// Task in esecuzione (anche di una sessione già annullata, finché il worker non risponde).
    busy: Option<u32>,
    _on_message: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _on_error: Closure<dyn FnMut(web_sys::Event)>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.worker.set_onmessage(None);
        self.worker.set_onerror(None);
        self.worker.terminate();
    }
}

struct Session {
    id: u64,
    state: Rc<Counters>,
    queue: EventQueue,
    started: Instant,
    files: Vec<FileJob>,
    /// Task non ancora assegnati a un worker: (indice del file, indice della parte).
    pending: VecDeque<(usize, usize)>,
    /// Task in esecuzione: id → (indice del file, indice della parte).
    running: HashMap<u32, (usize, usize)>,
    ok: usize,
    failed: usize,
}

struct FileJob {
    id: FileId,
    input: WebInput,
    size: u64,
    parts: Vec<Part>,
    started_at: Option<Instant>,
    /// Byte calcolati (minimo fra le parti).
    progress: u64,
    digests: BTreeMap<Algo, Vec<u8>>,
    finished: bool,
}

struct Part {
    algos: Vec<Algo>,
    bytes: u64,
    done: bool,
}

impl WebEngine {
    /// Crea il motore e avvia subito i worker (`parallel_files` come nelle impostazioni), così
    /// sono pronti quando arriva il primo file.
    pub fn new(ctx: &egui::Context, parallel_files: usize) -> Self {
        let inner = Rc::new_cyclic(|me| {
            RefCell::new(Inner {
                ctx: ctx.clone(),
                me: me.clone(),
                slots: Vec::new(),
                generation: 0,
                next_task: 1,
                next_session: 1,
                session: None,
            })
        });
        if let Err(e) = inner.borrow_mut().ensure_pool(worker_count(parallel_files)) {
            log::error!("impossibile avviare i Web Worker: {e}");
        }
        WebEngine { inner }
    }

    /// Avvia una sessione e restituisce subito la maniglia e la coda degli eventi.
    /// Una sessione precedente ancora attiva viene annullata.
    pub fn start(&self, inputs: Vec<WebInput>, config: EngineConfig) -> Result<(EngineHandle, EventQueue), String> {
        let mut inner = self.inner.try_borrow_mut().map_err(|_| "motore occupato".to_owned())?;
        if let Some(old) = inner.session.as_ref().map(|s| s.id) {
            inner.cancel(old);
        }
        inner.ensure_pool(worker_count(config.parallel_files))?;
        let live = inner.slots.iter().filter(|s| !s.dead).count().max(1);

        let mut algos: Vec<Algo> = Vec::with_capacity(config.algos.len());
        for a in config.algos {
            if !algos.contains(&a) {
                algos.push(a);
            }
        }
        algos.sort();

        let state = Rc::new(Counters::default());
        state.running.set(true);
        let queue: EventQueue = Rc::default();
        let id = inner.next_session;
        inner.next_session += 1;

        let n_files = inputs.len();
        let per_file = if n_files == 0 { 1 } else { live.div_ceil(n_files) };
        let mut files = Vec::with_capacity(n_files);
        let mut pending = VecDeque::new();
        {
            let mut q = queue.borrow_mut();
            for (i, input) in inputs.into_iter().enumerate() {
                let file_id = (i + 1) as FileId;
                let size = input.size();
                Counters::add(&state.files_total, 1);
                Counters::add(&state.bytes_total, size);
                q.push_back(EngineEvent::Discovered {
                    id: file_id,
                    path: input.path(),
                    size,
                });
                let k = if n_files >= live || size < SPLIT_MIN_BYTES {
                    1
                } else {
                    per_file.min(algos.len()).max(1)
                };
                let parts: Vec<Part> = split_algos(&algos, k)
                    .into_iter()
                    .map(|algos| Part {
                        algos,
                        bytes: 0,
                        done: false,
                    })
                    .collect();
                for p in 0..parts.len() {
                    pending.push_back((i, p));
                }
                files.push(FileJob {
                    id: file_id,
                    input,
                    size,
                    parts,
                    started_at: None,
                    progress: 0,
                    digests: BTreeMap::new(),
                    finished: false,
                });
            }
        }

        inner.session = Some(Session {
            id,
            state: state.clone(),
            queue: queue.clone(),
            started: Instant::now(),
            files,
            pending,
            running: HashMap::new(),
            ok: 0,
            failed: 0,
        });
        let handle = EngineHandle {
            state,
            engine: inner.me.clone(),
            session: id,
        };
        inner.after_change();
        Ok((handle, queue))
    }
}

/// Numero di worker per l'impostazione "file in parallelo" (0 = automatico).
pub(crate) fn worker_count(requested: usize) -> usize {
    if requested > 0 {
        return requested.min(MAX_WORKERS);
    }
    let cores = web_sys::window()
        .map(|w| w.navigator().hardware_concurrency())
        .filter(|n| n.is_finite() && *n >= 1.0)
        .map(|n| n as usize)
        .unwrap_or(4);
    cores.clamp(1, AUTO_MAX_WORKERS)
}

/// Costo relativo (per byte) di un algoritmo in wasm, per bilanciare le parti di un file.
fn cost(a: Algo) -> u32 {
    match a {
        Algo::Md2 => 400,
        Algo::Sha3_512 => 14,
        Algo::Sha3_256 => 8,
        Algo::Sha224 | Algo::Sha256 => 7,
        Algo::Ripemd160 | Algo::Ripemd320 => 6,
        Algo::Blake2s256 => 5,
        Algo::Sha384 | Algo::Sha512 | Algo::Ripemd128 | Algo::Ripemd256 => 4,
        Algo::Md5 | Algo::Sha1 | Algo::Blake2b512 => 3,
        Algo::Md4 | Algo::Ed2k | Algo::Crc64 | Algo::Blake3 => 2,
        Algo::Crc32 | Algo::Xxh64 | Algo::Xxh3_128 => 1,
    }
}

/// Ripartisce gli algoritmi in `k` gruppi di costo simile (il più costoso al gruppo più
/// scarico); i gruppi vuoti vengono scartati. Con `k = 1` restituisce tutti gli algoritmi.
fn split_algos(algos: &[Algo], k: usize) -> Vec<Vec<Algo>> {
    if k <= 1 || algos.len() <= 1 {
        return vec![algos.to_vec()];
    }
    let mut sorted = algos.to_vec();
    sorted.sort_by_key(|a| std::cmp::Reverse(cost(*a)));
    let mut groups: Vec<(u32, Vec<Algo>)> = vec![(0, Vec::new()); k];
    for a in sorted {
        if let Some(g) = groups.iter_mut().min_by_key(|g| g.0) {
            g.0 += cost(a) + 1; // +1: ogni parte rilegge il file
            g.1.push(a);
        }
    }
    groups
        .into_iter()
        .filter(|g| !g.1.is_empty())
        .map(|(_, mut v)| {
            v.sort();
            v
        })
        .collect()
}

fn js_error_text(e: &JsValue) -> String {
    if let Some(s) = e.as_string() {
        return s;
    }
    if let Some(err) = e.dyn_ref::<js_sys::Error>() {
        return String::from(err.message());
    }
    format!("{e:?}")
}

fn get(obj: &JsValue, key: &str) -> JsValue {
    Reflect::get(obj, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

fn set(obj: &Object, key: &str, value: &JsValue) {
    let _ = Reflect::set(obj, &JsValue::from_str(key), value);
}

impl Inner {
    /// Crea (o ricrea) i worker in modo che siano `n` e tutti vivi.
    fn ensure_pool(&mut self, n: usize) -> Result<(), String> {
        if self.slots.len() != n {
            self.slots.clear();
            self.generation = self.generation.wrapping_add(1);
        }
        for i in 0..n {
            let need = self.slots.get(i).is_none_or(|s| s.dead);
            if need {
                let slot = self.spawn(i)?;
                if i < self.slots.len() {
                    self.slots[i] = slot;
                } else {
                    self.slots.push(slot);
                }
            }
        }
        Ok(())
    }

    fn spawn(&self, index: usize) -> Result<Slot, String> {
        let opts = web_sys::WorkerOptions::new();
        opts.set_type(web_sys::WorkerType::Module);
        let worker = web_sys::Worker::new_with_options(WORKER_URL, &opts).map_err(|e| js_error_text(&e))?;
        let generation = self.generation;

        let me = self.me.clone();
        let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |e: web_sys::MessageEvent| {
            if let Some(rc) = me.upgrade() {
                match rc.try_borrow_mut() {
                    Ok(mut inner) if inner.generation == generation => inner.on_message(index, &e.data()),
                    Ok(_) => {}
                    Err(_) => log::warn!("messaggio del worker ignorato: motore occupato"),
                }
            }
        });
        let me = self.me.clone();
        let on_error = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            e.prevent_default();
            let msg = get(&e, "message").as_string().unwrap_or_default();
            if let Some(rc) = me.upgrade() {
                if let Ok(mut inner) = rc.try_borrow_mut() {
                    if inner.generation == generation {
                        inner.on_worker_error(index, msg);
                    }
                }
            }
        });
        worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        Ok(Slot {
            worker,
            ready: false,
            dead: false,
            busy: None,
            _on_message: on_message,
            _on_error: on_error,
        })
    }

    /// Annulla la sessione `id` (se è quella corrente e non è già conclusa).
    pub(crate) fn cancel_session(rc: &Rc<RefCell<Inner>>, id: u64) {
        match rc.try_borrow_mut() {
            Ok(mut inner) => inner.cancel(id),
            Err(_) => log::warn!("annullamento non riuscito: motore occupato"),
        }
    }

    fn cancel(&mut self, id: u64) {
        if self.session.as_ref().is_none_or(|s| s.id != id) {
            return;
        }
        let Some(s) = self.session.take() else { return };
        for task in s.running.keys() {
            if let Some(slot) = self.slots.iter().find(|sl| sl.busy == Some(*task)) {
                let msg = Object::new();
                set(&msg, "cmd", &JsValue::from_str(cmd::CANCEL));
                set(&msg, "task", &JsValue::from_f64(f64::from(*task)));
                if let Err(e) = slot.worker.post_message(&msg) {
                    log::warn!("cancel non inviato al worker: {}", js_error_text(&e));
                }
            }
        }
        s.state.cancel.set(true);
        s.state.running.set(false);
        s.queue.borrow_mut().push_back(EngineEvent::AllDone {
            files_ok: s.ok,
            files_failed: s.failed,
            cancelled: true,
            elapsed: s.started.elapsed(),
        });
        self.ctx.request_repaint();
    }

    /// Assegna i task in attesa ai worker liberi e chiude la sessione se è tutto finito.
    fn after_change(&mut self) {
        self.dispatch();
        self.fail_if_no_workers();
        self.check_done();
        self.ctx.request_repaint();
    }

    fn dispatch(&mut self) {
        let Some(s) = self.session.as_mut() else { return };
        for slot in self.slots.iter_mut() {
            if !slot.ready || slot.dead || slot.busy.is_some() {
                continue;
            }
            // Salta le parti di file già falliti.
            let next = loop {
                match s.pending.pop_front() {
                    Some((f, p)) if s.files[f].finished => {
                        let _ = p;
                        continue;
                    }
                    other => break other,
                }
            };
            let Some((f, p)) = next else { break };
            let task = self.next_task;
            self.next_task = self.next_task.wrapping_add(1).max(1);
            let file = &s.files[f];
            let algos = Array::new();
            for a in &file.parts[p].algos {
                algos.push(&JsValue::from_str(a.id()));
            }
            let msg = Object::new();
            set(&msg, "cmd", &JsValue::from_str(cmd::HASH));
            set(&msg, "task", &JsValue::from_f64(f64::from(task)));
            set(&msg, "file", &file.input.file);
            set(&msg, "algos", &algos);
            match slot.worker.post_message(&msg) {
                Ok(()) => {
                    slot.busy = Some(task);
                    s.running.insert(task, (f, p));
                }
                Err(e) => {
                    let error = js_error_text(&e);
                    log::error!("postMessage al worker non riuscito: {error}");
                    fail_file(s, f, error);
                }
            }
        }
    }

    /// Se nessun worker è utilizzabile, i file ancora da calcolare falliscono subito invece di
    /// restare in attesa per sempre.
    fn fail_if_no_workers(&mut self) {
        if !self.slots.is_empty() && self.slots.iter().any(|s| !s.dead) {
            return;
        }
        let Some(s) = self.session.as_mut() else { return };
        let error = t_worker_unavailable();
        for f in 0..s.files.len() {
            if !s.files[f].finished {
                fail_file(s, f, error.clone());
            }
        }
        s.pending.clear();
        s.running.clear();
    }

    fn check_done(&mut self) {
        let done = self
            .session
            .as_ref()
            .is_some_and(|s| s.files.iter().all(|f| f.finished));
        if !done {
            return;
        }
        let Some(s) = self.session.take() else { return };
        s.state.running.set(false);
        s.queue.borrow_mut().push_back(EngineEvent::AllDone {
            files_ok: s.ok,
            files_failed: s.failed,
            cancelled: false,
            elapsed: s.started.elapsed(),
        });
    }

    fn on_worker_error(&mut self, index: usize, message: String) {
        let Some(slot) = self.slots.get_mut(index) else { return };
        log::error!("Web Worker {index} non disponibile: {message}");
        slot.dead = true;
        slot.ready = false;
        let task = slot.busy.take();
        if let (Some(task), Some(s)) = (task, self.session.as_mut()) {
            if let Some((f, _)) = s.running.remove(&task) {
                let error = if message.is_empty() {
                    t_worker_unavailable()
                } else {
                    message
                };
                fail_file(s, f, error);
            }
        }
        self.after_change();
    }

    fn on_message(&mut self, index: usize, data: &JsValue) {
        let kind = get(data, "ev").as_string().unwrap_or_default();
        let task = get(data, "task").as_f64().map(|t| t as u32);
        match kind.as_str() {
            ev::READY => {
                if let Some(slot) = self.slots.get_mut(index) {
                    slot.ready = true;
                }
            }
            ev::STARTED => {
                if let (Some(task), Some(s)) = (task, self.session.as_mut()) {
                    if let Some(&(f, _)) = s.running.get(&task) {
                        let file = &mut s.files[f];
                        if file.started_at.is_none() {
                            file.started_at = Some(Instant::now());
                            s.queue.borrow_mut().push_back(EngineEvent::Started { id: file.id });
                        }
                    }
                }
            }
            ev::PROGRESS => {
                let bytes = get(data, "bytes").as_f64().unwrap_or(0.0).max(0.0) as u64;
                if let (Some(task), Some(s)) = (task, self.session.as_mut()) {
                    if let Some(&(f, p)) = s.running.get(&task) {
                        s.files[f].parts[p].bytes = bytes;
                        update_progress(s, f);
                    }
                }
            }
            ev::DONE => {
                self.free(index, task);
                let bytes = get(data, "bytes").as_f64().unwrap_or(0.0).max(0.0) as u64;
                let digests = get(data, "digests");
                if let (Some(task), Some(s)) = (task, self.session.as_mut()) {
                    if let Some((f, p)) = s.running.remove(&task) {
                        part_done(s, f, p, bytes, &digests);
                    }
                }
            }
            ev::FAILED => {
                self.free(index, task);
                let error = get(data, "error")
                    .as_string()
                    .unwrap_or_else(|| "errore sconosciuto".to_owned());
                if let (Some(task), Some(s)) = (task, self.session.as_mut()) {
                    if let Some((f, _)) = s.running.remove(&task) {
                        fail_file(s, f, error);
                        self.cancel_siblings(f);
                    }
                }
            }
            ev::CANCELLED => {
                self.free(index, task);
                if let (Some(task), Some(s)) = (task, self.session.as_mut()) {
                    s.running.remove(&task);
                }
            }
            other => log::debug!("messaggio sconosciuto dal worker: {other:?}"),
        }
        self.after_change();
    }

    /// Il worker `index` ha concluso `task` (o un task di una sessione precedente).
    fn free(&mut self, index: usize, task: Option<u32>) {
        if let Some(slot) = self.slots.get_mut(index) {
            if task.is_none() || slot.busy == task {
                slot.busy = None;
            }
        }
    }

    /// Ferma le altre parti di un file fallito.
    fn cancel_siblings(&mut self, f: usize) {
        let Some(s) = self.session.as_mut() else { return };
        let tasks: Vec<u32> = s
            .running
            .iter()
            .filter(|(_, (ff, _))| *ff == f)
            .map(|(t, _)| *t)
            .collect();
        for task in tasks {
            s.running.remove(&task);
            if let Some(slot) = self.slots.iter().find(|sl| sl.busy == Some(task)) {
                let msg = Object::new();
                set(&msg, "cmd", &JsValue::from_str(cmd::CANCEL));
                set(&msg, "task", &JsValue::from_f64(f64::from(task)));
                let _ = slot.worker.post_message(&msg);
            }
        }
    }
}

fn t_worker_unavailable() -> String {
    rust_i18n::t!("toast.worker_unavailable").into_owned()
}

/// Ricalcola l'avanzamento del file (minimo fra le parti) e lo comunica se è cresciuto.
fn update_progress(s: &mut Session, f: usize) {
    let file = &mut s.files[f];
    let min = file.parts.iter().map(|p| p.bytes).min().unwrap_or(0);
    if min > file.progress {
        Counters::add(&s.state.bytes_done, min - file.progress);
        file.progress = min;
        s.queue.borrow_mut().push_back(EngineEvent::Progress {
            id: file.id,
            bytes_done: min,
        });
    }
}

fn part_done(s: &mut Session, f: usize, p: usize, bytes: u64, digests: &JsValue) {
    if s.files[f].finished {
        return;
    }
    let mut missing = Vec::new();
    {
        let file = &mut s.files[f];
        let part = &mut file.parts[p];
        part.bytes = bytes;
        part.done = true;
        for a in &part.algos {
            let decoded = get(digests, a.id())
                .as_string()
                .and_then(|h| decode_digest(a.id(), &h))
                .filter(|(algo, _)| algo == a);
            match decoded {
                Some((algo, d)) => {
                    file.digests.insert(algo, d);
                }
                None => missing.push(a.name()),
            }
        }
    }
    if !missing.is_empty() {
        fail_file(s, f, format!("digest mancante: {}", missing.join(", ")));
        return;
    }
    if !s.files[f].parts.iter().all(|p| p.done) {
        update_progress(s, f);
        return;
    }
    // Tutte le parti concluse: risultato del file.
    let file = &mut s.files[f];
    file.finished = true;
    let hashed = file.parts.iter().map(|p| p.bytes).max().unwrap_or(bytes);
    // Allinea il totale ai byte effettivamente letti (come `reconcile_total` del desktop).
    if hashed > file.size {
        Counters::add(&s.state.bytes_total, hashed - file.size);
    } else {
        Counters::sub(&s.state.bytes_total, file.size - hashed);
    }
    if hashed > file.progress {
        Counters::add(&s.state.bytes_done, hashed - file.progress);
    } else {
        Counters::sub(&s.state.bytes_done, file.progress - hashed);
    }
    file.progress = hashed;
    Counters::add(&s.state.files_done, 1);
    s.ok += 1;
    let elapsed = file.started_at.map(|t| t.elapsed()).unwrap_or_default();
    let info = FileInfo::basic(file.input.path(), hashed, file.input.modified());
    let mut q = s.queue.borrow_mut();
    q.push_back(EngineEvent::Progress {
        id: file.id,
        bytes_done: hashed,
    });
    q.push_back(EngineEvent::Finished(Box::new(FileResult {
        id: file.id,
        info,
        digests: std::mem::take(&mut file.digests),
        elapsed,
    })));
}

fn fail_file(s: &mut Session, f: usize, error: String) {
    let file = &mut s.files[f];
    if file.finished {
        return;
    }
    file.finished = true;
    // Il file non verrà completato: il totale scende ai byte già contati.
    Counters::sub(&s.state.bytes_total, file.size.saturating_sub(file.progress));
    Counters::add(&s.state.files_done, 1);
    s.failed += 1;
    s.queue.borrow_mut().push_back(EngineEvent::Failed {
        id: file.id,
        path: file.input.path(),
        error,
    });
}
