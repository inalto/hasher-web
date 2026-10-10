//! Modalità compatta (smartphone, tablet stretti, finestre piccole) e rilevamento del touch.
//!
//! Sopra [`COMPACT_WIDTH`] l'interfaccia è identica al desktop; sotto, le viste usano le varianti
//! compatte (barra superiore con menu ☰, pulsanti impilati, colonne ridotte, pannello Verifica a
//! tutta larghezza). Il valore è ricalcolato a ogni frame: ruotando il telefono cambia subito.

use egui::Context;

/// Larghezza (in punti egui = pixel CSS) sotto la quale l'interfaccia diventa compatta.
pub const COMPACT_WIDTH: f32 = 700.0;
/// Altezza sotto la quale finestre e popup scorrono invece di uscire dallo schermo.
pub const SHORT_HEIGHT: f32 = 600.0;

/// `true` se la pagina è più stretta di [`COMPACT_WIDTH`].
pub fn compact(ctx: &Context) -> bool {
    ctx.content_rect().width() < COMPACT_WIDTH
}

/// `true` se la pagina è bassa (telefono in orizzontale): i contenuti alti devono scorrere.
pub fn short(ctx: &Context) -> bool {
    ctx.content_rect().height() < SHORT_HEIGHT
}

/// `true` su dispositivi con schermo touch (`navigator.maxTouchPoints > 0`), letto una volta:
/// niente suggerimenti su trascinamento, Shift/Ctrl e scorciatoie da tastiera.
pub fn is_touch() -> bool {
    thread_local! {
        static TOUCH: bool = web_sys::window()
            .map(|w| w.navigator().max_touch_points() > 0)
            .unwrap_or(false);
    }
    TOUCH.with(|t| *t)
}

/// Testo tradotto nella variante per il touch (`<chiave>_touch`) quando lo schermo è touch:
/// niente riferimenti a trascinamento, Ctrl+clic o scorciatoie.
pub fn touch_text(key: &str) -> String {
    if is_touch() {
        rust_i18n::t!(format!("{key}_touch")).into_owned()
    } else {
        rust_i18n::t!(key).into_owned()
    }
}

/// Larghezza di finestre e popup: `min(desiderata, larghezza della pagina − 16)`.
pub fn window_width(ctx: &Context, wanted: f32) -> f32 {
    wanted.min(ctx.content_rect().width() - 16.0).max(200.0)
}

/// Altezza massima di finestre e popup (`altezza della pagina − 90`).
pub fn max_window_height(ctx: &Context) -> f32 {
    (ctx.content_rect().height() - 90.0).max(160.0)
}
