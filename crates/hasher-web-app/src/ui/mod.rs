//! Interfaccia grafica egui/eframe: stato dell'applicazione, ciclo di aggiornamento, azioni.
//!
//! Le viste (`topbar`, `single`, `list`, `verify`, …) leggono lo stato di [`HasherApp`] e
//! accodano [`Action`] che vengono eseguite a fine frame, così il codice di disegno resta
//! separato dagli effetti (dialoghi del browser, avvio del motore, salvataggi).
//!
//! Port web dell'interfaccia del desktop: i file arrivano dal modulo `web` (trascinamento,
//! dialoghi, apertura dal sistema) come [`InputBatch`] già espansi, il motore è un gruppo di
//! Web Worker ([`WebEngine`]) e i percorsi sono relativi (nome del file o percorso dentro la
//! cartella trascinata).

mod anim;
mod dropzone;
mod export;
mod format;
mod help;
pub(crate) mod layout;
mod list;
mod settings;
mod single;
mod statusbar;
mod theme;
mod topbar;
mod verify;
mod widgets;

use crate::engine::{EngineConfig, EngineEvent, EngineHandle, EventQueue, FileId, FileResult, WebEngine, WebInput};
use crate::settings::{Language, Settings, ThemeMode};
use crate::web::picker::PickKind;
use crate::web::{self, Bridge, ChecksumBytes, InputBatch, WalkOptions};
use egui::{Context, Event, Key, Modifiers, Ui};
use hasher_core::algo::{normalize_hex, Algo};
use hasher_core::checksum_file::{self, ChecksumEntry};
use rust_i18n::t;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use widgets::{ToastKind, Verdict};

/// Valore segnaposto dei metadati non ancora letti (mostrato come scheletro animato).
pub(crate) const PENDING_VALUE: &str = "\u{2026}";

/// Codice lingua effettivo (per `Auto`, la lingua del browser).
pub(crate) fn resolve_language(lang: Language) -> &'static str {
    lang.resolve()
}

/// Applica la lingua all'intera interfaccia (e all'attributo `lang` della pagina).
pub(crate) fn apply_language(lang: Language) {
    let code = resolve_language(lang);
    rust_i18n::set_locale(code);
    if let Some(root) = web::document().and_then(|d| d.document_element()) {
        let _ = root.set_attribute("lang", code);
    }
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
    /// Percorso relativo del file trovato fra quelli disponibili (o quello atteso, se manca).
    pub resolved: PathBuf,
    pub file_id: Option<FileId>,
    pub status: CheckStatus,
}

#[derive(Debug, Clone)]
pub(crate) struct ChecksumSession {
    /// Percorso relativo del (primo) file di checksum.
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
    rx: EventQueue,
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
    /// Prompt di installazione della PWA.
    Install,
    Copy {
        text: String,
        toast: String,
    },
    CopyAll(FileId),
    RowClick {
        id: FileId,
        mods: Modifiers,
    },
    ToggleExpand(FileId),
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
}

pub struct HasherApp {
    pub(crate) settings: Settings,
    /// Algoritmi attivi (ordine di `Algo::ALL`).
    pub(crate) algos: Vec<Algo>,
    pub(crate) entries: Vec<FileEntry>,
    index: HashMap<FileId, usize>,
    session: Option<Session>,
    /// Motore: gruppo di Web Worker creato all'avvio.
    engine: WebEngine,
    /// Code condivise con i listener della pagina.
    pub(crate) bridge: Bridge,
    /// Input dell'ultima sessione (per ricalcolare).
    pub(crate) inputs: Vec<WebInput>,
    /// File disponibili per percorso relativo: quelli in elenco (ricalcolo, ripresa, confronto,
    /// risoluzione dei file di checksum).
    known: HashMap<PathBuf, WebInput>,
    /// Il browser consente l'installazione come app (aggiornato a ogni frame).
    pub(crate) can_install: bool,
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
    applied_theme: Option<(ThemeMode, bool, bool)>,
    /// Notifiche generate fuori dal frame (senza `Context`), mostrate come toast.
    notices: Vec<(ToastKind, String)>,
}

