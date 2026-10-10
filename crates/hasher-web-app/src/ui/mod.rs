//! Interfaccia grafica egui/eframe: stato dell'applicazione, ciclo di aggiornamento, azioni.
//!
//! Le viste (`topbar`, `single`, `list`, `verify`, …) leggono lo stato di [`HasherApp`] e
//! accodano [`Action`] che vengono eseguite a fine frame, così il codice di disegno resta
//! separato dagli effetti (dialoghi nativi, avvio del motore, salvataggi).

mod anim;
mod dropzone;
mod export;
mod format;
mod help;
mod list;
mod settings;
mod single;
mod statusbar;
mod theme;
mod topbar;
mod verify;
mod widgets;

use crate::backend::algo::{normalize_hex, Algo};
use crate::backend::checksum_file::{self, ChecksumEntry};
use crate::backend::engine::{self, EngineConfig, EngineEvent, EngineHandle, FileId, FileResult};
use crate::backend::settings::{Language, Settings, ThemeMode, WindowState};
use crossbeam_channel::Receiver;
use egui::{Context, Event, Key, Modifiers, Ui, ViewportCommand};
use rust_i18n::t;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::time::Duration;
use widgets::{ToastKind, Verdict};

/// Valore segnaposto dei metadati non ancora letti (mostrato come scheletro animato).
pub(crate) const PENDING_VALUE: &str = "\u{2026}";

/// Argomenti da riga di comando: file da aprire subito e hook per i test grafici headless.
#[derive(Debug, Clone, Default)]
pub struct CliArgs {
    pub files: Vec<PathBuf>,
    /// Se impostato, l'app salva uno screenshot PNG in questo percorso dopo `screenshot_after_frames`
    /// frame (o appena terminato il calcolo dei file passati) e poi esce. Usato dai test con Xvfb.
    pub screenshot: Option<PathBuf>,
    pub screenshot_after_frames: u32,
}

impl CliArgs {
    pub fn parse() -> CliArgs {
        let mut args = CliArgs {
            screenshot_after_frames: 30,
            ..Default::default()
        };
        let mut it = std::env::args_os().skip(1);
        while let Some(a) = it.next() {
            match a.to_str() {
                Some("--screenshot") => args.screenshot = it.next().map(PathBuf::from),
                Some("--screenshot-frames") => {
                    args.screenshot_after_frames = it.next().and_then(|v| v.to_str()?.parse().ok()).unwrap_or(30)
                }
                _ => args.files.push(PathBuf::from(a)),
            }
        }
        args
    }
}

/// Avvia l'applicazione (finestra nativa). Blocca fino alla chiusura.
pub fn run(args: CliArgs) -> eframe::Result {
    let settings = load_settings();
    let screenshot_mode = args.screenshot.is_some();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(crate::APP_NAME)
        .with_app_id("hasher")
        .with_inner_size([1100.0, 760.0])
        .with_min_inner_size([760.0, 520.0])
        .with_drag_and_drop(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icons/icon-256.png")) {
        viewport = viewport.with_icon(icon);
    }
    if !screenshot_mode {
        if let Some(w) = settings.window.filter(window_state_is_sane) {
            viewport = viewport
                .with_inner_size([w.width, w.height])
                .with_maximized(w.maximized);
            viewport = viewport.with_position([w.x, w.y]);
        }
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        crate::APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(HasherApp::new(cc, args, settings)))),
    )
}

fn window_state_is_sane(w: &WindowState) -> bool {
    w.width.is_finite()
        && w.height.is_finite()
        && w.width >= 400.0
        && w.height >= 300.0
        && w.width <= 16_000.0
        && w.height <= 16_000.0
        && (-8_000.0..=16_000.0).contains(&w.x)
        && (-8_000.0..=16_000.0).contains(&w.y)
}

/// Carica le impostazioni proteggendosi da panic (backend non ancora pronto o file corrotto).
fn load_settings() -> Settings {
    match std::panic::catch_unwind(Settings::load) {
        Ok(s) => s,
        Err(_) => {
            log::warn!("Settings::load non disponibile: uso i valori predefiniti");
            Settings::default()
        }
    }
}

/// Esegue una chiamata al backend intercettando eventuali panic (stub `todo!()` durante lo
/// sviluppo, bug imprevisti): l'interfaccia mostra un errore invece di chiudersi.
pub(crate) fn guard<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(AssertUnwindSafe(f)).map_err(|e| {
        e.downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| e.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".to_owned())
    })
}

/// Codice lingua effettivo.
pub(crate) fn resolve_language(lang: Language) -> &'static str {
    // `Language::resolve` interroga il sistema per `Auto`: se fallisse (stub non ancora
    // implementato) si ricade sull'inglese.
    std::panic::catch_unwind(AssertUnwindSafe(|| lang.resolve())).unwrap_or("en")
}

/// Applica la lingua all'intera interfaccia.
pub(crate) fn apply_language(lang: Language) {
    rust_i18n::set_locale(resolve_language(lang));
}

// ---------------------------------------------------------------------------------------------
// Stato
// ---------------------------------------------------------------------------------------------

/// Stato di un file nella sessione.
#[derive(Debug)]
pub(crate) enum EntryState {
    Queued,
    Computing {
        bytes_done: u64,
        started: f64,
    },
    Done(Box<FileResult>),
    Failed(String),
    /// Calcolo annullato prima della fine (si può riprendere).
    Cancelled,
}

/// Un file in elenco.
#[derive(Debug)]
pub(crate) struct FileEntry {
    pub id: FileId,
    pub path: PathBuf,
    pub name: String,
    /// Nome in minuscolo (filtro).
    pub name_lc: String,
    /// Cartella relativa all'input (mostrata sotto il nome nella vista elenco).
    pub dir_text: String,
    pub extension: Option<String>,
    pub size: u64,
    pub size_text: String,
    pub state: EntryState,
    /// Esadecimali già formattati (cache) quando il calcolo è completo.
    pub hexes: Vec<(Algo, String)>,
    pub modified_text: String,
    pub expanded: bool,
    pub selected: bool,
    /// Esito complessivo (verifica incollata o file di checksum).
    pub verdict: Verdict,
    /// Algoritmi che corrispondono all'hash incollato.
    pub match_algos: Vec<Algo>,
    /// Momento di comparsa (per la dissolvenza).
    pub appeared: f64,
    /// Altezza misurata del dettaglio espanso.
    pub detail_h: f32,
}

impl FileEntry {
    fn new(id: FileId, path: PathBuf, size: u64, now: f64) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let extension = path.extension().map(|e| e.to_string_lossy().into_owned());
        Self {
            id,
            dir_text: String::new(),
            name_lc: name.to_lowercase(),
            name,
            extension,
            path,
            size,
            size_text: format::size(size),
            state: EntryState::Queued,
            hexes: Vec::new(),
            modified_text: String::new(),
            expanded: false,
            selected: false,
            verdict: Verdict::None,
            match_algos: Vec::new(),
            appeared: now,
            detail_h: 0.0,
        }
    }

    pub fn hex(&self, algo: Algo) -> Option<&str> {
        self.hexes.iter().find(|(a, _)| *a == algo).map(|(_, h)| h.as_str())
    }

    pub fn result(&self) -> Option<&FileResult> {
        match &self.state {
            EntryState::Done(r) => Some(r),
            _ => None,
        }
    }

    pub fn is_done(&self) -> bool {
        matches!(self.state, EntryState::Done(_))
    }

    /// Frazione completata del singolo file.
    pub fn fraction(&self) -> f32 {
        match &self.state {
            EntryState::Queued => 0.0,
            EntryState::Computing { bytes_done, .. } => {
                if self.size == 0 {
                    0.0
                } else {
                    (*bytes_done as f64 / self.size as f64).min(1.0) as f32
                }
            }
            EntryState::Done(_) | EntryState::Failed(_) => 1.0,
            EntryState::Cancelled => 0.0,
        }
    }

    fn rebuild_hexes(&mut self, uppercase: bool) {
        if let EntryState::Done(r) = &self.state {
            self.hexes = r
                .digests
                .keys()
                .filter_map(|a| r.hex(*a, uppercase).map(|h| (*a, h)))
                .collect();
            self.modified_text = format::datetime(r.info.modified);
        }
    }
}

