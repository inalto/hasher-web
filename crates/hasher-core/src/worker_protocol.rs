//! Parti pure (senza JavaScript) del protocollo del Web Worker (docs/PIANO.md §5.3).
//!
//! Il modulo `worker` (feature `worker`, solo wasm32) si occupa di `postMessage`, `File.slice` e
//! delle promesse; qui restano le regole che non dipendono dal browser, compilate su ogni target e
//! quindi verificabili con un normale `cargo test`:
//!
//! - nomi dei comandi ([`cmd`]) e degli eventi ([`ev`]);
//! - interpretazione di `task`, `algos` e `chunk` ([`parse_task_id`], [`parse_cancel_target`],
//!   [`parse_algo_ids`], [`chunk_size`]);
//! - coda dei task con annullamento ([`TaskQueue`]): un solo task in esecuzione, gli altri in
//!   attesa in ordine FIFO;
//! - cadenza degli eventi `progress` ([`ProgressThrottle`]);
//! - calcolo contemporaneo di più algoritmi e codifica dei digest ([`TaskHasher`],
//!   [`digest_entries`], [`decode_digest`]).
//!
//! Il thread principale può usare le stesse costanti e [`decode_digest`] per interpretare l'evento
//! `done` senza duplicare le regole.

use crate::algo::{to_hex, Algo};
use crate::hasher::{build, StreamHasher};
use std::collections::VecDeque;

/// Valori del campo `cmd` dei messaggi in ingresso (thread principale → worker).
pub mod cmd {
    /// `{ cmd: "hash", task, file, algos, chunk? }`
    pub const HASH: &str = "hash";
    /// `{ cmd: "cancel", task }` (`task = -1`: tutti)
    pub const CANCEL: &str = "cancel";
}

/// Valori del campo `ev` dei messaggi in uscita (worker → thread principale).
pub mod ev {
    /// `{ ev: "ready" }`: il worker è inizializzato e accetta comandi.
    pub const READY: &str = "ready";
    /// `{ ev: "started", task }`
    pub const STARTED: &str = "started";
    /// `{ ev: "progress", task, bytes }`
    pub const PROGRESS: &str = "progress";
    /// `{ ev: "done", task, bytes, elapsed_ms, digests: { "<algo id>": "<hex minuscolo>" } }`
    pub const DONE: &str = "done";
    /// `{ ev: "failed", task, error }`
    pub const FAILED: &str = "failed";
    /// `{ ev: "cancelled", task }`
    pub const CANCELLED: &str = "cancelled";
}

/// Dimensione di default del blocco letto con `File.slice(…).arrayBuffer()`: 4 MiB.
pub const DEFAULT_CHUNK: usize = 4 * 1024 * 1024;

/// Blocco minimo accettato per `chunk` (valori più piccoli vengono portati a questo).
pub const MIN_CHUNK: usize = 64 * 1024;

/// Blocco massimo accettato per `chunk` (limita la memoria del worker).
pub const MAX_CHUNK: usize = 64 * 1024 * 1024;

/// Sotto-blocco con cui ogni blocco letto viene passato agli hasher. Tutti gli algoritmi elaborano
/// lo stesso sotto-blocco mentre è ancora in cache, e l'avanzamento può essere segnalato anche a
/// metà di un blocco quando gli algoritmi sono lenti (MD2).
pub const UPDATE_BLOCK: usize = 256 * 1024;

/// Intervallo minimo fra due eventi `progress` dello stesso task (al massimo 20 al secondo).
pub const PROGRESS_INTERVAL_MS: f64 = 50.0;

/// Valore di `task` nel comando `cancel` che annulla tutti i task (in corso e in coda).
pub const CANCEL_ALL: i32 = -1;

/// Il più grande intero rappresentabile esattamente in un `number` JavaScript (2⁵³ − 1).
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Identificativo di un task, assegnato dal thread principale e restituito in ogni evento.
pub type TaskId = u64;

/// Interpreta il campo `task` di un comando `hash`: intero non negativo esatto (≤ 2⁵³ − 1).
pub fn parse_task_id(value: f64) -> Option<TaskId> {
    let valid = value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= MAX_SAFE_INTEGER;
    valid.then_some(value as TaskId)
}

