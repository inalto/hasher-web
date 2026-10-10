//! Impostazioni utente con persistenza "portable" (accanto all'eseguibile) e fallback nella
//! cartella di configurazione dell'utente.
//!
//! Ordine di risoluzione del percorso (`Settings::path`):
//! 1. variabile d'ambiente `HASHER_SETTINGS_PATH` (percorso assoluto; utile per test e screenshot);
//! 2. `hasher.settings.json` accanto all'eseguibile, se esiste già o se la cartella è scrivibile
//!    (su macOS, dentro un bundle `.app`, la cartella che contiene il bundle);
//! 3. `<config_dir>/MartiniMultimedia/Hasher/hasher.settings.json`;
//! 4. cartella corrente, se non esiste una cartella di configurazione.

use super::algo::Algo;
use super::export::ExportFormat;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const SETTINGS_FILE_NAME: &str = "hasher.settings.json";

/// Variabile d'ambiente che forza il percorso del file impostazioni.
pub const SETTINGS_PATH_ENV: &str = "HASHER_SETTINGS_PATH";

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

    /// Codice effettivo da usare: per `Auto` rileva la lingua di sistema (`sys-locale`) e
    /// ricade su `en` se non supportata.
    pub fn resolve(self) -> &'static str {
        if let Some(c) = self.code() {
            return c;
        }
        resolve_code(sys_locale::get_locale())
    }
}

/// Riduce una locale di sistema (`it-IT`, `en_US`, `de_DE.UTF-8`, `pt-BR`, …) a uno dei codici
/// supportati; qualsiasi altro valore (o assenza di locale) ricade su `en`.
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
    /// Lettura tollerante: nomi di algoritmi sconosciuti (es. file scritto da una versione più
    /// recente) vengono ignorati invece di invalidare l'intero file.
    #[serde(deserialize_with = "lenient_algos")]
    pub default_algos: Vec<Algo>,
    pub uppercase_hex: bool,
    pub recurse_folders: bool,
    pub follow_symlinks: bool,
    pub include_hidden: bool,
    /// 0 = automatico.
    pub parallel_files: usize,
    pub last_export_format: Option<ExportFormat>,
    pub last_export_dir: Option<PathBuf>,
    /// Mostra le animazioni (disattivabile per accessibilità / prestazioni).
    pub animations: bool,
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
    /// Carica le impostazioni dal percorso restituito da `path()`; in caso di assenza o errore
    /// restituisce i valori predefiniti (registrando l'errore con `log::warn!`).
    pub fn load() -> Settings {
        Self::load_from(&Self::path())
    }

    /// Salva in formato JSON leggibile (pretty) nel percorso restituito da `path()`.
    pub fn save(&self) -> io::Result<()> {
        self.save_to(&Self::path())
    }

    /// Percorso del file impostazioni: accanto all'eseguibile se la cartella è scrivibile
    /// (modalità portable), altrimenti `<config_dir>/MartiniMultimedia/Hasher/hasher.settings.json`.
    ///
    /// La variabile d'ambiente `HASHER_SETTINGS_PATH` ha la precedenza (letta a ogni chiamata);
    /// il percorso calcolato altrimenti è memorizzato per la durata del processo.
    pub fn path() -> PathBuf {
        if let Some(p) = env_override() {
            return p;
        }
        static CACHE: OnceLock<PathBuf> = OnceLock::new();
        CACHE.get_or_init(resolve_path).clone()
    }

    /// `true` se le impostazioni vivono accanto all'eseguibile.
    pub fn is_portable() -> bool {
        is_portable_path(&Self::path(), portable_dir().as_deref())
    }

    /// Legge e interpreta il file; mai in errore: assente → default, illeggibile o non valido →
    /// avviso nel log e default.
    fn load_from(path: &Path) -> Settings {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Settings::default(),
            Err(e) => {
                log::warn!("impossibile leggere le impostazioni {}: {e}", path.display());
                return Settings::default();
            }
        };
        // Alcuni editor di Windows aggiungono il BOM UTF-8, che serde_json non accetta.
        let text = text.trim_start_matches('\u{feff}');
        match serde_json::from_str(text) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("impostazioni non valide in {}: {e}", path.display());
                Settings::default()
            }
        }
    }

    /// Scrittura atomica: file temporaneo `<path>.tmp` nella stessa cartella, poi `rename`.
    fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let mut json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        json.push('\n');

        let mut tmp_name = path.as_os_str().to_owned();
        tmp_name.push(".tmp");
        let tmp = PathBuf::from(tmp_name);

        let written = (|| {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(json.as_bytes())?;
            f.sync_all()?;
            drop(f);
            fs::rename(&tmp, path)
        })();
        if written.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        written
    }
}