/// Testo incollato nel campo di verifica, interpretato.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum HashInput {
    #[default]
    Empty,
    NotHex,
    BadLength(usize),
    Valid {
        hex: String,
        candidates: Vec<Algo>,
    },
}

impl HashInput {
    fn parse(text: &str) -> HashInput {
        if text.trim().is_empty() {
            return HashInput::Empty;
        }
        // 1) L'intero testo è un hash (anche con spazi, due punti o trattini fra i gruppi).
        let whole = normalize_hex(text);
        if let Some(hex) = &whole {
            let candidates = Algo::candidates_for_hex_len(hex.len());
            if !candidates.is_empty() {
                return HashInput::Valid {
                    hex: hex.clone(),
                    candidates,
                };
            }
        }
        // 2) Riga copiata da un file di checksum (`hash  nome` o `ALGO (nome) = hash`): si
        //    prende il token esadecimale più lungo.
        let token = text
            .split(|c: char| c.is_whitespace() || c == '=' || c == '(' || c == ')' || c == '*')
            .filter_map(normalize_hex)
            .max_by_key(|s| s.len());
        if let Some(hex) = &token {
            let candidates = Algo::candidates_for_hex_len(hex.len());
            if !candidates.is_empty() {
                return HashInput::Valid {
                    hex: hex.clone(),
                    candidates,
                };
            }
        }
        match whole.or(token) {
            Some(hex) => HashInput::BadLength(hex.len()),
            None => HashInput::NotHex,
        }
    }
}

/// Esito di una voce di un file di checksum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CheckStatus {
    Pending,
    Ok(Algo),
    Failed,
    Missing,
    Error(String),
}

#[derive(Debug, Clone)]
pub(crate) struct ChecksumRow {
    pub entry: ChecksumEntry,
    pub resolved: PathBuf,
    pub canonical: Option<PathBuf>,
    pub file_id: Option<FileId>,
    pub status: CheckStatus,
}

#[derive(Debug, Clone)]
pub(crate) struct ChecksumSession {
    pub path: PathBuf,
    pub name: String,
    pub rows: Vec<ChecksumRow>,
}

impl ChecksumSession {
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        let mut ok = 0;
        let mut failed = 0;
        let mut missing = 0;
        let mut pending = 0;
        for r in &self.rows {
            match r.status {
                CheckStatus::Ok(_) => ok += 1,
                CheckStatus::Failed | CheckStatus::Error(_) => failed += 1,
                CheckStatus::Missing => missing += 1,
                CheckStatus::Pending => pending += 1,
            }
        }
        (ok, failed, missing, pending)
    }
}

#[derive(Debug, Default)]
pub(crate) struct VerifyState {
    pub input: String,
    last_input: String,
    pub parsed: HashInput,
    pub checksum: Option<ChecksumSession>,
    pub focus_request: bool,
    /// Il prossimo evento "incolla" sostituisce il contenuto del campo (pulsante "Incolla").
    pub paste_replace: bool,
}

/// Stato complessivo dell'esecuzione.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RunStatus {
    Idle,
    Running,
    Done {
        ok: usize,
        failed: usize,
        cancelled: bool,
        elapsed: Duration,
    },
}

struct Session {
    handle: EngineHandle,
    rx: Receiver<EngineEvent>,
}

/// Misura della velocità complessiva (media esponenziale).
#[derive(Debug, Default)]
pub(crate) struct SpeedMeter {
    last_t: f64,
    last_bytes: u64,
    pub bytes_per_sec: f64,
}