/// Destinatario di un comando `cancel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelTarget {
    /// `task = -1`: il task in corso e tutti quelli in coda.
    All,
    Task(TaskId),
}

/// Interpreta il campo `task` di un comando `cancel` (`-1` = tutti).
pub fn parse_cancel_target(value: f64) -> Option<CancelTarget> {
    if value == f64::from(CANCEL_ALL) {
        Some(CancelTarget::All)
    } else {
        parse_task_id(value).map(CancelTarget::Task)
    }
}

/// Dimensione del blocco richiesta (`chunk?`): assente o non valida → [`DEFAULT_CHUNK`], altrimenti
/// arrotondata per difetto e limitata a [`MIN_CHUNK`]..=[`MAX_CHUNK`].
pub fn chunk_size(requested: Option<f64>) -> usize {
    match requested {
        Some(c) if c.is_finite() && c >= 1.0 => c.floor().clamp(MIN_CHUNK as f64, MAX_CHUNK as f64) as usize,
        _ => DEFAULT_CHUNK,
    }
}

/// Converte gli identificativi ricevuti in `algos` (`Algo::id`, con ripiego tollerante su
/// `Algo::from_label`) togliendo i duplicati e mantenendo l'ordine. Errore se un identificativo è
/// sconosciuto o se la lista è vuota.
pub fn parse_algo_ids<I, S>(ids: I) -> Result<Vec<Algo>, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut algos = Vec::new();
    for id in ids {
        let id = id.as_ref();
        let algo = Algo::from_id(id)
            .or_else(|| Algo::from_label(id))
            .ok_or_else(|| format!("unknown algorithm: {id:?}"))?;
        if !algos.contains(&algo) {
            algos.push(algo);
        }
    }
    if algos.is_empty() {
        return Err("no algorithm requested".to_string());
    }
    Ok(algos)
}

/// Digest esadecimali minuscoli indicizzati per `Algo::id`, nell'ordine ricevuto: il contenuto
/// dell'oggetto `digests` dell'evento `done`.
pub fn digest_entries(digests: &[(Algo, Vec<u8>)]) -> Vec<(&'static str, String)> {
    digests.iter().map(|(a, d)| (a.id(), to_hex(d, false))).collect()
}

/// Inverso di [`digest_entries`] per una voce: identificativo e digest esadecimale (maiuscolo o
/// minuscolo) → algoritmo e byte. `None` se l'algoritmo è sconosciuto, l'esadecimale non è valido
/// o la lunghezza non corrisponde all'algoritmo.
pub fn decode_digest(id: &str, hex: &str) -> Option<(Algo, Vec<u8>)> {
    let algo = Algo::from_id(id)?;
    let bytes = hex::decode(hex).ok()?;
    (bytes.len() == algo.digest_len()).then_some((algo, bytes))
}

/// Calcola più algoritmi sugli stessi dati letti una sola volta e conta i byte ricevuti.
pub struct TaskHasher {
    hashers: Vec<(Algo, Box<dyn StreamHasher>)>,
    bytes: u64,
}

impl TaskHasher {
    pub fn new(algos: &[Algo]) -> Self {
        TaskHasher {
            hashers: algos.iter().map(|a| (*a, build(*a))).collect(),
            bytes: 0,
        }
    }

    /// Aggiorna tutti gli algoritmi con il blocco successivo.
    pub fn update(&mut self, data: &[u8]) {
        for (_, h) in &mut self.hashers {
            h.update(data);
        }
        self.bytes += data.len() as u64;
    }

    /// Byte elaborati finora.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Digest finali, nell'ordine degli algoritmi passati a [`TaskHasher::new`].
    pub fn finalize(self) -> Vec<(Algo, Vec<u8>)> {
        self.hashers.into_iter().map(|(a, h)| (a, h.finalize())).collect()
    }
}

/// Limita la frequenza degli eventi `progress`: al più uno ogni [`PROGRESS_INTERVAL_MS`].
#[derive(Debug, Clone)]
pub struct ProgressThrottle {
    interval_ms: f64,
    last_ms: f64,
}

