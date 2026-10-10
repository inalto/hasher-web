//! Sorgente dei file statici: astrazione `Assets` + implementazione su `rust-embed`.
//!
//! Il server non conosce `rust-embed` direttamente: lavora con il trait [`Assets`]. In produzione
//! l'implementazione è [`EmbedAssets<Dist>`] (cartella `dist/` embeddata nel binario); i test
//! usano la stessa implementazione puntata a `crates/hasher-web-server/testdata/`, così
//! il percorso di codice provato è quello reale (stesso hash SHA-256, stessa ricerca per nome).

use std::borrow::Cow;
use std::marker::PhantomData;

use rust_embed::Embed;

/// Un file statico: contenuto e hash SHA-256 (da cui si ricava l'`ETag`).
pub struct Asset {
    /// Contenuto. In release è `Cow::Borrowed` sui byte embeddati nel binario (nessuna copia).
    pub data: Cow<'static, [u8]>,
    /// SHA-256 del contenuto, calcolato da `rust-embed`.
    pub sha256: [u8; 32],
}

/// Insieme di file indicizzati per percorso relativo con `/` come separatore (`index.html`, `icons/icon.svg`).
pub trait Assets: Send + Sync + 'static {
    /// Restituisce il file, se esiste.
    fn get(&self, path: &str) -> Option<Asset>;
}

/// Adattatore da un tipo con `#[derive(Embed)]` al trait [`Assets`].
pub struct EmbedAssets<E>(PhantomData<fn() -> E>);

impl<E> EmbedAssets<E> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<E> Default for EmbedAssets<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Embed + 'static> Assets for EmbedAssets<E> {
    fn get(&self, path: &str) -> Option<Asset> {
        let file = E::get(path)?;
        Some(Asset {
            sha256: file.metadata.sha256_hash(),
            data: file.data,
        })
    }
}

/// La cartella `dist/` prodotta da `scripts/build-web.sh`.
///
/// In release i file sono incorporati nel binario; in debug vengono letti dal disco a ogni
/// richiesta (ciclo di sviluppo: `scripts/dev.sh`). `.gitkeep` non fa parte del sito.
#[derive(Embed)]
#[folder = "../../dist"]
#[exclude = ".gitkeep"]
pub struct Dist;