impl SpeedMeter {
    fn reset(&mut self) {
        *self = SpeedMeter {
            last_t: f64::NAN,
            ..Default::default()
        };
    }
    fn update(&mut self, now: f64, bytes: u64) {
        if self.last_t.is_nan() {
            self.last_t = now;
            self.last_bytes = bytes;
            return;
        }
        let dt = now - self.last_t;
        if dt < 0.25 {
            return;
        }
        let inst = bytes.saturating_sub(self.last_bytes) as f64 / dt;
        self.bytes_per_sec = if self.bytes_per_sec <= 0.0 {
            inst
        } else {
            self.bytes_per_sec * 0.7 + inst * 0.3
        };
        self.last_t = now;
        self.last_bytes = bytes;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum HelpTab {
    #[default]
    Guide,
    Shortcuts,
    About,
}

/// Modalità di avvio di una sessione.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartMode {
    Normal,
    /// Due file da confrontare (selezionati automaticamente).
    Compare,
    /// File elencati in un file di checksum.
    Checksum,
}

/// Azioni richieste dalle viste, eseguite a fine frame.
#[derive(Debug, Clone)]
pub(crate) enum Action {
    OpenFiles,
    OpenFolder,
    OpenChecksum,
    ShowExport,
    ClearList,
    RemoveSelected,
    Remove(FileId),
    Cancel,
    OpenSettings,
    OpenHelp(HelpTab),
    ToggleVerify,
    CycleTheme,
    Recompute,
    /// Ricalcola i file annullati, mantenendo quelli già completati.
    ResumeCancelled,
    ResetAlgos,
    ToggleAlgo(Algo),
    Quit,
    Copy {
        text: String,
        toast: String,
    },
    CopyAll(FileId),
    OpenDir(PathBuf),
    RowClick {
        id: FileId,
        mods: Modifiers,
    },
    ToggleExpand(FileId),
}

/// Hook per gli screenshot dei test headless.
#[derive(Debug, Default)]
struct ShotHook {
    path: Option<PathBuf>,
    after_frames: u32,
    frames: u32,
    wait_done: bool,
    done_at: Option<f64>,
    requested: bool,
    started: Option<f64>,
    /// Comandi di `HASHER_UI` da eseguire a calcolo terminato (aprire finestre, selezionare righe).
    deferred: Vec<String>,
    deferred_at: Option<f64>,
    /// Mostra la sovrimpressione di trascinamento (verifica grafica senza un vero drag & drop).
    force_overlay: bool,
    /// Annulla il calcolo dopo 40 frame (verifica grafica dello stato "annullato").
    cancel_early: bool,
}

/// Stato delle finestre e dei controlli dell'interfaccia.
#[derive(Debug)]
pub(crate) struct UiState {
    pub settings_open: bool,
    pub settings_draft: Option<Settings>,
    pub help_open: bool,
    pub help_tab: HelpTab,
    pub export_open: bool,
    pub export: export::ExportDialog,
    pub column_algo: Algo,
    pub filter: String,
    pub last_clicked: Option<FileId>,
    /// Apre il menu degli algoritmi al prossimo frame (hook per i test grafici).
    pub open_algo_menu: bool,
}

pub struct HasherApp {
    pub(crate) settings: Settings,
    /// Algoritmi attivi (ordine di `Algo::ALL`).
    pub(crate) algos: Vec<Algo>,
    pub(crate) entries: Vec<FileEntry>,
    index: HashMap<FileId, usize>,
    session: Option<Session>,
    /// Input dell'ultima sessione (per ricalcolare).
    pub(crate) inputs: Vec<PathBuf>,
    /// Vista elenco anche con un solo file (cartelle, più input).
    pub(crate) multi: bool,
    pub(crate) status: RunStatus,
    pub(crate) speed: SpeedMeter,
    pub(crate) verify: VerifyState,
    pub(crate) verify_open: bool,
    pub(crate) ui: UiState,
    pub(crate) actions: Vec<Action>,
    /// Ultima frazione nota dell'avanzamento globale.
    pub(crate) fraction: f32,
    pub(crate) bytes_done: u64,
    pub(crate) bytes_total: u64,
    pub(crate) files_total: u64,
    pub(crate) files_done: u64,
    compare_on_discover: bool,
    /// Gli id del motore ripartono da 1 a ogni sessione: quando una sessione aggiunge file a un
    /// elenco esistente (ripresa dei file annullati) gli id vengono traslati di questo valore.
    id_base: FileId,
    verdicts_dirty: bool,
    hex_uppercase: bool,
    theme_override: Option<ThemeMode>,
    applied_theme: Option<(ThemeMode, bool)>,
    window: Option<WindowState>,
    shot: ShotHook,
    /// Notifiche generate fuori dal frame (senza `Context`), mostrate come toast.
    notices: Vec<(ToastKind, String)>,
}

impl HasherApp {
    fn new(cc: &eframe::CreationContext<'_>, args: CliArgs, settings: Settings) -> Self {
        let ctx = &cc.egui_ctx;
        egui_extras::install_image_loaders(ctx);
        theme::install_fonts(ctx);
        apply_language(settings.language);

        let theme_override = std::env::var("HASHER_THEME")
            .ok()
            .and_then(|v| match v.to_ascii_lowercase().as_str() {
                "light" => Some(ThemeMode::Light),
                "dark" => Some(ThemeMode::Dark),
                "system" => Some(ThemeMode::System),
                _ => None,
            });

        let mut algos: Vec<Algo> = Algo::ALL
            .iter()
            .copied()
            .filter(|a| settings.default_algos.contains(a))
            .collect();
        if algos.is_empty() {
            algos = Algo::DEFAULT.to_vec();
        }
        let column_algo = if algos.contains(&Algo::Sha256) {
            Algo::Sha256
        } else {
            algos[0]
        };
        let export = export::ExportDialog::from_settings(&settings);

        let mut app = HasherApp {
            hex_uppercase: settings.uppercase_hex,
            window: settings.window,
            settings,
            algos,
            entries: Vec::new(),
            index: HashMap::new(),
            session: None,
            inputs: Vec::new(),
            multi: false,
            status: RunStatus::Idle,
            speed: SpeedMeter::default(),
            verify: VerifyState::default(),
            verify_open: false,
            ui: UiState {
                settings_open: false,
                settings_draft: None,
                help_open: false,
                help_tab: HelpTab::Guide,
                export_open: false,
                export,
                column_algo,
                filter: String::new(),
                last_clicked: None,
                open_algo_menu: false,
            },
            actions: Vec::new(),
            fraction: 0.0,
            bytes_done: 0,
            bytes_total: 0,
            files_total: 0,
            files_done: 0,
            compare_on_discover: false,
            id_base: 0,
            verdicts_dirty: false,
            theme_override,
            applied_theme: None,
            notices: Vec::new(),
            shot: ShotHook {
                wait_done: !args.files.is_empty(),
                path: args.screenshot.clone(),
                after_frames: args.screenshot_after_frames.max(1),
                ..Default::default()
            },
        };
        app.apply_theme(ctx);
        app.apply_ui_env();

        if !args.files.is_empty() {
            app.open_paths(args.files, Modifiers::NONE);
        }
        app
    }

    /// Variabili d'ambiente per i test grafici headless (screenshot di finestre e stati):
    /// `HASHER_VERIFY_INPUT=<hash>` precompila il campo di verifica; `HASHER_UI=a,b,…` con
    /// `settings`, `help`, `shortcuts`, `about`, `verify`, `overlay`, `algos`, `cancel`, `nowait` (screenshot dopo
    /// `--screenshot-frames` frame anche se il calcolo è in corso), `export`, `select=0+1`,
    /// `expand=0` (gli ultimi tre vengono eseguiti a calcolo terminato).
    fn apply_ui_env(&mut self) {
        if let Ok(v) = std::env::var("HASHER_VERIFY_INPUT") {
            self.verify.input = v;
            self.verify_open = true;
        }
        let Ok(v) = std::env::var("HASHER_UI") else { return };
        for cmd in v.split(',').map(str::trim).filter(|c| !c.is_empty()) {
            match cmd {
                "settings" => self.actions.push(Action::OpenSettings),
                "help" => self.actions.push(Action::OpenHelp(HelpTab::Guide)),
                "shortcuts" => self.actions.push(Action::OpenHelp(HelpTab::Shortcuts)),
                "about" => self.actions.push(Action::OpenHelp(HelpTab::About)),
                "verify" => self.verify_open = true,
                "overlay" => self.shot.force_overlay = true,
                "algos" => self.ui.open_algo_menu = true,
                "cancel" => self.shot.cancel_early = true,
                "nowait" => self.shot.wait_done = false,
                other => self.shot.deferred.push(other.to_owned()),
            }
        }
    }

    fn run_deferred(&mut self, cmd: &str) {
        let indices = |s: &str| -> Vec<usize> { s.split('+').filter_map(|n| n.trim().parse().ok()).collect() };
        if cmd == "export" {
            self.actions.push(Action::ShowExport);
        } else if let Some(list) = cmd.strip_prefix("select=") {
            let idx = indices(list);
            for (i, e) in self.entries.iter_mut().enumerate() {
                e.selected = idx.contains(&i);
            }
            if self.compare_pair().is_some() {
                self.verify_open = true;
            }
        } else if let Some(list) = cmd.strip_prefix("expand=") {
            for i in indices(list) {
                if let Some(e) = self.entries.get_mut(i) {
                    e.expanded = true;
                }
            }
        } else {
            log::warn!("HASHER_UI: comando sconosciuto {cmd}");
        }
    }

    // -----------------------------------------------------------------------------------------
    // Tema e impostazioni
    // -----------------------------------------------------------------------------------------

    pub(crate) fn effective_theme(&self) -> ThemeMode {
        // Anteprima dalla finestra impostazioni solo se l'utente ha cambiato il tema nella bozza.
        if let Some(draft) = self
            .ui
            .settings_draft
            .as_ref()
            .filter(|d| d.theme != self.settings.theme)
        {
            return draft.theme;
        }
        self.theme_override.unwrap_or(self.settings.theme)
    }

    fn effective_animations(&self) -> bool {
        self.ui
            .settings_draft
            .as_ref()
            .map_or(self.settings.animations, |d| d.animations)
    }

