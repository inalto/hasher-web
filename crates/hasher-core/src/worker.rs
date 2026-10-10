//! Entry point del Web Worker che calcola gli hash (feature `worker`, solo wasm32).
//!
//! Il worker è avviato da `web/worker.js` (`import init, { worker_main } from './core.js'`):
//! [`worker_main`] installa `onmessage` sul `DedicatedWorkerGlobalScope` corrente e invia subito
//! `{ ev: "ready" }`. Il thread principale deve attendere `ready` prima di inviare comandi: i
//! messaggi arrivati mentre il modulo wasm si inizializza non hanno ancora un gestore.
//!
//! # Protocollo (docs/PIANO.md §5.3)
//!
//! Messaggi come oggetti JS via `postMessage` (letti e scritti con `js_sys::Reflect`).
//!
//! In ingresso (dal thread principale):
//! - `{ cmd: "hash", task: number, file: File, algos: string[], chunk?: number }`
//!   calcola gli algoritmi indicati (`Algo::id`) leggendo `file` a blocchi di `chunk` byte
//!   (default 4 MiB, limitato a 64 KiB..=64 MiB) con `File.slice(..).arrayBuffer()`: il file non è
//!   mai in memoria per intero;
//! - `{ cmd: "cancel", task: number }` annulla il task (`-1`: tutti, in corso e in coda).
//!
//! In uscita (verso il thread principale):
//! - `{ ev: "ready" }` appena il worker è inizializzato;
//! - `{ ev: "started", task }` quando il task passa in esecuzione;
//! - `{ ev: "progress", task, bytes }` al massimo ogni 50 ms (≤ 20 al secondo per task);
//! - `{ ev: "done", task, bytes, elapsed_ms, digests: { "<algo id>": "<hex minuscolo>", … } }`
//!   (`bytes` = byte effettivamente letti, `elapsed_ms` misurato con `performance.now()`);
//! - `{ ev: "failed", task, error: string }` (comando non valido, `NotReadableError` se il file è
//!   stato modificato o non è più leggibile, …);
//! - `{ ev: "cancelled", task }`.
//!
//! # Esecuzione
//!
//! Un solo task alla volta per worker; gli altri attendono in una coda FIFO
//! ([`TaskQueue`]). Il thread principale ne invia normalmente uno per volta, la coda serve solo
//! per robustezza. Ogni task emette esattamente uno fra `done`, `failed` e `cancelled`; un task
//! annullato mentre è ancora in coda emette solo `cancelled` (senza `started`).
//!
//! La lettura è un ciclo `async` (`wasm_bindgen_futures::spawn_local`): i messaggi `cancel`
//! vengono consegnati durante gli `await` di `arrayBuffer()` e il flag di annullamento è
//! controllato dopo ogni lettura e prima del risultato finale, quindi un task annullato non emette
//! `done`. Un `cancel` che arriva dopo `done` (task già concluso) viene ignorato.
//!
//! Niente `std::time` (su wasm32 `Instant::now()` va in panic): i tempi vengono da
//! `performance.now()` del worker. Nessun `unwrap` su valori provenienti da JavaScript: ogni
//! anomalia diventa un evento `failed` (o un messaggio in console se manca perfino `task`).

use crate::algo::Algo;
use crate::worker_protocol::{
    chunk_size, cmd, digest_entries, ev, parse_algo_ids, parse_cancel_target, parse_task_id, ProgressThrottle,
    TaskHasher, TaskId, TaskQueue, UPDATE_BLOCK,
};
use js_sys::{Array, ArrayBuffer, Object, Reflect, Uint8Array};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{console, Blob, DedicatedWorkerGlobalScope, MessageEvent, Performance};

/// Avvia il ciclo di ricezione dei messaggi nel `DedicatedWorkerGlobalScope` corrente e invia
/// `{ ev: "ready" }`. Va chiamata una sola volta, dopo `init()` del modulo wasm.
///
/// Fuori da un Web Worker dedicato solleva un'eccezione JavaScript.
#[wasm_bindgen]
pub fn worker_main() {
    console_error_panic_hook::set_once();
    let scope = match js_sys::global().dyn_into::<DedicatedWorkerGlobalScope>() {
        Ok(scope) => scope,
        Err(_) => wasm_bindgen::throw_str("hasher worker: worker_main() must run in a dedicated Web Worker"),
    };
    let performance = scope.performance();
    let worker = Rc::new(Worker {
        scope,
        performance,
        tasks: RefCell::new(TaskQueue::new()),
        pumping: Cell::new(false),
        buffer: RefCell::new(Vec::new()),
    });
    let handler = {
        let worker = Rc::clone(&worker);
        Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| worker.on_message(&event.data()))
    };
    worker.scope.set_onmessage(Some(handler.as_ref().unchecked_ref()));
    // Il gestore resta installato per tutta la vita del worker.
    handler.forget();
    worker.post(&message(ev::READY, None));
}

