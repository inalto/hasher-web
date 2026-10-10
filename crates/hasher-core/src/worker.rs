//! Entry point del Web Worker che calcola gli hash (feature `worker`, solo wasm32).
//!
//! Protocollo (docs/PIANO.md §5.3), messaggi come oggetti JS via `postMessage`:
//!
//! In ingresso (dal thread principale):
//! - `{ cmd: "hash", task: number, file: File, algos: string[], chunk?: number }`
//!   calcola gli algoritmi indicati (`Algo::id`) leggendo `file` a blocchi di `chunk` byte
//!   (default 4 MiB) con `File.slice(..).arrayBuffer()`; il file non è mai in memoria per intero;
//! - `{ cmd: "cancel", task: number }` annulla il task (controllato fra un blocco e l'altro).
//!
//! In uscita (verso il thread principale):
//! - `{ ev: "ready" }` appena il worker è inizializzato;
//! - `{ ev: "started", task }`;
//! - `{ ev: "progress", task, bytes }` al massimo ~20 volte al secondo;
//! - `{ ev: "done", task, bytes, elapsed_ms, digests: { "<algo id>": "<hex minuscolo>", … } }`;
//! - `{ ev: "failed", task, error: string }`;
//! - `{ ev: "cancelled", task }`.
//!
//! IMPLEMENTAZIONE A CARICO DELL'AGENTE "core" (fase 1A).

use wasm_bindgen::prelude::*;

/// Avvia il ciclo di ricezione dei messaggi nel `DedicatedWorkerGlobalScope` corrente.
#[wasm_bindgen]
pub fn worker_main() {
    console_error_panic_hook::set_once();
    todo!("fase 1A: vedi docs/PIANO.md §5.3")
}