    fn apply_theme(&mut self, ctx: &Context) {
        let wanted = (self.effective_theme(), self.effective_animations());
        if self.applied_theme != Some(wanted) {
            theme::apply(ctx, wanted.0, wanted.1);
            self.applied_theme = Some(wanted);
        }
    }

    /// Salva le impostazioni su disco; in caso di errore mostra un toast se richiesto.
    pub(crate) fn save_settings(&mut self, ctx: &Context, notify: bool) -> bool {
        if self.shot.path.is_some() {
            // Nei test headless non si tocca il file impostazioni.
            return true;
        }
        let res = guard(|| self.settings.save());
        match res {
            Ok(Ok(())) => true,
            Ok(Err(e)) => {
                log::warn!("salvataggio impostazioni fallito: {e}");
                if notify {
                    widgets::toast(ctx, ToastKind::Error, t!("toast.settings_error", error = e.to_string()));
                }
                false
            }
            Err(e) => {
                log::warn!("salvataggio impostazioni non disponibile: {e}");
                if notify {
                    widgets::toast(ctx, ToastKind::Error, t!("toast.settings_error", error = e));
                }
                false
            }
        }
    }

    /// Imposta la preferenza esadecimale maiuscola/minuscola (ricostruisce le cache).
    pub(crate) fn set_uppercase(&mut self, uppercase: bool) {
        if self.hex_uppercase != uppercase {
            self.hex_uppercase = uppercase;
            for e in &mut self.entries {
                e.rebuild_hexes(uppercase);
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Sessioni del motore
    // -----------------------------------------------------------------------------------------

    pub(crate) fn is_running(&self) -> bool {
        self.session.is_some()
    }

    /// Apre file o cartelle (trascinati, da dialogo o da riga di comando).
    pub(crate) fn open_paths(&mut self, paths: Vec<PathBuf>, mods: Modifiers) {
        let paths: Vec<PathBuf> = paths.into_iter().filter(|p| !p.as_os_str().is_empty()).collect();
        if paths.is_empty() {
            return;
        }
        if paths.len() == 1 && paths[0].is_file() && checksum_file::is_checksum_file(&paths[0]) {
            let p = paths[0].clone();
            self.load_checksum(&p);
            return;
        }
        if mods.shift && paths.len() == 1 && paths[0].is_file() {
            let previous = self.entries.last().map(|e| e.path.clone());
            if let Some(prev) = previous.filter(|p| *p != paths[0]) {
                self.start_session(vec![prev, paths[0].clone()], StartMode::Compare);
                return;
            }
        }
        if (mods.ctrl || mods.command) && paths.len() == 2 && paths.iter().all(|p| p.is_file()) {
            self.start_session(paths, StartMode::Compare);
            return;
        }
        self.start_session(paths, StartMode::Normal);
    }

    fn start_session(&mut self, paths: Vec<PathBuf>, mode: StartMode) {
        self.start_session_ext(paths, mode, false);
    }

    /// Avvia una sessione; con `keep` i file già in elenco restano (ripresa dei file annullati).
    fn start_session_ext(&mut self, paths: Vec<PathBuf>, mode: StartMode, keep: bool) {
        self.cancel_session();
        if keep {
            self.id_base = self.entries.iter().map(|e| e.id).max().unwrap_or(0);
            self.multi = true;
        } else {
            self.entries.clear();
            self.index.clear();
            self.id_base = 0;
            self.ui.last_clicked = None;
            self.multi = paths.len() > 1 || paths.iter().any(|p| p.is_dir());
            self.inputs = paths.clone();
        }
        self.compare_on_discover = mode == StartMode::Compare;
        if mode != StartMode::Checksum {
            self.verify.checksum = None;
        } else if let Some(cs) = &mut self.verify.checksum {
            for r in &mut cs.rows {
                let kept = keep && r.file_id.is_some_and(|id| self.index.contains_key(&id));
                if !kept {
                    r.file_id = None;
                    r.status = if r.resolved.is_file() {
                        CheckStatus::Pending
                    } else {
                        CheckStatus::Missing
                    };
                }
            }
        }
        if mode == StartMode::Compare {
            self.verify_open = true;
        }

        let mut algos = self.algos.clone();
        if let Some(cs) = &self.verify.checksum {
            for r in &cs.rows {
                algos.extend(r.entry.candidate_algos());
            }
        }
        algos.sort();
        algos.dedup();

        let config = EngineConfig {
            algos,
            parallel_files: self.settings.parallel_files,
            recurse: self.settings.recurse_folders,
            follow_symlinks: self.settings.follow_symlinks,
            include_hidden: self.settings.include_hidden,
        };
        self.fraction = 0.0;
        self.bytes_done = 0;
        self.bytes_total = paths
            .iter()
            .filter_map(|p| p.metadata().ok())
            .filter(|m| m.is_file())
            .map(|m| m.len())
            .sum();
        self.files_total = 0;
        self.files_done = 0;
        self.verdicts_dirty = true;
        match guard(|| engine::start(paths, config)) {
            Ok((handle, rx)) => {
                self.session = Some(Session { handle, rx });
                self.status = RunStatus::Running;
                self.speed.reset();
            }
            Err(e) => {
                log::error!("avvio del motore fallito: {e}");
                self.status = RunStatus::Idle;
                self.shot.done_at = Some(f64::NEG_INFINITY);
                self.notify(ToastKind::Error, t!("toast.engine_error", error = e));
            }
        }
    }

    /// Accoda un toast da mostrare al prossimo frame.
    pub(crate) fn notify(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.notices.push((kind, text.into()));
    }

    /// Annulla la sessione in corso (gli eventi successivi vengono ignorati).
    fn cancel_session(&mut self) {
        if let Some(s) = self.session.take() {
            s.handle.cancel();
        }
        for e in &mut self.entries {
            if matches!(e.state, EntryState::Queued | EntryState::Computing { .. }) {
                e.state = EntryState::Cancelled;
            }
        }
    }

    /// Annulla dall'interfaccia (pulsante o Esc): mantiene i risultati già calcolati.
    pub(crate) fn cancel(&mut self) {
        let Some(s) = &self.session else { return };
        s.handle.cancel();
        // L'evento `AllDone { cancelled: true }` chiuderà la sessione; se il motore non
        // rispondesse, il prossimo avvio la sostituisce comunque.
    }

    pub(crate) fn clear(&mut self) {
        if let Some(s) = self.session.take() {
            s.handle.cancel();
        }
        self.entries.clear();
        self.index.clear();
        self.id_base = 0;
        self.inputs.clear();
        self.multi = false;
        self.status = RunStatus::Idle;
        self.verify.checksum = None;
        self.fraction = 0.0;
        self.bytes_done = 0;
        self.bytes_total = 0;
        self.files_total = 0;
        self.files_done = 0;
        self.ui.last_clicked = None;
        self.verdicts_dirty = true;
    }

    /// Cartella del file relativa all'input che la contiene (o percorso completo della cartella).
    fn relative_dir(&self, path: &Path) -> String {
        let parent = path.parent().unwrap_or(Path::new(""));
        for input in self.inputs.iter().filter(|i| i.is_dir()) {
            if let Ok(rel) = parent.strip_prefix(input) {
                let base = input
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let rel = rel.to_string_lossy();
                return if rel.is_empty() {
                    base
                } else {
                    format!("{base}{}{rel}", std::path::MAIN_SEPARATOR)
                };
            }
        }
        // File aperti singolarmente: le ultime due cartelle bastano a riconoscerli.
        let parts: Vec<String> = parent
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let sep = std::path::MAIN_SEPARATOR;
        if parts.len() > 3 {
            format!("…{sep}{}{sep}{}", parts[parts.len() - 2], parts[parts.len() - 1])
        } else {
            parent.to_string_lossy().into_owned()
        }
    }

    fn rebuild_index(&mut self) {
        self.index = self.entries.iter().enumerate().map(|(i, e)| (e.id, i)).collect();
    }

    pub(crate) fn remove_entries(&mut self, pred: impl Fn(&FileEntry) -> bool) {
        self.entries.retain(|e| !pred(e));
        self.rebuild_index();
        if self.entries.is_empty() && !self.is_running() {
            self.clear();
        }
        self.verdicts_dirty = true;
    }

    fn entry_mut(&mut self, id: FileId) -> Option<&mut FileEntry> {
        let i = *self.index.get(&id)?;
        self.entries.get_mut(i)
    }

    pub(crate) fn entry(&self, id: FileId) -> Option<&FileEntry> {
        let i = *self.index.get(&id)?;
        self.entries.get(i)
    }

    /// Consuma gli eventi del motore disponibili (senza bloccare).
    fn poll_engine(&mut self, ctx: &Context) {
        let now = anim::now(ctx);
        let mut events = Vec::new();
        if let Some(s) = &self.session {
            while events.len() < 50_000 {
                match s.rx.try_recv() {
                    Ok(ev) => events.push(ev),
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        if !events.iter().any(|e| matches!(e, EngineEvent::AllDone { .. })) {
                            // Il motore è terminato senza `AllDone` (es. panic in un thread).
                            events.push(EngineEvent::AllDone {
                                files_ok: 0,
                                files_failed: 0,
                                cancelled: s.handle.is_cancelled(),
                                elapsed: Duration::ZERO,
                            });
                        }
                        break;
                    }
                }
            }
            self.fraction = s.handle.fraction();
            self.bytes_done = s.handle.bytes_done();
            self.bytes_total = self.bytes_total.max(s.handle.bytes_total());
            self.files_total = s.handle.files_total();
            self.files_done = s.handle.files_done();
            self.speed.update(now, self.bytes_done);
        }
        for ev in events {
            self.on_event(ev, now);
        }
        if self.is_running() {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }

    fn on_event(&mut self, ev: EngineEvent, now: f64) {
        let base = self.id_base;
        let ev = match ev {
            EngineEvent::Discovered { id, path, size } => EngineEvent::Discovered {
                id: id + base,
                path,
                size,
            },
            EngineEvent::Started { id } => EngineEvent::Started { id: id + base },
            EngineEvent::Progress { id, bytes_done } => EngineEvent::Progress {
                id: id + base,
                bytes_done,
            },
            EngineEvent::Finished(mut r) => {
                r.id += base;
                EngineEvent::Finished(r)
            }
            EngineEvent::Failed { id, path, error } => EngineEvent::Failed {
                id: id + base,
                path,
                error,
            },
            other => other,
        };
        match ev {
            EngineEvent::Discovered { id, path, size } => {
                if self.index.contains_key(&id) {
                    return;
                }
                self.index.insert(id, self.entries.len());
                let mut entry = FileEntry::new(id, path.clone(), size, now);
                entry.dir_text = self.relative_dir(&path);
                self.entries.push(entry);
                self.link_checksum(id, &path);
                if self.compare_on_discover && self.entries.len() == 2 {
                    for e in &mut self.entries {
                        e.selected = true;
                    }
                }
            }
            EngineEvent::Started { id } => {
                if let Some(e) = self.entry_mut(id) {
                    e.state = EntryState::Computing {
                        bytes_done: 0,
                        started: now,
                    };
                }
            }
            EngineEvent::Progress { id, bytes_done } => {
                if let Some(e) = self.entry_mut(id) {
                    match &mut e.state {
                        EntryState::Computing { bytes_done: b, .. } => *b = bytes_done,
                        EntryState::Queued => {
                            e.state = EntryState::Computing {
                                bytes_done,
                                started: now,
                            }
                        }
                        _ => {}
                    }
                }
            }
            EngineEvent::Finished(result) => {
                let id = result.id;
                let uppercase = self.hex_uppercase;
                if !self.index.contains_key(&id) {
                    // Risultato di un file mai annunciato: lo aggiungiamo comunque.
                    self.index.insert(id, self.entries.len());
                    self.entries
                        .push(FileEntry::new(id, result.info.path.clone(), result.info.size, now));
                }
                if let Some(e) = self.entry_mut(id) {
                    e.size = result.info.size;
                    e.size_text = format::size(result.info.size);
                    e.state = EntryState::Done(result);
                    e.rebuild_hexes(uppercase);
                }
                self.update_checksum_status(id);
                self.verdicts_dirty = true;
            }
            EngineEvent::Failed { id, path, error } => {
                if !self.index.contains_key(&id) {
                    self.index.insert(id, self.entries.len());
                    self.entries.push(FileEntry::new(id, path, 0, now));
                }
                if let Some(e) = self.entry_mut(id) {
                    e.state = EntryState::Failed(error.clone());
                }
                if let Some(cs) = &mut self.verify.checksum {
                    for r in cs.rows.iter_mut().filter(|r| r.file_id == Some(id)) {
                        r.status = CheckStatus::Error(error.clone());
                    }
                }
            }
            EngineEvent::AllDone {
                files_ok,
                files_failed,
                cancelled,
                elapsed,
            } => {
                self.session = None;
                self.status = RunStatus::Done {
                    ok: files_ok,
                    failed: files_failed,
                    cancelled,
                    elapsed,
                };
                if !cancelled {
                    self.fraction = 1.0;
                }
                for e in &mut self.entries {
                    if matches!(e.state, EntryState::Queued | EntryState::Computing { .. }) {
                        e.state = EntryState::Cancelled;
                    }
                }
                if self.shot.done_at.is_none() {
                    self.shot.done_at = Some(now);
                }
                self.verdicts_dirty = true;
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Verifica: hash incollato, file di checksum, confronto
    // -----------------------------------------------------------------------------------------

    /// Carica un file di checksum e avvia la verifica dei file elencati.
    pub(crate) fn load_checksum(&mut self, path: &Path) {
        let loaded = match guard(|| checksum_file::load(path)) {
            Ok(Ok(f)) => f,
            Ok(Err(e)) => {
                self.notify(ToastKind::Error, t!("toast.checksum_error", error = e.to_string()));
                return;
            }
            Err(e) => {
                self.notify(ToastKind::Error, t!("toast.checksum_error", error = e));
                return;
            }
        };
        let rows: Vec<ChecksumRow> = loaded
            .entries
            .iter()
            .map(|entry| {
                let p = Path::new(&entry.file_name);
                let resolved = if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    loaded.base_dir.join(p)
                };
                let exists = resolved.is_file();
                ChecksumRow {
                    entry: entry.clone(),
                    canonical: if exists { resolved.canonicalize().ok() } else { None },
                    resolved,
                    file_id: None,
                    status: if exists {
                        CheckStatus::Pending
                    } else {
                        CheckStatus::Missing
                    },
                }
            })
            .collect();
        let mut paths: Vec<PathBuf> = rows
            .iter()
            .filter(|r| r.status == CheckStatus::Pending)
            .map(|r| r.resolved.clone())
            .collect();
        paths.dedup();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let total = rows.len();
        self.verify.checksum = Some(ChecksumSession {
            path: path.to_path_buf(),
            name,
            rows,
        });
        self.verify_open = true;
        if total == 0 {
            self.notify(ToastKind::Warning, t!("toast.checksum_empty"));
        }
        if paths.is_empty() {
            if let Some(s) = self.session.take() {
                s.handle.cancel();
            }
            self.entries.clear();
            self.index.clear();
            self.status = RunStatus::Idle;
        } else {
            self.start_session(paths, StartMode::Checksum);
            self.multi = true;
        }
        self.verdicts_dirty = true;
    }

    fn link_checksum(&mut self, id: FileId, path: &Path) {
        let Some(cs) = &mut self.verify.checksum else { return };
        let mut canonical: Option<Option<PathBuf>> = None;
        for r in cs
            .rows
            .iter_mut()
            .filter(|r| r.file_id.is_none() && r.status == CheckStatus::Pending)
        {
            let same = r.resolved == path || {
                let c = canonical.get_or_insert_with(|| path.canonicalize().ok());
                c.is_some() && *c == r.canonical
            };
            if same {
                r.file_id = Some(id);
            }
        }
    }

    fn update_checksum_status(&mut self, id: FileId) {
        let Some(i) = self.index.get(&id).copied() else { return };
        let Some(cs) = &mut self.verify.checksum else { return };
        let Some(result) = self.entries.get(i).and_then(|e| e.result()) else {
            return;
        };
        for r in cs.rows.iter_mut().filter(|r| r.file_id == Some(id)) {
            let hit = r
                .entry
                .candidate_algos()
                .into_iter()
                .find(|a| result.hex(*a, false).as_deref() == Some(r.entry.hex.as_str()));
            r.status = match hit {
                Some(a) => CheckStatus::Ok(a),
                None => CheckStatus::Failed,
            };
        }
    }

    /// Ricalcola esiti e corrispondenze dopo un cambiamento (input, nuovi risultati).
    fn refresh_verdicts(&mut self) {
        let input = std::mem::take(&mut self.verify.input);
        if input != self.verify.last_input {
            self.verify.parsed = HashInput::parse(&input);
            self.verify.last_input.clone_from(&input);
            self.verdicts_dirty = true;
        }
        self.verify.input = input;
        if !self.verdicts_dirty {
            return;
        }
        self.verdicts_dirty = false;
        let hex = match &self.verify.parsed {
            HashInput::Valid { hex, .. } => Some(hex.clone()),
            _ => None,
        };
        let checksum = self.verify.checksum.as_ref();
        for e in &mut self.entries {
            e.match_algos = match (&hex, e.result()) {
                (Some(h), Some(r)) => r.matching_algos(h),
                _ => Vec::new(),
            };
            let cs_status = checksum.and_then(|cs| cs.rows.iter().find(|r| r.file_id == Some(e.id)).map(|r| &r.status));
            e.verdict = if !e.match_algos.is_empty() {
                Verdict::Match
            } else {
                match cs_status {
                    Some(CheckStatus::Ok(_)) => Verdict::Match,
                    Some(CheckStatus::Failed) => Verdict::Mismatch,
                    _ => Verdict::None,
                }
            };
        }
    }

    /// Le due voci da confrontare (esattamente due selezionate).
    pub(crate) fn compare_pair(&self) -> Option<(&FileEntry, &FileEntry)> {
        let mut sel = self.entries.iter().filter(|e| e.selected);
        let a = sel.next()?;
        let b = sel.next()?;
        if sel.next().is_some() {
            return None;
        }
        Some((a, b))
    }

    /// Esito da mostrare su una riga hash di un file.
    pub(crate) fn algo_verdict(&self, e: &FileEntry, algo: Algo) -> Verdict {
        if !e.is_done() {
            return Verdict::None;
        }
        if let HashInput::Valid { candidates, .. } = &self.verify.parsed {
            if e.match_algos.contains(&algo) {
                return Verdict::Match;
            }
            if e.match_algos.is_empty() && candidates.contains(&algo) {
                return Verdict::Mismatch;
            }
            return Verdict::None;
        }
        if let Some(cs) = &self.verify.checksum {
            if let Some(r) = cs.rows.iter().find(|r| r.file_id == Some(e.id)) {
                if r.entry.candidate_algos().contains(&algo) {
                    return match &r.status {
                        CheckStatus::Ok(a) if *a == algo => Verdict::Match,
                        CheckStatus::Ok(_) => Verdict::None,
                        CheckStatus::Failed => Verdict::Mismatch,
                        _ => Verdict::None,
                    };
                }
            }
        }
        if let Some((a, b)) = self.compare_pair() {
            if a.id == e.id || b.id == e.id {
                return match (a.hex(algo), b.hex(algo)) {
                    (Some(x), Some(y)) if x == y => Verdict::Match,
                    (Some(_), Some(_)) => Verdict::Mismatch,
                    _ => Verdict::None,
                };
            }
        }
        Verdict::None
    }

    /// Algoritmi da mostrare nelle righe hash.
    pub(crate) fn shown_algos(&self) -> &[Algo] {
        &self.algos
    }

    /// Risultati completati, nell'ordine dell'elenco (eventualmente solo i selezionati).
    pub(crate) fn done_results(&self, selected_only: bool) -> Vec<FileResult> {
        self.entries
            .iter()
            .filter(|e| !selected_only || e.selected)
            .filter_map(|e| e.result().cloned())
            .collect()
    }

    pub(crate) fn any_done(&self) -> bool {
        self.entries.iter().any(|e| e.is_done())
    }

    // -----------------------------------------------------------------------------------------
    // Azioni
    // -----------------------------------------------------------------------------------------

    fn process_actions(&mut self, ctx: &Context) {
        let actions = std::mem::take(&mut self.actions);
        for a in actions {
            self.run_action(ctx, a);
        }
    }

    fn run_action(&mut self, ctx: &Context, action: Action) {
        match action {
            Action::OpenFiles => {
                let mut dlg = rfd::FileDialog::new().set_title(t!("menu.open_files"));
                if let Some(dir) = self.inputs.first().and_then(|p| p.parent()) {
                    dlg = dlg.set_directory(dir);
                }
                if let Some(files) = dlg.pick_files() {
                    let mods = ctx.input(|i| i.modifiers);
                    self.open_paths(files, mods);
                }
            }
            Action::OpenFolder => {
                if let Some(dir) = rfd::FileDialog::new().set_title(t!("menu.open_folder")).pick_folder() {
                    self.open_paths(vec![dir], Modifiers::NONE);
                }
            }
            Action::OpenChecksum => {
                let exts: Vec<&str> = checksum_file::CHECKSUM_EXTENSIONS.to_vec();
                if let Some(file) = rfd::FileDialog::new()
                    .set_title(t!("menu.open_checksum"))
                    .add_filter(t!("verify.checksum_filter"), &exts)
                    .pick_file()
                {
                    self.load_checksum(&file);
                }
            }
            Action::ShowExport => {
                if self.any_done() {
                    self.ui
                        .export
                        .prepare(&self.algos, self.entries.iter().any(|e| e.selected && e.is_done()));
                    self.ui.export_open = true;
                } else {
                    widgets::toast(ctx, ToastKind::Info, t!("toast.nothing_to_export"));
                }
            }
            Action::ClearList => self.clear(),
            Action::RemoveSelected => self.remove_entries(|e| e.selected),
            Action::Remove(id) => self.remove_entries(|e| e.id == id),
            Action::Cancel => self.cancel(),
            Action::OpenSettings => {
                self.ui.settings_draft = Some(self.settings.clone());
                self.ui.settings_open = true;
            }
            Action::OpenHelp(tab) => {
                self.ui.help_tab = tab;
                self.ui.help_open = true;
            }
            Action::ToggleVerify => {
                self.verify_open = !self.verify_open;
                if self.verify_open {
                    self.verify.focus_request = true;
                }
            }
            Action::CycleTheme => {
                let next = match self.effective_theme() {
                    ThemeMode::System => ThemeMode::Light,
                    ThemeMode::Light => ThemeMode::Dark,
                    ThemeMode::Dark => ThemeMode::System,
                };
                self.theme_override = None;
                self.settings.theme = next;
                if let Some(d) = &mut self.ui.settings_draft {
                    d.theme = next;
                }
                self.save_settings(ctx, false);
            }
            Action::Recompute => {
                if !self.inputs.is_empty() {
                    let mode = if self.verify.checksum.is_some() {
                        StartMode::Checksum
                    } else {
                        StartMode::Normal
                    };
                    let multi = self.multi;
                    self.start_session(self.inputs.clone(), mode);
                    self.multi = multi;
                }
            }
            Action::ResumeCancelled => self.resume_cancelled(),
            Action::ResetAlgos => {
                self.algos = Algo::ALL
                    .iter()
                    .copied()
                    .filter(|a| self.settings.default_algos.contains(a))
                    .collect();
                if self.algos.is_empty() {
                    self.algos = Algo::DEFAULT.to_vec();
                }
                self.fix_column_algo();
            }
            Action::ToggleAlgo(a) => {
                if self.algos.contains(&a) {
                    if self.algos.len() > 1 {
                        self.algos.retain(|x| *x != a);
                    }
                } else {
                    self.algos.push(a);
                    self.algos.sort();
                }
                self.fix_column_algo();
            }
            Action::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
            Action::Copy { text, toast } => {
                ctx.copy_text(text);
                if !toast.is_empty() {
                    widgets::toast(ctx, ToastKind::Success, toast);
                }
            }
            Action::CopyAll(id) => {
                if let Some(e) = self.entry(id) {
                    let mut text = String::new();
                    for a in &self.algos {
                        if let Some(h) = e.hex(*a) {
                            text.push_str(&format!("{:<12} {}\n", a.name(), h));
                        }
                    }
                    if !text.is_empty() {
                        ctx.copy_text(text.trim_end().to_owned());
                        widgets::toast(ctx, ToastKind::Success, t!("toast.copied_all"));
                    }
                }
            }
            Action::OpenDir(dir) => {
                if let Err(e) = open::that_detached(&dir) {
                    widgets::toast(ctx, ToastKind::Error, t!("toast.open_dir_error", error = e.to_string()));
                }
            }
            Action::RowClick { id, mods } => self.row_click(id, mods),
            Action::ToggleExpand(id) => {
                if let Some(e) = self.entry_mut(id) {
                    e.expanded = !e.expanded;
                }
            }
        }
    }

    /// File annullati presenti in elenco.
    pub(crate) fn cancelled_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.state, EntryState::Cancelled))
            .count()
    }

    fn resume_cancelled(&mut self) {
        if self.is_running() {
            return;
        }
        let paths: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|e| matches!(e.state, EntryState::Cancelled))
            .map(|e| e.path.clone())
            .collect();
        if paths.is_empty() {
            return;
        }
        if self.entries.len() == paths.len() {
            // Nessun risultato da conservare: si ricomincia dagli input originali.
            let inputs = if self.inputs.is_empty() {
                paths
            } else {
                self.inputs.clone()
            };
            let mode = if self.verify.checksum.is_some() {
                StartMode::Checksum
            } else {
                StartMode::Normal
            };
            let multi = self.multi;
            self.start_session(inputs, mode);
            self.multi = multi;
            return;
        }
        self.remove_entries(|e| matches!(e.state, EntryState::Cancelled));
        let mode = if self.verify.checksum.is_some() {
            StartMode::Checksum
        } else {
            StartMode::Normal
        };
        self.start_session_ext(paths, mode, true);
    }

    fn fix_column_algo(&mut self) {
        if !self.algos.contains(&self.ui.column_algo) {
            self.ui.column_algo = if self.algos.contains(&Algo::Sha256) {
                Algo::Sha256
            } else {
                self.algos[0]
            };
        }
    }

    fn row_click(&mut self, id: FileId, mods: Modifiers) {
        if mods.command || mods.ctrl {
            if let Some(e) = self.entry_mut(id) {
                e.selected = !e.selected;
            }
            self.ui.last_clicked = Some(id);
            if self.compare_pair().is_some() {
                self.verify_open = true;
            }
        } else if mods.shift {
            let anchor = self.ui.last_clicked.and_then(|a| self.index.get(&a).copied());
            let target = self.index.get(&id).copied();
            if let (Some(a), Some(b)) = (anchor, target) {
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                for (i, e) in self.entries.iter_mut().enumerate() {
                    e.selected = (lo..=hi).contains(&i);
                }
            }
        } else {
            for e in &mut self.entries {
                if e.id == id {
                    e.selected = true;
                    e.expanded = !e.expanded;
                } else {
                    e.selected = false;
                }
            }
            self.ui.last_clicked = Some(id);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Input: tastiera, incolla, trascinamento
    // -----------------------------------------------------------------------------------------

    fn handle_input(&mut self, ctx: &Context) {
        let text_focus = ctx.text_edit_focused();
        let popup = ctx.any_popup_open();
        let (open_folder, open_files, export, settings, help, esc, del, clear) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::O),
                i.consume_key(Modifiers::COMMAND, Key::O),
                i.consume_key(Modifiers::COMMAND, Key::E),
                i.consume_key(Modifiers::COMMAND, Key::Comma),
                i.consume_key(Modifiers::NONE, Key::F1),
                !text_focus && !popup && i.consume_key(Modifiers::NONE, Key::Escape),
                !text_focus && i.consume_key(Modifiers::NONE, Key::Delete),
                i.consume_key(Modifiers::COMMAND, Key::L),
            )
        });
        if open_folder {
            self.actions.push(Action::OpenFolder);
        }
        if open_files {
            self.actions.push(Action::OpenFiles);
        }
        if export {
            self.actions.push(Action::ShowExport);
        }
        if settings {
            self.actions.push(Action::OpenSettings);
        }
        if help {
            self.actions.push(Action::OpenHelp(HelpTab::Guide));
        }
        if clear {
            self.actions.push(Action::ClearList);
        }
        if del && self.entries.iter().any(|e| e.selected) {
            self.actions.push(Action::RemoveSelected);
        }
        if esc {
            if self.is_running() {
                self.actions.push(Action::Cancel);
            } else if self.ui.export_open {
                self.ui.export_open = false;
            } else if self.ui.settings_open {
                self.close_settings(ctx, false);
            } else if self.ui.help_open {
                self.ui.help_open = false;
            }
        }

