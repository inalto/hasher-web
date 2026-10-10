//! Hasher Web — interfaccia egui in WebAssembly. Martini Multimedia s.a.s.
//!
//! Sul web `main` viene eseguito da `init()` del glue wasm-bindgen (`web/boot.js`): avvia
//! `eframe::WebRunner` sul canvas `#hasher_canvas` e rimuove lo splash `#splash`.
//!
//! STATO: scheletro con un'app segnaposto. Il port dell'interfaccia del desktop
//! (`src/ui/`, copia verbatim di `hasher/src/ui`, e `src/settings.rs`) è la fase 1B di
//! docs/PIANO.md: i moduli vanno adattati e poi dichiarati qui (`mod ui; mod settings; …`).

#[cfg(target_arch = "wasm32")]
mod placeholder {
    /// App minima per verificare la pipeline (build, server, PWA) prima del port dell'interfaccia.
    pub struct Placeholder;

    impl eframe::App for Placeholder {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.heading(hasher_core::APP_NAME);
                    ui.label(format!("{} — interfaccia in costruzione (fase 1B)", hasher_core::COMPANY));
                    ui.label(format!("core {}", hasher_core::APP_VERSION));
                });
            });
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast as _;

    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Info);

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("no window")
            .document()
            .expect("no document");
        let canvas = document
            .get_element_by_id("hasher_canvas")
            .expect("#hasher_canvas not found")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("#hasher_canvas is not a canvas");

        let started = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|_cc| Ok(Box::new(placeholder::Placeholder))),
            )
            .await;

        if let Some(splash) = document.get_element_by_id("splash") {
            match &started {
                Ok(()) => splash.remove(),
                Err(e) => {
                    splash.set_inner_html(&format!(
                        "<p class=\"error\">Impossibile avviare Hasher: {}</p>",
                        html_escape(&format!("{e:?}"))
                    ));
                }
            }
        }
        if let Err(e) = started {
            log::error!("avvio dell'app fallito: {e:?}");
        }
    });
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