/// Percorso forzato da `HASHER_SETTINGS_PATH` (ignorato se vuoto).
fn env_override() -> Option<PathBuf> {
    std::env::var_os(SETTINGS_PATH_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Cartella "portable": quella dell'eseguibile (seguendo i link simbolici, se possibile) oppure,
/// su macOS dentro un bundle `.app`, la cartella che contiene il bundle. Calcolata una volta.
fn portable_dir() -> Option<PathBuf> {
    static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let exe = std::env::current_exe().ok()?;
            let exe = exe.canonicalize().map(strip_verbatim_prefix).unwrap_or(exe);
            if cfg!(target_os = "macos") {
                if let Some(dir) = app_bundle_parent(&exe) {
                    return Some(dir);
                }
            }
            exe.parent().map(Path::to_path_buf)
        })
        .clone()
}

/// Per un eseguibile `…/Nome.app/Contents/MacOS/nome` restituisce la cartella che contiene
/// `Nome.app`; `None` se il percorso non è dentro un bundle.
fn app_bundle_parent(exe: &Path) -> Option<PathBuf> {
    exe.ancestors().find_map(|macos| {
        if macos.file_name()? != "MacOS" {
            return None;
        }
        let contents = macos.parent()?;
        if contents.file_name()? != "Contents" {
            return None;
        }
        let app = contents.parent()?;
        if app.extension()? != "app" {
            return None;
        }
        app.parent().map(Path::to_path_buf)
    })
}

/// `canonicalize` su Windows restituisce percorsi `\\?\C:\…`; li riporta alla forma normale
/// (esclusi i percorsi UNC `\\?\UNC\…`, che restano invariati).
fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    if let Some(rest) = path.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        let b = rest.as_bytes();
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            return PathBuf::from(rest);
        }
    }
    path
}

/// Verifica che si possa creare un file in `dir` provando a crearne (e rimuoverne) uno.
/// `permissions().readonly()` non è affidabile (ACL, volumi di sola lettura, …).
fn dir_is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".hasher-write-test-{}", std::process::id()));
    let created = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&probe)
        .is_ok();
    if created {
        let _ = fs::remove_file(&probe);
    }
    created
}

/// Percorso scelto per l'eseguibile in `exe_dir`: il file accanto all'eseguibile se esiste già o
/// se la cartella è scrivibile, altrimenti quello nella cartella di configurazione utente.
fn compute_path(exe_dir: &Path) -> PathBuf {
    compute_path_with(exe_dir, dirs::config_dir())
}

fn compute_path_with(exe_dir: &Path, config_dir: Option<PathBuf>) -> PathBuf {
    let portable = exe_dir.join(SETTINGS_FILE_NAME);
    if portable.exists() || dir_is_writable(exe_dir) {
        portable
    } else {
        fallback_path(config_dir)
    }
}

/// `<config_dir>/MartiniMultimedia/Hasher/hasher.settings.json`; senza cartella di
/// configurazione, il file nella cartella corrente.
fn fallback_path(config_dir: Option<PathBuf>) -> PathBuf {
    match config_dir {
        Some(dir) => dir.join("MartiniMultimedia").join("Hasher").join(SETTINGS_FILE_NAME),
        None => std::env::current_dir().unwrap_or_default().join(SETTINGS_FILE_NAME),
    }
}

fn resolve_path() -> PathBuf {
    match portable_dir() {
        Some(dir) => compute_path(&dir),
        None => fallback_path(dirs::config_dir()),
    }
}