/// Comando `hash` già validato, in attesa o in esecuzione.
struct Job {
    /// `File` (o qualsiasi `Blob`) da leggere a blocchi.
    file: Blob,
    algos: Vec<Algo>,
    chunk: usize,
}

/// Esito di un task in esecuzione.
enum Outcome {
    Done {
        bytes: u64,
        elapsed_ms: f64,
        digests: Vec<(Algo, Vec<u8>)>,
    },
    Cancelled,
}

/// Stato del worker, condiviso (in un `Rc`) fra il gestore `onmessage` e il ciclo asincrono.
/// Nessun `borrow` dei `RefCell` attraversa un `await`.
struct Worker {
    scope: DedicatedWorkerGlobalScope,
    performance: Option<Performance>,
    tasks: RefCell<TaskQueue<Job>>,
    /// `true` mentre il ciclo asincrono che svuota la coda è attivo.
    pumping: Cell<bool>,
    /// Buffer riutilizzato fra un blocco e l'altro (e fra un task e l'altro) per i dati letti.
    buffer: RefCell<Vec<u8>>,
}

impl Worker {
    /// Smista un messaggio ricevuto dal thread principale.
    fn on_message(self: &Rc<Self>, data: &JsValue) {
        match get(data, "cmd").as_string().as_deref() {
            Some(cmd::HASH) => self.on_hash(data),
            Some(cmd::CANCEL) => self.on_cancel(data),
            _ => console::warn_2(&JsValue::from_str("hasher worker: unknown message"), data),
        }
    }

    fn on_hash(self: &Rc<Self>, data: &JsValue) {
        let Some(task) = get(data, "task").as_f64().and_then(parse_task_id) else {
            console::error_2(&JsValue::from_str("hasher worker: `hash` without a valid `task`"), data);
            return;
        };
        match parse_job(data) {
            Ok(job) => {
                self.tasks.borrow_mut().push(task, job);
                self.pump();
            }
            Err(error) => self.post(&failed(task, &error)),
        }
    }

    fn on_cancel(&self, data: &JsValue) {
        let Some(target) = get(data, "task").as_f64().and_then(parse_cancel_target) else {
            console::error_2(
                &JsValue::from_str("hasher worker: `cancel` without a valid `task`"),
                data,
            );
            return;
        };
        // Il task in esecuzione (se coinvolto) risponde dal proprio ciclo; quelli in coda subito.
        let removed = self.tasks.borrow_mut().cancel(target);
        for task in removed {
            self.post(&message(ev::CANCELLED, Some(task)));
        }
    }

    /// Avvia, se non è già attivo, il ciclo asincrono che esegue i task in coda uno alla volta.
    fn pump(self: &Rc<Self>) {
        if self.pumping.replace(true) {
            return;
        }
        let worker = Rc::clone(self);
        spawn_local(async move {
            loop {
                let next = worker.tasks.borrow_mut().start_next();
                let Some((task, job)) = next else {
                    break;
                };
                worker.run(task, &job).await;
                worker.tasks.borrow_mut().finish();
            }
            // Fra l'ultimo `start_next` e qui non ci sono `await`: nessun messaggio può essersi
            // inserito, quindi la coda è davvero vuota.
            worker.pumping.set(false);
        });
    }

    /// Esegue un task ed emette `started` e poi esattamente uno fra `done`, `failed`, `cancelled`.
    async fn run(&self, task: TaskId, job: &Job) {
        self.post(&message(ev::STARTED, Some(task)));
        let reply = match self.hash(task, job).await {
            Ok(Outcome::Done {
                bytes,
                elapsed_ms,
                digests,
            }) => done(task, bytes, elapsed_ms, &digests),
            Ok(Outcome::Cancelled) => message(ev::CANCELLED, Some(task)),
            Err(error) => failed(task, &error),
        };
        self.post(&reply);
    }

    /// Ciclo di lettura: `slice` → `arrayBuffer()` → copia nel buffer riutilizzato → hasher.
    async fn hash(&self, task: TaskId, job: &Job) -> Result<Outcome, String> {
        let started_at = self.now();
        let size = job.file.size();
        let mut hasher = TaskHasher::new(&job.algos);
        let mut throttle = ProgressThrottle::new(started_at);
        let mut offset = 0.0_f64;
        while offset < size {
            if self.cancel_requested() {
                return Ok(Outcome::Cancelled);
            }
            let end = (offset + job.chunk as f64).min(size);
            let piece = job
                .file
                .slice_with_f64_and_f64(offset, end)
                .map_err(describe_js_error)?;
            let buffer = JsFuture::from(piece.array_buffer()).await.map_err(describe_js_error)?;
            // Durante l'`await` può essere arrivato un `cancel`.
            if self.cancel_requested() {
                return Ok(Outcome::Cancelled);
            }
            if !buffer.is_instance_of::<ArrayBuffer>() {
                return Err("arrayBuffer() did not return an ArrayBuffer".to_string());
            }
            let view = Uint8Array::new(&buffer);
            let len = view.length() as usize;
            if len == 0 {
                // Il file si è accorciato: si usano i byte effettivamente letti.
                break;
            }
            {
                let mut data = self.buffer.borrow_mut();
                if data.len() < len {
                    data.resize(len, 0);
                }
                view.copy_to(&mut data[..len]);
                // A sotto-blocchi: i dati restano in cache fra un algoritmo e l'altro e con
                // algoritmi lenti l'avanzamento arriva anche a metà del blocco.
                for block in data[..len].chunks(UPDATE_BLOCK) {
                    hasher.update(block);
                    if throttle.ready(self.now()) {
                        self.post(&progress(task, hasher.bytes()));
                    }
                }
            }
            offset += len as f64;
        }
        if self.cancel_requested() {
            return Ok(Outcome::Cancelled);
        }
        let bytes = hasher.bytes();
        let digests = hasher.finalize();
        Ok(Outcome::Done {
            bytes,
            elapsed_ms: self.now() - started_at,
            digests,
        })
    }