        // Ctrl+V fuori da un campo di testo: incolla nel campo di verifica.
        if !text_focus {
            let pasted = ctx.input_mut(|i| {
                let mut text = None;
                i.events.retain(|e| match e {
                    Event::Paste(s) => {
                        text = Some(s.clone());
                        false
                    }
                    _ => true,
                });
                text
            });
            if let Some(s) = pasted {
                self.verify_open = true;
                if self.verify.input.trim().is_empty() || self.verify.paste_replace {
                    self.verify.input = s.trim().to_owned();
                }
                self.verify.paste_replace = false;
                self.verify.focus_request = true;
            }
        }

        // File rilasciati sulla finestra.
        let (dropped, mods) = ctx.input(|i| {
            (
                i.raw
                    .dropped_files
                    .iter()
                    .map(|f| f.path().to_path_buf())
                    .collect::<Vec<_>>(),
                i.modifiers,
            )
        });
        if !dropped.is_empty() {
            self.open_paths(dropped, mods);
        }
    }

    /// Chiude la finestra impostazioni; `save` applica e salva la bozza.
    pub(crate) fn close_settings(&mut self, ctx: &Context, save: bool) {
        self.ui.settings_open = false;
        let Some(draft) = self.ui.settings_draft.take() else {
            return;
        };
        if save {
            let algos_changed = draft.default_algos != self.settings.default_algos;
            let uppercase = draft.uppercase_hex;
            let window = self.settings.window;
            self.settings = draft;
            self.settings.window = window;
            self.theme_override = None;
            if algos_changed {
                self.actions.push(Action::ResetAlgos);
            }
            self.set_uppercase(uppercase);
            if self.save_settings(ctx, true) {
                widgets::toast(ctx, ToastKind::Success, t!("toast.settings_saved"));
            }
        }
        apply_language(self.settings.language);
        for e in &mut self.entries {
            e.rebuild_hexes(self.hex_uppercase);
        }
    }

