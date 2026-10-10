//! Accesso a `localStorage` (assente o bloccato in alcune modalità private: mai in errore fatale).

use super::{js_error_text, window};

fn local_storage() -> Option<web_sys::Storage> {
    window()?.local_storage().ok().flatten()
}

/// Valore della chiave, se presente e leggibile.
pub fn get(key: &str) -> Option<String> {
    local_storage()?.get_item(key).ok().flatten()
}

/// Scrive la chiave; l'errore (quota, storage disabilitato) è restituito come testo.
pub fn set(key: &str, value: &str) -> Result<(), String> {
    let storage = local_storage().ok_or_else(|| "localStorage non disponibile".to_owned())?;
    storage.set_item(key, value).map_err(|e| js_error_text(&e))
}