impl ProgressThrottle {
    /// `start_ms` è l'istante d'inizio del task (`performance.now()`): il primo evento arriva
    /// dopo almeno un intervallo.
    pub fn new(start_ms: f64) -> Self {
        Self::with_interval(start_ms, PROGRESS_INTERVAL_MS)
    }

    pub fn with_interval(start_ms: f64, interval_ms: f64) -> Self {
        ProgressThrottle {
            interval_ms,
            last_ms: start_ms,
        }
    }

    /// `true` se all'istante `now_ms` si può emettere un evento (e lo registra come emesso).
    pub fn ready(&mut self, now_ms: f64) -> bool {
        if now_ms - self.last_ms >= self.interval_ms {
            self.last_ms = now_ms;
            true
        } else {
            false
        }
    }
}

/// Coda dei task di un worker: al più un task in esecuzione, gli altri in attesa in ordine FIFO.
///
/// L'annullamento di un task in coda lo rimuove subito (il chiamante emette `cancelled`);
/// l'annullamento del task in esecuzione alza un flag che il ciclo di lettura controlla fra un
/// blocco e l'altro. I comandi per task sconosciuti o già conclusi vengono ignorati.
#[derive(Debug)]
pub struct TaskQueue<J> {
    waiting: VecDeque<(TaskId, J)>,
    running: Option<TaskId>,
    cancel_running: bool,
}

impl<J> Default for TaskQueue<J> {
    fn default() -> Self {
        TaskQueue {
            waiting: VecDeque::new(),
            running: None,
            cancel_running: false,
        }
    }
}

impl<J> TaskQueue<J> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Accoda un task.
    pub fn push(&mut self, task: TaskId, job: J) {
        self.waiting.push_back((task, job));
    }

    /// Se nessun task è in esecuzione, estrae il primo in coda e lo segna come in esecuzione.
    pub fn start_next(&mut self) -> Option<(TaskId, J)> {
        if self.running.is_some() {
            return None;
        }
        let (task, job) = self.waiting.pop_front()?;
        self.running = Some(task);
        self.cancel_running = false;
        Some((task, job))
    }

    /// Il task in esecuzione è terminato (concluso, fallito o annullato).
    pub fn finish(&mut self) {
        self.running = None;
        self.cancel_running = false;
    }

    /// Task in esecuzione, se c'è.
    pub fn running(&self) -> Option<TaskId> {
        self.running
    }

    /// `true` se è stato chiesto di annullare il task in esecuzione.
    pub fn is_running_cancelled(&self) -> bool {
        self.running.is_some() && self.cancel_running
    }

    /// Numero di task in attesa.
    pub fn waiting_len(&self) -> usize {
        self.waiting.len()
    }

    /// Applica un comando `cancel`: segna il task in esecuzione (se coinvolto) e restituisce i task
    /// tolti dalla coda, nell'ordine in cui erano accodati.
    pub fn cancel(&mut self, target: CancelTarget) -> Vec<TaskId> {
        match target {
            CancelTarget::All => {
                if self.running.is_some() {
                    self.cancel_running = true;
                }
                self.waiting.drain(..).map(|(task, _)| task).collect()
            }
            CancelTarget::Task(task) => {
                if self.running == Some(task) {
                    self.cancel_running = true;
                }
                let mut removed = Vec::new();
                self.waiting.retain(|(t, _)| {
                    let hit = *t == task;
                    if hit {
                        removed.push(task);
                    }
                    !hit
                });
                removed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_all_vs_single() {
        let mut q = TaskQueue::new();
        for t in 1..=4u64 {
            q.push(t, ());
        }
        assert_eq!(q.start_next().map(|(t, _)| t), Some(1));
        assert_eq!(q.cancel(CancelTarget::Task(3)), vec![3]);
        assert!(!q.is_running_cancelled());
        assert_eq!(q.cancel(CancelTarget::All), vec![2, 4]);
        assert!(q.is_running_cancelled());
        q.finish();
        assert!(q.start_next().is_none());
    }
}