    fn capture_window_state(&mut self, ctx: &Context) {
        let (outer, inner, maximized, minimized) = ctx.input(|i| {
            let vp = i.viewport();
            (
                vp.outer_rect,
                vp.inner_rect,
                vp.maximized.unwrap_or(false),
                vp.minimized.unwrap_or(false),
            )
        });
        if minimized {
            return;
        }
        if maximized {
            if let Some(w) = &mut self.window {
                w.maximized = true;
            }
            return;
        }
        if let Some(inner) = inner {
            let pos = outer.map(|o| o.min).unwrap_or(inner.min);
            self.window = Some(WindowState {
                x: pos.x,
                y: pos.y,
                width: inner.width(),
                height: inner.height(),
                maximized: false,
            });
        }
    }

    // -----------------------------------------------------------------------------------------
    // Screenshot per i test headless
    // -----------------------------------------------------------------------------------------

    fn screenshot_hook(&mut self, ctx: &Context) {
        let Some(path) = self.shot.path.clone() else { return };
        let now = anim::now(ctx);
        self.shot.frames += 1;
        let started = *self.shot.started.get_or_insert(now);
        if self.shot.cancel_early && self.shot.frames == 40 {
            self.cancel();
        }

        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            let saved = image::RgbaImage::from_raw(w as u32, h as u32, image.as_raw().to_vec())
                .map(|img| img.save_with_format(&path, image::ImageFormat::Png));
            match saved {
                Some(Ok(())) => log::info!("screenshot salvato in {}", path.display()),
                Some(Err(e)) => log::error!("screenshot non salvato: {e}"),
                None => log::error!("screenshot: dimensioni non valide"),
            }
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }
        if self.shot.requested {
            ctx.request_repaint();
            return;
        }
        let mut done_ready = !self.shot.wait_done || self.shot.done_at.is_some_and(|t| now - t > 0.9);
        if done_ready && !self.shot.deferred.is_empty() {
            for cmd in std::mem::take(&mut self.shot.deferred) {
                self.run_deferred(&cmd);
            }
            self.shot.deferred_at = Some(now);
            ctx.request_repaint();
        }
        if let Some(t) = self.shot.deferred_at {
            done_ready = done_ready && now - t > 0.9;
        }
        let timeout = now - started > 45.0;
        if self.shot.frames >= self.shot.after_frames && (done_ready || timeout) {
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::UserData::default()));
            self.shot.requested = true;
        }
        ctx.request_repaint();
    }
}

