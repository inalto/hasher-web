//! Hasher core — Martini Multimedia s.a.s.
//!
//! Logica indipendente dall'interfaccia e dal sistema operativo, condivisa fra l'app web (egui in
//! WebAssembly) e il Web Worker che calcola gli hash:
//!
//! - [`algo`]: catalogo degli algoritmi, esadecimale, normalizzazione degli hash incollati;
//! - [`hasher`]: trait `StreamHasher` e implementazioni (incluso ED2K);
//! - [`checksum_file`]: parser dei file `.md5`, `.sha256`, `.sfv`… (formati GNU, BSD, SFV);
//! - [`fileinfo`] e [`result`]: metadati di un file e risultato del calcolo;
//! - [`export`]: rapporti TXT, CSV, JSON, XLSX, DOCX e file di checksum, prodotti in memoria;
//! - [`worker_protocol`]: parti pure del protocollo del Web Worker (comandi, eventi, coda dei task,
//!   cadenza dell'avanzamento, digest), verificabili anche in nativo;
//! - `worker` (feature `worker`, solo wasm32): entry point del Web Worker.
//!
//! Il crate non usa mai il file system: compila e gira identico su `x86_64` (test) e su
//! `wasm32-unknown-unknown`.

pub mod algo;
pub mod checksum_file;
pub mod export;
pub mod fileinfo;
pub mod hasher;
pub mod result;
#[cfg(all(feature = "worker", target_arch = "wasm32"))]
pub mod worker;
pub mod worker_protocol;

pub const APP_NAME: &str = "Hasher";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COMPANY: &str = "Martini Multimedia s.a.s.";
pub const COMPANY_URL: &str = "https://www.martini-multimedia.net";