impl HasherApp {
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings, bridge: Bridge) -> Self {
        let ctx = &cc.egui_ctx;
        egui_extras::install_image_loaders(ctx);
        theme::install_fonts(ctx);
        apply_language(settings.language);

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
        let engine = WebEngine::new(ctx, settings.parallel_files);

        let mut app = HasherApp {
            hex_uppercase: settings.uppercase_hex,
            settings,
            algos,
            entries: Vec::new(),
            index: HashMap::new(),
            session: None,
            engine,
            bridge,
            inputs: Vec::new(),
            known: HashMap::new(),
            can_install: false,
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
            applied_theme: None,
            notices: Vec::new(),
        };
        app.apply_theme(ctx);
        // File aperti dal sistema ("Apri con Hasher") prima che l'app fosse pronta.
        web::launch::take_pending(&app.bridge);
        app
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
        self.settings.theme
    }

    fn effective_animations(&self) -> bool {
        self.ui
            .settings_draft
            .as_ref()
            .map_or(self.settings.animations, |d| d.animations)
    }

    fn apply_theme(&mut self, ctx: &Context) {
        let wanted = (
            self.effective_theme(),
            self.effective_animations(),
            layout::compact(ctx),
        );
        if self.applied_theme != Some(wanted) {
            theme::apply(ctx, wanted.0, wanted.1, wanted.2);
            self.applied_theme = Some(wanted);
        }
    }

    /// Salva le impostazioni nel browser; in caso di errore mostra un toast se richiesto.
    pub(crate) fn save_settings(&mut self, ctx: &Context, notify: bool) -> bool {
        match self.settings.save() {
            Ok(()) => true,
            Err(e) => {
                log::warn!("salvataggio impostazioni fallito: {e}");
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

    /// Apre un gruppo di file (trascinati, da dialogo o aperti dal sistema).
    ///
    /// Come sul desktop: un file di checksum avvia la verifica dei file elencati (sul web cercati
    /// fra quelli rilasciati insieme e quelli già in elenco); Shift confronta il file rilasciato
    /// con l'ultimo dell'elenco; Ctrl con due file li confronta.
    pub(crate) fn open_batch(&mut self, batch: InputBatch, current_mods: Modifiers) {
        let mods = batch.mods.unwrap_or(current_mods);
        let InputBatch {
            inputs,
            origin,
            had_dirs,
            checksums,
            ..
        } = batch;
        log::debug!(
            "{} file ({origin:?}), {} file di checksum, cartelle: {had_dirs}",
            inputs.len(),
            checksums.len()
        );
        if !checksums.is_empty() && self.load_checksum(checksums, inputs.clone()) {
            return;
        }
        if inputs.is_empty() {
            return;
        }
        if mods.shift && inputs.len() == 1 && !had_dirs {
            let previous = self.entries.last().and_then(|e| self.known.get(&e.path).cloned());
            if let Some(prev) = previous.filter(|p| p.rel_path != inputs[0].rel_path) {
                let mut pair = vec![prev];
                pair.extend(inputs);
                self.start_session(pair, StartMode::Compare, false);
                return;
            }
        }
        if (mods.ctrl || mods.command) && inputs.len() == 2 && !had_dirs {
            self.start_session(inputs, StartMode::Compare, false);
            return;
        }
        self.start_session(inputs, StartMode::Normal, had_dirs);
    }

    fn start_session(&mut self, inputs: Vec<WebInput>, mode: StartMode, dirs: bool) {
        self.start_session_ext(inputs, mode, false, dirs);
    }

    /// Avvia una sessione; con `keep` i file già in elenco restano (ripresa dei file annullati).
    /// `dirs`: gli input vengono da cartelle (vista elenco anche con un solo file).
    fn start_session_ext(&mut self, inputs: Vec<WebInput>, mode: StartMode, keep: bool, dirs: bool) {
        self.cancel_session();
        if keep {
            self.id_base = self.entries.iter().map(|e| e.id).max().unwrap_or(0);
            self.multi = true;
        } else {
            self.entries.clear();
            self.index.clear();
            self.known.clear();
            self.id_base = 0;
            self.ui.last_clicked = None;
            self.multi = inputs.len() > 1 || dirs;
            self.inputs = inputs.clone();
        }
        for i in &inputs {
            self.known.insert(i.path(), i.clone());
        }
        self.compare_on_discover = mode == StartMode::Compare;
        if mode != StartMode::Checksum {
            self.verify.checksum = None;
        } else if let Some(cs) = &mut self.verify.checksum {
            for r in &mut cs.rows {
                let kept = keep && r.file_id.is_some_and(|id| self.index.contains_key(&id));
                if !kept {
                    r.file_id = None;
                    r.status = if self.known.contains_key(&r.resolved) {
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
            include_hidden: self.settings.include_hidden,
        };
        self.fraction = 0.0;
        self.bytes_done = 0;
        self.bytes_total = inputs.iter().map(WebInput::size).sum();
        self.files_total = 0;
        self.files_done = 0;
        self.verdicts_dirty = true;
        match self.engine.start(inputs, config) {
            Ok((handle, rx)) => {
                self.session = Some(Session { handle, rx });
                self.status = RunStatus::Running;
                self.speed.reset();
            }
            Err(e) => {
                log::error!("avvio del motore fallito: {e}");
                self.status = RunStatus::Idle;
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
        self.known.clear();
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

    /// Cartella del file relativa alla cartella trascinata o aperta (vuota per i file singoli:
    /// sul web il percorso completo non è disponibile).
    fn relative_dir(path: &Path) -> String {
        path.parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
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
            {
                let mut rx = s.rx.borrow_mut();
                while events.len() < 50_000 {
                    match rx.pop_front() {
                        Some(ev) => events.push(ev),
                        None => break,
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
                entry.dir_text = Self::relative_dir(&path);
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
                self.verdicts_dirty = true;
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Verifica: hash incollato, file di checksum, confronto
    // -----------------------------------------------------------------------------------------

    /// Carica uno o più file di checksum e avvia la verifica dei file elencati, cercati fra
    /// quelli rilasciati insieme (`batch`) e quelli già in elenco: per percorso relativo, poi per
    /// percorso finale, infine per solo nome; gli altri risultano MANCANTI.
    ///
    /// Restituisce `false` se i file di checksum non sono utilizzabili (illeggibili o senza voci)
    /// ma insieme a loro sono arrivati altri file: in tal caso il chiamante li calcola normalmente.
    pub(crate) fn load_checksum(&mut self, checksums: Vec<ChecksumBytes>, batch: Vec<WebInput>) -> bool {
        let mut pool: Vec<WebInput> = batch
            .into_iter()
            .filter(|i| !checksums.iter().any(|c| c.rel_path == i.rel_path))
            .collect();
        let others_in_batch = !pool.is_empty();
        for e in &self.entries {
            if let Some(w) = self.known.get(&e.path) {
                if !pool.iter().any(|p| p.rel_path == w.rel_path) {
                    pool.push(w.clone());
                }
            }
        }

        let mut rows: Vec<ChecksumRow> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut first_path: Option<PathBuf> = None;
        for cs in &checksums {
            let bytes = match &cs.bytes {
                Ok(b) => b,
                Err(e) => {
                    self.notify(ToastKind::Error, t!("toast.checksum_error", error = e.as_str()));
                    continue;
                }
            };
            let path = PathBuf::from(&cs.rel_path);
            let loaded = checksum_file::parse_bytes(&path, bytes);
            names.push(
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| cs.rel_path.clone()),
            );
            first_path.get_or_insert(path);
            for entry in loaded.entries {
                let (resolved, found) = resolve_checksum_entry(&loaded.base_dir, &entry.file_name, &pool);
                rows.push(ChecksumRow {
                    entry,
                    resolved,
                    file_id: None,
                    status: if found {
                        CheckStatus::Pending
                    } else {
                        CheckStatus::Missing
                    },
                });
            }
        }
        let Some(path) = first_path else {
            return !others_in_batch;
        };
        if rows.is_empty() && others_in_batch {
            self.notify(ToastKind::Warning, t!("toast.checksum_empty"));
            return false;
        }

        let mut inputs: Vec<WebInput> = Vec::new();
        for r in rows.iter().filter(|r| r.status == CheckStatus::Pending) {
            if inputs.iter().any(|i| i.path() == r.resolved) {
                continue;
            }
            if let Some(w) = pool.iter().find(|w| w.path() == r.resolved) {
                inputs.push(w.clone());
            }
        }
        let total = rows.len();
        self.verify.checksum = Some(ChecksumSession {
            path,
            name: names.join(", "),
            rows,
        });
        self.verify_open = true;
        if total == 0 {
            self.notify(ToastKind::Warning, t!("toast.checksum_empty"));
        }
        if inputs.is_empty() {
            if let Some(s) = self.session.take() {
                s.handle.cancel();
            }
            self.entries.clear();
            self.index.clear();
            self.status = RunStatus::Idle;
        } else {
            self.start_session(inputs, StartMode::Checksum, true);
            self.multi = true;
        }
        self.verdicts_dirty = true;
        true
    }

    fn link_checksum(&mut self, id: FileId, path: &Path) {
        let Some(cs) = &mut self.verify.checksum else { return };
        for r in cs
            .rows
            .iter_mut()
            .filter(|r| r.file_id.is_none() && r.status == CheckStatus::Pending)
        {
            if r.resolved == path {
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
            // I file scelti arrivano più tardi come `InputBatch` (vedi `handle_input`).
            Action::OpenFiles => web::picker::open(&self.bridge, PickKind::Files),
            Action::OpenFolder => web::picker::open(&self.bridge, PickKind::Folder),
            Action::OpenChecksum => web::picker::open(&self.bridge, PickKind::Checksum),
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
                    self.start_session(self.inputs.clone(), mode, multi);
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
            Action::Install => web::install::install_app(ctx),
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
        let paths: Vec<WebInput> = self
            .entries
            .iter()
            .filter(|e| matches!(e.state, EntryState::Cancelled))
            .filter_map(|e| self.known.get(&e.path).cloned())
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
            self.start_session(inputs, mode, multi);
            self.multi = multi;
            return;
        }
        self.remove_entries(|e| matches!(e.state, EntryState::Cancelled));
        let mode = if self.verify.checksum.is_some() {
            StartMode::Checksum
        } else {
            StartMode::Normal
        };
        self.start_session_ext(paths, mode, true, true);
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

        // Pulsante "Incolla": testo letto con `navigator.clipboard.readText()`.
        let read = self.bridge.pasted.borrow_mut().take();
        match read {
            Some(Ok(s)) => {
                self.verify_open = true;
                self.verify.input = s.trim().to_owned();
                self.verify.paste_replace = false;
                self.verify.focus_request = true;
            }
            Some(Err(e)) => {
                log::warn!("lettura degli appunti non riuscita: {e}");
                self.verify.paste_replace = false;
                self.verify.focus_request = true;
                widgets::toast(ctx, ToastKind::Info, t!("toast.paste_unavailable"));
            }
            None => {}
        }

        // File rilasciati (trascinamento intercettato da `web::dnd`), scelti nei dialoghi o
        // aperti dal sistema.
        let mods = ctx.input(|i| i.modifiers);
        loop {
            let next = self.bridge.batches.borrow_mut().pop_front();
            let Some(batch) = next else { break };
            self.open_batch(batch, mods);
        }
        // Ripiego: file consegnati da eframe (browser senza `webkitGetAsEntry`).
        let dropped: Vec<WebInput> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.web_file().cloned())
                .map(|f| {
                    let name = f.name();
                    WebInput::new(f, name)
                })
                .collect()
        });
        if !dropped.is_empty() {
            let bridge = self.bridge.clone();
            wasm_bindgen_futures::spawn_local(async move {
                bridge.deliver(dropped, Some(mods), web::InputOrigin::Drop, false).await;
            });
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

    /// Esplorazione di cartelle trascinate ancora in corso.
    pub(crate) fn is_exploring(&self) -> bool {
        self.bridge.exploring.get() > 0
    }
}

/// Risolve una voce di un file di checksum fra i file disponibili: percorso relativo alla
/// cartella del file di checksum, poi stesso percorso finale (es. `sub/file` dentro
/// `cartella/sub/file`), infine solo il nome. Restituisce il percorso e se il file è stato trovato.
fn resolve_checksum_entry(base_dir: &Path, file_name: &str, pool: &[WebInput]) -> (PathBuf, bool) {
    let mut parts: Vec<String> = Vec::new();
    let base = base_dir.to_string_lossy();
    for c in base
        .split(['/', '\\'])
        .chain(file_name.split(['/', '\\']))
        .filter(|c| !c.is_empty() && *c != ".")
    {
        if c == ".." {
            parts.pop();
        } else {
            parts.push(c.to_owned());
        }
    }
    let key = parts.join("/");
    if let Some(w) = pool.iter().find(|w| w.rel_path == key) {
        return (w.path(), true);
    }
    let own: Vec<&str> = file_name
        .split(['/', '\\'])
        .filter(|c| !c.is_empty() && *c != "." && *c != "..")
        .collect();
    if own.len() > 1 {
        let tail = format!("/{}", own.join("/"));
        if let Some(w) = pool.iter().find(|w| w.rel_path.ends_with(&tail)) {
            return (w.path(), true);
        }
    }
    if let Some(name) = own.last() {
        if let Some(w) = pool
            .iter()
            .find(|w| w.path().file_name().is_some_and(|n| n.to_string_lossy() == *name))
        {
            return (w.path(), true);
        }
    }
    (PathBuf::from(key), false)
}

impl eframe::App for HasherApp {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if web::fix_canvas_resolution() {
            ctx.request_repaint();
        }
        self.poll_engine(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        anim::set_enabled(&ctx, self.effective_animations());
        self.apply_theme(&ctx);
        self.bridge.walk.set(WalkOptions {
            recurse: self.settings.recurse_folders,
            include_hidden: self.settings.include_hidden,
        });
        self.can_install = web::install::can_install();
        self.handle_input(&ctx);
        self.refresh_verdicts();
        for (kind, text) in std::mem::take(&mut self.notices) {
            widgets::toast(&ctx, kind, text);
        }

        // Sotto i 700 px (smartphone) le viste usano le varianti compatte; il pannello Verifica
        // non è laterale ma prende il posto della scheda o dell'elenco.
        let compact = layout::compact(&ctx);
        topbar::show(self, ui);
        statusbar::show(self, ui);
        if !compact {
            verify::panel(self, ui);
        }
        let p = theme::pal(ui);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::same(if compact { 12 } else { 16 })),
            )
            .show(ui, |ui| {
                if compact && self.verify_open {
                    verify::full(self, ui);
                } else if self.entries.is_empty() && !self.is_running() && !self.is_exploring() {
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
        dropzone::overlay(&ctx, self.bridge.drag_mods.get());
        widgets::show_toasts(&ctx, 56.0);

        self.process_actions(&ctx);
        self.refresh_verdicts();
        self.apply_theme(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(s) = self.session.take() {
            s.handle.cancel();
        }
    }
}