impl eframe::App for HasherApp {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.poll_engine(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        anim::set_enabled(&ctx, self.effective_animations());
        self.apply_theme(&ctx);
        self.capture_window_state(&ctx);
        self.handle_input(&ctx);
        self.refresh_verdicts();
        for (kind, text) in std::mem::take(&mut self.notices) {
            widgets::toast(&ctx, kind, text);
        }

        topbar::show(self, ui);
        statusbar::show(self, ui);
        verify::panel(self, ui);
        let p = theme::pal(ui);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::same(16)))
            .show(ui, |ui| {
                if self.entries.is_empty() && !self.is_running() {
                    dropzone::empty_state(self, ui);
                } else if self.entries.len() == 1 && !self.multi {
                    single::show(self, ui);
                } else {
                    list::show(self, ui);
                }
            });

        settings::window(self, &ctx);
        help::window(self, &ctx);
        export::window(self, &ctx);
        dropzone::overlay(&ctx, self.shot.force_overlay);
        widgets::show_toasts(&ctx, 56.0);

        self.process_actions(&ctx);
        self.refresh_verdicts();
        self.apply_theme(&ctx);
        self.screenshot_hook(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(s) = self.session.take() {
            s.handle.cancel();
        }
        if self.shot.path.is_some() {
            return;
        }
        if let Some(w) = self.window {
            self.settings.window = Some(w);
        }
        if let Ok(Err(e)) = guard(|| self.settings.save()) {
            log::warn!("salvataggio impostazioni all'uscita fallito: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA256_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn valid_hex(input: &str) -> Option<String> {
        match HashInput::parse(input) {
            HashInput::Valid { hex, .. } => Some(hex),
            _ => None,
        }
    }

    #[test]
    fn parse_plain_and_grouped() {
        assert_eq!(valid_hex(SHA256_EMPTY).as_deref(), Some(SHA256_EMPTY));
        assert_eq!(valid_hex(&SHA256_EMPTY.to_uppercase()).as_deref(), Some(SHA256_EMPTY));
        let grouped: String = SHA256_EMPTY
            .as_bytes()
            .chunks(8)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(valid_hex(&grouped).as_deref(), Some(SHA256_EMPTY));
    }

    #[test]
    fn parse_checksum_lines() {
        assert_eq!(
            valid_hex(&format!("{SHA256_EMPTY}  empty.txt")).as_deref(),
            Some(SHA256_EMPTY)
        );
        assert_eq!(
            valid_hex(&format!("SHA256 (empty.txt) = {SHA256_EMPTY}")).as_deref(),
            Some(SHA256_EMPTY)
        );
    }

    #[test]
    fn parse_errors() {
        assert_eq!(HashInput::parse("   "), HashInput::Empty);
        assert_eq!(HashInput::parse("hello world"), HashInput::NotHex);
        assert_eq!(HashInput::parse("abcde"), HashInput::BadLength(5));
    }

    #[test]
    fn candidates_by_length() {
        match HashInput::parse("d41d8cd98f00b204e9800998ecf8427e") {
            HashInput::Valid { candidates, .. } => assert!(candidates.contains(&Algo::Md5)),
            other => panic!("unexpected {other:?}"),
        }
    }
}
