//! Hasher Web — interfaccia egui in WebAssembly. Martini Multimedia s.a.s.
//!
//! Sul web `main` viene eseguito da `init()` del glue wasm-bindgen (`web/boot.js`): carica le
//! impostazioni da `localStorage`, avvia `eframe::WebRunner` sul canvas `#hasher_canvas` con
//! l'interfaccia del desktop (`ui`), installa i listener della pagina (`web`) e rimuove lo splash
//! `#splash`. Gli hash vengono calcolati dai Web Worker (`engine`, `hasher-core` in wasm): nessun
//! file lascia il browser.

#[cfg(target_arch = "wasm32")]
mod engine;
#[cfg(target_arch = "wasm32")]
mod settings;
#[cfg(target_arch = "wasm32")]
mod ui;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
pub use hasher_core::{APP_NAME, APP_VERSION, COMPANY, COMPANY_URL};

#[cfg(target_arch = "wasm32")]
rust_i18n::i18n!("locales", fallback = "en");

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast as _;

    console_error_panic_hook::set_once();
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(LOGGER.level.to_level_filter());
    }

    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            log::error!("document non disponibile");
            return;
        };
        let canvas = match document
            .get_element_by_id(web::CANVAS_ID)
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        {
            Some(c) => c,
            None => {
                log::error!("#{} non trovato", web::CANVAS_ID);
                return;
            }
        };

        let settings = settings::Settings::load();
        let walk = web::WalkOptions {
            recurse: settings.recurse_folders,
            include_hidden: settings.include_hidden,
        };
        let started = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(move |cc| {
                    // I listener della pagina vanno registrati qui, prima di quelli di eframe
                    // (il `drop` in capture sul canvas deve precedere il suo).
                    let bridge = web::Bridge::install(&cc.egui_ctx, walk)
                        .map_err(|e| format!("listener della pagina: {}", web::js_error_text(&e)))?;
                    Ok(Box::new(ui::HasherApp::new(cc, settings, bridge)))
                }),
            )
            .await;

        if let Some(splash) = document.get_element_by_id("splash") {
            match &started {
                Ok(()) => splash.remove(),
                Err(e) => {
                    splash.set_inner_html(&format!(
                        "<p class=\"error\">Impossibile avviare Hasher: {}</p>",
                        html_escape(&web::js_error_text(e))
                    ));
                }
            }
        }
        match started {
            // Senza focus il canvas non riceve tastiera né `paste` (Ctrl+V).
            Ok(()) => web::focus_canvas(),
            Err(e) => log::error!("avvio dell'app fallito: {}", web::js_error_text(&e)),
        }
    });
}

/// Logger della console del browser (`console_log`) che scarta un avviso innocuo di eframe: egui
/// invia `ViewportCommand::SetTheme` a ogni cambio di tema e il backend web non lo gestisce (il
/// tema dell'interfaccia cambia comunque).
#[cfg(target_arch = "wasm32")]
struct ConsoleLogger {
    level: log::Level,
}

#[cfg(target_arch = "wasm32")]
static LOGGER: ConsoleLogger = ConsoleLogger {
    level: if cfg!(debug_assertions) {
        log::Level::Info
    } else {
        log::Level::Warn
    },
};

#[cfg(target_arch = "wasm32")]
impl log::Log for ConsoleLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if record.target().starts_with("eframe") && record.args().to_string().contains("SetTheme") {
            return;
        }
        console_log::log(record);
    }

    fn flush(&self) {}
}

#[cfg(target_arch = "wasm32")]
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("hasher-web-app si compila solo per wasm32-unknown-unknown: usa scripts/build-web.sh");
    std::process::exit(2);
}