    fn cancel_requested(&self) -> bool {
        self.tasks.borrow().is_running_cancelled()
    }

    /// Millisecondi da `performance.now()` (in mancanza, `Date.now()`).
    fn now(&self) -> f64 {
        match &self.performance {
            Some(p) => p.now(),
            None => js_sys::Date::now(),
        }
    }

    fn post(&self, msg: &Object) {
        if let Err(err) = self.scope.post_message(msg) {
            console::error_2(&JsValue::from_str("hasher worker: postMessage failed"), &err);
        }
    }
}

/// Valida un comando `hash`: `file` deve essere un `File`/`Blob`, `algos` un array di stringhe.
fn parse_job(data: &JsValue) -> Result<Job, String> {
    let file = get(data, "file")
        .dyn_into::<Blob>()
        .map_err(|_| "`file` is missing or is not a File".to_string())?;
    let algos = get(data, "algos");
    if !Array::is_array(&algos) {
        return Err("`algos` must be an array of algorithm ids".to_string());
    }
    let ids = Array::from(&algos)
        .iter()
        .map(|v| {
            v.as_string()
                .ok_or_else(|| "`algos` must contain only strings".to_string())
        })
        .collect::<Result<Vec<String>, String>>()?;
    let algos = parse_algo_ids(&ids)?;
    let chunk = chunk_size(get(data, "chunk").as_f64());
    Ok(Job { file, algos, chunk })
}

/// `target[key]`, oppure `undefined` se `target` non è un oggetto o la lettura fallisce.
fn get(target: &JsValue, key: &str) -> JsValue {
    Reflect::get(target, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

/// `target[key] = value` su un oggetto creato qui (non può fallire: l'esito si ignora).
fn set(target: &Object, key: &str, value: &JsValue) {
    let _ = Reflect::set(target, &JsValue::from_str(key), value);
}

/// `{ ev, task? }`
fn message(event: &str, task: Option<TaskId>) -> Object {
    let msg = Object::new();
    set(&msg, "ev", &JsValue::from_str(event));
    if let Some(task) = task {
        set(&msg, "task", &JsValue::from_f64(task as f64));
    }
    msg
}

/// `{ ev: "progress", task, bytes }`
fn progress(task: TaskId, bytes: u64) -> Object {
    let msg = message(ev::PROGRESS, Some(task));
    set(&msg, "bytes", &JsValue::from_f64(bytes as f64));
    msg
}

/// `{ ev: "done", task, bytes, elapsed_ms, digests: { "<id>": "<hex>" } }`
fn done(task: TaskId, bytes: u64, elapsed_ms: f64, digests: &[(Algo, Vec<u8>)]) -> Object {
    let msg = message(ev::DONE, Some(task));
    set(&msg, "bytes", &JsValue::from_f64(bytes as f64));
    set(&msg, "elapsed_ms", &JsValue::from_f64(elapsed_ms.max(0.0)));
    let map = Object::new();
    for (id, hex) in digest_entries(digests) {
        set(&map, id, &JsValue::from_str(&hex));
    }
    set(&msg, "digests", &map);
    msg
}

/// `{ ev: "failed", task, error }`
fn failed(task: TaskId, error: &str) -> Object {
    let msg = message(ev::FAILED, Some(task));
    set(&msg, "error", &JsValue::from_str(error));
    msg
}

/// Testo leggibile di un errore JavaScript (`DOMException`, `Error`, stringa…), es.
/// `NotReadableError: The requested file could not be read…`.
fn describe_js_error(err: JsValue) -> String {
    let name = get(&err, "name").as_string().filter(|s| !s.is_empty());
    let text = get(&err, "message").as_string().filter(|s| !s.is_empty());
    match (name, text) {
        (Some(name), Some(text)) => format!("{name}: {text}"),
        (Some(only), None) | (None, Some(only)) => only,
        (None, None) => err.as_string().unwrap_or_else(|| format!("{err:?}")),
    }
}
