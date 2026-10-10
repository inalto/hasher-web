//! Impostazioni utente salvate nel browser (`localStorage["hasher.settings"]`).
//!
//! Stessi tipi e stesso JSON del desktop (`hasher.settings.json`): un file esportato dall'app
//! desktop e incollato nella chiave resta valido e viceversa. Il campo `window` esiste ancora per
//! compatibilità ma sul web viene ignorato. La lettura è tollerante come sul desktop: chiave
//! assente → default, JSON non valido → default (con avviso nel log), campi mancanti → default,
//! algoritmi sconosciuti ignorati.

use crate::web::storage;
use hasher_core::algo::Algo;
use hasher_core::export::ExportFormat;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Chiave di `localStorage` che contiene il JSON delle impostazioni.
pub const STORAGE_KEY: &str = "hasher.settings";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Auto,
    It,
    En,
    Fr,
    De,
    Es,
}

impl Language {
    pub const ALL: [Language; 6] = [
        Language::Auto,
        Language::It,
        Language::En,
        Language::Fr,
        Language::De,
        Language::Es,
    ];

    /// Codice locale (`None` per Auto).
    pub fn code(self) -> Option<&'static str> {
        match self {
            Language::Auto => None,
            Language::It => Some("it"),
            Language::En => Some("en"),
            Language::Fr => Some("fr"),
            Language::De => Some("de"),
            Language::Es => Some("es"),
        }
    }

    /// Nome della lingua nella lingua stessa (mostrato nel selettore).
    pub fn native_name(self) -> &'static str {
        match self {
            Language::Auto => "Auto",
            Language::It => "Italiano",
            Language::En => "English",
            Language::Fr => "Français",
            Language::De => "Deutsch",
            Language::Es => "Español",
        }
    }

    /// Codice effettivo da usare: per `Auto` rileva la lingua del browser (`sys-locale`, cioè
    /// `navigator.languages`) e ricade su `en` se non supportata.
    pub fn resolve(self) -> &'static str {
        if let Some(c) = self.code() {
            return c;
        }
        resolve_code(sys_locale::get_locale())
    }
}

/// Riduce una locale (`it-IT`, `en_US`, `de_DE.UTF-8`, `pt-BR`, …) a uno dei codici supportati;
/// qualsiasi altro valore (o assenza di locale) ricade su `en`.
fn resolve_code(locale: Option<String>) -> &'static str {
    let Some(locale) = locale else {
        return "en";
    };
    let lang = locale
        .trim()
        .split(['-', '_', '.', '@'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match lang.as_str() {
        "it" => "it",
        "en" => "en",
        "fr" => "fr",
        "de" => "de",
        "es" => "es",
        _ => "en",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

/// Posizione e dimensione della finestra del desktop: conservata nel JSON, ignorata sul web.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub language: Language,
    pub theme: ThemeMode,
    /// Algoritmi attivi di default per i nuovi calcoli.
    /// Lettura tollerante: nomi di algoritmi sconosciuti (es. JSON scritto da una versione più
    /// recente) vengono ignorati invece di invalidare l'intero contenuto.
    #[serde(deserialize_with = "lenient_algos")]
    pub default_algos: Vec<Algo>,
    pub uppercase_hex: bool,
    pub recurse_folders: bool,
    /// Sul web i link simbolici non esistono: il campo resta solo per compatibilità del JSON.
    pub follow_symlinks: bool,
    pub include_hidden: bool,
    /// Numero di Web Worker; 0 = automatico (`navigator.hardwareConcurrency`, al massimo 8).
    pub parallel_files: usize,
    pub last_export_format: Option<ExportFormat>,
    /// Non usato sul web (i download finiscono nella cartella scelta dal browser).
    pub last_export_dir: Option<PathBuf>,
    /// Mostra le animazioni (disattivabile per accessibilità / prestazioni).
    pub animations: bool,
    /// Non usato sul web.
    pub window: Option<WindowState>,
}

/// Deserializza `Vec<Algo>` scartando le voci non riconosciute.
fn lenient_algos<'de, D>(d: D) -> Result<Vec<Algo>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Vec<String> = Vec::deserialize(d)?;
    Ok(raw
        .into_iter()
        .filter_map(
            |name| match serde_json::from_value::<Algo>(serde_json::Value::String(name.clone())) {
                Ok(a) => Some(a),
                Err(_) => {
                    log::warn!("algoritmo sconosciuto nelle impostazioni ignorato: {name}");
                    None
                }
            },
        )
        .collect())
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::Auto,
            theme: ThemeMode::System,
            default_algos: Algo::DEFAULT.to_vec(),
            uppercase_hex: false,
            recurse_folders: true,
            follow_symlinks: false,
            include_hidden: false,
            parallel_files: 0,
            last_export_format: None,
            last_export_dir: None,
            animations: true,
            window: None,
        }
    }
}

impl Settings {
    /// Carica le impostazioni da `localStorage`; in caso di assenza o errore restituisce i valori
    /// predefiniti (registrando l'errore con `log::warn!`).
    pub fn load() -> Settings {
        match storage::get(STORAGE_KEY) {
            Some(text) => Self::from_json(&text),
            None => Settings::default(),
        }
    }

    /// Salva in formato JSON leggibile (pretty), come il file del desktop.
    pub fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        storage::set(STORAGE_KEY, &json)
    }

    /// Interpreta il JSON; mai in errore: non valido → avviso nel log e default.
    pub fn from_json(text: &str) -> Settings {
        // Un JSON copiato da un file salvato con Blocco note può iniziare con il BOM UTF-8.
        let text = text.trim_start_matches('\u{feff}');
        if text.trim().is_empty() {
            return Settings::default();
        }
        match serde_json::from_str(text) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("impostazioni non valide in localStorage[{STORAGE_KEY}]: {e}");
                Settings::default()
            }
        }
    }
}