/// `true` se la cartella che contiene `path` coincide con `exe_dir`.
fn is_portable_path(path: &Path, exe_dir: Option<&Path>) -> bool {
    let (Some(parent), Some(exe_dir)) = (path.parent(), exe_dir) else {
        return false;
    };
    if parent == exe_dir {
        return true;
    }
    match (parent.canonicalize(), exe_dir.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::export::ExportFormat;

    fn sample_settings() -> Settings {
        Settings {
            language: Language::Fr,
            theme: ThemeMode::Dark,
            default_algos: vec![Algo::DEFAULT[0], Algo::DEFAULT[1]],
            uppercase_hex: true,
            recurse_folders: false,
            follow_symlinks: true,
            include_hidden: true,
            parallel_files: 3,
            last_export_format: Some(ExportFormat::Checksum(Algo::DEFAULT[2])),
            last_export_dir: Some(PathBuf::from("/tmp/exports")),
            animations: false,
            window: Some(WindowState {
                x: 10.5,
                y: 20.0,
                width: 900.0,
                height: 600.25,
                maximized: true,
            }),
        }
    }

    // --- Language::resolve ------------------------------------------------------------------

    #[test]
    fn resolve_code_maps_supported_locales() {
        let cases = [
            ("it-IT", "it"),
            ("it_IT", "it"),
            ("it", "it"),
            ("en_US", "en"),
            ("en-GB", "en"),
            ("fr-FR", "fr"),
            ("fr_CA.UTF-8", "fr"),
            ("de_DE.UTF-8", "de"),
            ("de-AT", "de"),
            ("es-419", "es"),
            ("es_ES@euro", "es"),
            ("IT-it", "it"),
            ("  FR  ", "fr"),
        ];
        for (input, expected) in cases {
            assert_eq!(resolve_code(Some(input.to_string())), expected, "{input}");
        }
    }

    #[test]
    fn resolve_code_falls_back_to_english() {
        for input in ["pt-BR", "zh-Hans-CN", "ja_JP", "C", "POSIX", "", "-", "ita", "xx"] {
            assert_eq!(resolve_code(Some(input.to_string())), "en", "{input}");
        }
        assert_eq!(resolve_code(None), "en");
    }

    #[test]
    fn explicit_language_resolves_to_its_code() {
        assert_eq!(Language::It.resolve(), "it");
        assert_eq!(Language::En.resolve(), "en");
        assert_eq!(Language::Fr.resolve(), "fr");
        assert_eq!(Language::De.resolve(), "de");
        assert_eq!(Language::Es.resolve(), "es");
        // `Auto` dipende dal sistema ma dà sempre un codice supportato.
        let auto = Language::Auto.resolve();
        assert!(["it", "en", "fr", "de", "es"].contains(&auto), "{auto}");
    }

    // --- load / save ------------------------------------------------------------------------

    #[test]
    fn load_missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let s = Settings::load_from(&dir.path().join(SETTINGS_FILE_NAME));
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn load_invalid_json_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(SETTINGS_FILE_NAME);
        for garbage in ["", "not json", "{\"theme\": ", "[1,2,3]", "{\"theme\":\"purple\"}"] {
            fs::write(&p, garbage).unwrap();
            assert_eq!(Settings::load_from(&p), Settings::default(), "{garbage:?}");
        }
    }

    #[test]
    fn load_unreadable_path_gives_defaults() {
        // Una cartella al posto del file: la lettura fallisce con un errore diverso da NotFound.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load_from(dir.path()), Settings::default());
    }

    #[test]
    fn load_partial_json_keeps_other_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(SETTINGS_FILE_NAME);
        fs::write(&p, r#"{"theme":"dark"}"#).unwrap();
        let s = Settings::load_from(&p);
        assert_eq!(s.theme, ThemeMode::Dark);
        assert_eq!(
            s,
            Settings {
                theme: ThemeMode::Dark,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn load_ignores_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(SETTINGS_FILE_NAME);
        fs::write(
            &p,
            r#"{"language":"de","future_option":{"a":1},"another":[1,2],"uppercase_hex":true}"#,
        )
        .unwrap();
        let s = Settings::load_from(&p);
        assert_eq!(s.language, Language::De);
        assert!(s.uppercase_hex);
        assert_eq!(
            s,
            Settings {
                language: Language::De,
                uppercase_hex: true,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn load_accepts_utf8_bom() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(SETTINGS_FILE_NAME);
        fs::write(&p, "\u{feff}{\"language\":\"es\"}").unwrap();
        assert_eq!(Settings::load_from(&p).language, Language::Es);
    }

    #[test]
    fn save_then_load_roundtrip_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a").join("b").join(SETTINGS_FILE_NAME);
        let s = sample_settings();
        s.save_to(&p).unwrap();
        assert!(p.is_file());
        assert_eq!(Settings::load_from(&p), s);

        // Il file è JSON "pretty" e il temporaneo non resta.
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("\n  \"language\": \"fr\""), "{text}");
        let leftovers: Vec<_> = fs::read_dir(p.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from(SETTINGS_FILE_NAME)]);
    }

    #[test]
    fn save_overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(SETTINGS_FILE_NAME);
        Settings::default().save_to(&p).unwrap();
        let s = sample_settings();
        s.save_to(&p).unwrap();
        assert_eq!(Settings::load_from(&p), s);
        let defaults = Settings::default();
        defaults.save_to(&p).unwrap();
        assert_eq!(Settings::load_from(&p), defaults);
    }

    #[test]
    fn save_failure_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        // Il bersaglio è una cartella non vuota: il `rename` fallisce.
        let p = dir.path().join(SETTINGS_FILE_NAME);
        fs::create_dir_all(p.join("occupied")).unwrap();
        assert!(Settings::default().save_to(&p).is_err());
        assert!(!dir.path().join(format!("{SETTINGS_FILE_NAME}.tmp")).exists());
    }

    #[test]
    fn serialized_json_uses_expected_keys() {
        let v = serde_json::to_value(sample_settings()).unwrap();
        assert_eq!(v["language"], "fr");
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["parallel_files"], 3);
        assert_eq!(v["default_algos"].as_array().unwrap().len(), 2);
    }

    // --- percorso ---------------------------------------------------------------------------

    #[test]
    fn compute_path_prefers_writable_exe_dir_and_leaves_no_probe() {
        let exe_dir = tempfile::tempdir().unwrap();
        let cfg = tempfile::tempdir().unwrap();
        let p = compute_path_with(exe_dir.path(), Some(cfg.path().to_path_buf()));
        assert_eq!(p, exe_dir.path().join(SETTINGS_FILE_NAME));
        assert_eq!(
            fs::read_dir(exe_dir.path()).unwrap().count(),
            0,
            "probe file left behind"
        );
        assert!(dir_is_writable(exe_dir.path()));
    }

    #[test]
    fn compute_path_uses_existing_file_in_exe_dir() {
        let exe_dir = tempfile::tempdir().unwrap();
        let existing = exe_dir.path().join(SETTINGS_FILE_NAME);
        fs::write(&existing, "{}").unwrap();
        let cfg = tempfile::tempdir().unwrap();
        assert_eq!(
            compute_path_with(exe_dir.path(), Some(cfg.path().to_path_buf())),
            existing
        );
    }

    #[test]
    fn compute_path_falls_back_to_config_dir_when_not_writable() {
        // Cartella inesistente: non scrivibile su qualsiasi piattaforma, anche da amministratore.
        let base = tempfile::tempdir().unwrap();
        let missing = base.path().join("does-not-exist");
        let cfg = tempfile::tempdir().unwrap();
        let expected = cfg
            .path()
            .join("MartiniMultimedia")
            .join("Hasher")
            .join(SETTINGS_FILE_NAME);
        assert_eq!(compute_path_with(&missing, Some(cfg.path().to_path_buf())), expected);
    }

    #[cfg(unix)]
    #[test]
    fn compute_path_falls_back_to_config_dir_for_read_only_dir() {
        use std::os::unix::fs::PermissionsExt;
        let ro = tempfile::tempdir().unwrap();
        fs::set_permissions(ro.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let cfg = tempfile::tempdir().unwrap();
        let expected = cfg
            .path()
            .join("MartiniMultimedia")
            .join("Hasher")
            .join(SETTINGS_FILE_NAME);
        // Se i test girano da root la cartella resta scrivibile: in tal caso non c'è nulla da verificare.
        if !dir_is_writable(ro.path()) {
            assert_eq!(compute_path_with(ro.path(), Some(cfg.path().to_path_buf())), expected);
        }
        fs::set_permissions(ro.path(), fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn compute_path_keeps_existing_file_in_read_only_dir() {
        use std::os::unix::fs::PermissionsExt;
        let ro = tempfile::tempdir().unwrap();
        let existing = ro.path().join(SETTINGS_FILE_NAME);
        fs::write(&existing, "{}").unwrap();
        fs::set_permissions(ro.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let cfg = tempfile::tempdir().unwrap();
        assert_eq!(compute_path_with(ro.path(), Some(cfg.path().to_path_buf())), existing);
        fs::set_permissions(ro.path(), fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn fallback_path_without_config_dir_is_current_dir() {
        let expected = std::env::current_dir().unwrap().join(SETTINGS_FILE_NAME);
        assert_eq!(fallback_path(None), expected);
        let missing = tempfile::tempdir().unwrap().path().join("nope");
        assert_eq!(compute_path_with(&missing, None), expected);
    }

    #[test]
    fn compute_path_real_config_dir_shape() {
        let missing = tempfile::tempdir().unwrap().path().join("nope");
        let p = compute_path(&missing);
        assert_eq!(p.file_name().unwrap(), SETTINGS_FILE_NAME);
        if dirs::config_dir().is_some() {
            assert!(p.ends_with(Path::new("MartiniMultimedia").join("Hasher").join(SETTINGS_FILE_NAME)));
        }
    }

    #[test]
    fn app_bundle_parent_detects_macos_bundles() {
        assert_eq!(
            app_bundle_parent(Path::new("/Applications/Tools/Hasher.app/Contents/MacOS/hasher")),
            Some(PathBuf::from("/Applications/Tools"))
        );
        assert_eq!(
            app_bundle_parent(Path::new("/Users/me/Hasher.app/Contents/MacOS/hasher")),
            Some(PathBuf::from("/Users/me"))
        );
        assert_eq!(app_bundle_parent(Path::new("/usr/local/bin/hasher")), None);
        assert_eq!(app_bundle_parent(Path::new("/opt/Hasher/Contents/MacOS/hasher")), None);
        assert_eq!(
            app_bundle_parent(Path::new("/opt/x.app/Contents/Resources/hasher")),
            None
        );
    }

    #[test]
    fn strip_verbatim_prefix_only_touches_drive_paths() {
        assert_eq!(
            strip_verbatim_prefix(PathBuf::from(r"\\?\C:\Tools\hasher.exe")),
            PathBuf::from(r"C:\Tools\hasher.exe")
        );
        let unc = PathBuf::from(r"\\?\UNC\server\share\hasher.exe");
        assert_eq!(strip_verbatim_prefix(unc.clone()), unc);
        let plain = PathBuf::from("/usr/bin/hasher");
        assert_eq!(strip_verbatim_prefix(plain.clone()), plain);
    }

    #[test]
    fn is_portable_path_compares_parent_directory() {
        let exe_dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let in_exe = exe_dir.path().join(SETTINGS_FILE_NAME);
        let elsewhere = other.path().join(SETTINGS_FILE_NAME);
        assert!(is_portable_path(&in_exe, Some(exe_dir.path())));
        assert!(!is_portable_path(&elsewhere, Some(exe_dir.path())));
        assert!(!is_portable_path(&in_exe, None));
        // Un sotto-cartella non è la cartella dell'eseguibile.
        assert!(!is_portable_path(
            &exe_dir.path().join("sub").join(SETTINGS_FILE_NAME),
            Some(exe_dir.path())
        ));
    }

    #[cfg(unix)]
    #[test]
    fn is_portable_path_follows_symlinked_directories() {
        let real = tempfile::tempdir().unwrap();
        let holder = tempfile::tempdir().unwrap();
        let link = holder.path().join("link");
        std::os::unix::fs::symlink(real.path(), &link).unwrap();
        assert!(is_portable_path(&link.join(SETTINGS_FILE_NAME), Some(real.path())));
    }

    #[test]
    fn portable_dir_is_the_test_executable_dir() {
        let dir = portable_dir().expect("current_exe available");
        let exe = std::env::current_exe().unwrap().canonicalize().unwrap();
        assert_eq!(dir.canonicalize().unwrap(), exe.parent().unwrap());
    }

    /// Unico test che passa dalle funzioni pubbliche con la variabile d'ambiente: nessun altro
    /// test deve dipendere da `Settings::path()`.
    #[test]
    fn public_api_honours_env_override() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nested").join(SETTINGS_FILE_NAME);
        std::env::set_var(SETTINGS_PATH_ENV, &p);

        assert_eq!(Settings::path(), p);
        assert!(!Settings::is_portable(), "override points outside the executable dir");
        assert_eq!(Settings::load(), Settings::default(), "missing file → defaults");

        let s = sample_settings();
        s.save().unwrap();
        assert!(p.is_file());
        assert_eq!(Settings::load(), s);

        // Un valore vuoto non è un override.
        std::env::set_var(SETTINGS_PATH_ENV, "");
        assert_ne!(Settings::path(), PathBuf::new());
        std::env::remove_var(SETTINGS_PATH_ENV);
    }

    #[test]
    fn unknown_algo_names_are_ignored() {
        let json = r#"{"default_algos":["sha256","quantum-9000","md5"]}"#;
        let st: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(st.default_algos, vec![Algo::Sha256, Algo::Md5]);
    }
}
