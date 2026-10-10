//! Risultato del calcolo per un file. Contratto condiviso fra motore, interfaccia ed export.

use crate::algo::{to_hex, Algo};
use crate::fileinfo::FileInfo;
use std::collections::BTreeMap;
use std::time::Duration;

/// Identificatore progressivo di un file all'interno di una sessione del motore.
pub type FileId = u64;

/// Risultato completo per un file.
#[derive(Debug, Clone)]
pub struct FileResult {
    pub id: FileId,
    pub info: FileInfo,
    /// Digest grezzi per algoritmo (ordinati per `Algo`).
    pub digests: BTreeMap<Algo, Vec<u8>>,
    pub elapsed: Duration,
}

impl FileResult {
    /// Digest esadecimale per l'algoritmo richiesto, se calcolato.
    pub fn hex(&self, algo: Algo, uppercase: bool) -> Option<String> {
        self.digests.get(&algo).map(|d| to_hex(d, uppercase))
    }

    /// Algoritmi il cui digest coincide con l'hash (normalizzato, minuscolo) fornito.
    pub fn matching_algos(&self, normalized_hex: &str) -> Vec<Algo> {
        self.digests
            .iter()
            .filter(|(_, d)| hex::encode(d) == normalized_hex)
            .map(|(a, _)| *a)
            .collect()
    }
}
