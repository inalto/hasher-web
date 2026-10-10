//! Widget riutilizzabili: riga hash, badge, card, griglia informazioni, pulsanti, barra di
//! avanzamento, interruttore, toast.

use super::anim;
use super::theme::{self, pal, Palette, RADIUS_CARD, RADIUS_WIDGET};
use crate::backend::algo::Algo;
use egui::text::LayoutJob;
use egui::{
    Align, Align2, Color32, Context, CornerRadius, Frame, Galley, Id, InnerResponse, Label, Layout, Margin, Order,
    Rect, Response, RichText, Sense, Shadow, Stroke, StrokeKind, Ui, Vec2, Widget, WidgetText,
};
use egui_phosphor::regular as icons;
use rust_i18n::t;
use std::sync::Arc;

// ---------------------------------------------------------------------------------------------
// Esito di verifica
// ---------------------------------------------------------------------------------------------

/// Esito del confronto di un hash con un valore atteso.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Verdict {
    #[default]
    None,
    Match,
    Mismatch,
}

/// Stato di una riga hash.
#[derive(Debug, Clone, Copy)]
pub enum HashState<'a> {
    /// In coda: scheletro con shimmer.
    Pending,
    /// In calcolo: spinner + shimmer.
    Computing,
    /// Calcolato.
    Done { hex: &'a str, verdict: Verdict },
    /// Algoritmo attivato dopo il calcolo: serve ricalcolare.
    NotComputed,
    /// Il file non è leggibile.
    Unavailable,
    /// Calcolo annullato.
    Cancelled,
}

/// Esito dell'interazione con una riga hash.
#[derive(Debug, Default, Clone, Copy)]
pub struct HashRowOutput {
    pub copied: bool,
}

/// Larghezza della colonna con il nome dell'algoritmo.
const ALGO_COL: f32 = 118.0;

/// Riga hash: nome algoritmo, valore esadecimale monospazio (selezionabile, a capo se serve),
/// badge di verifica e pulsante copia.
pub fn hash_row(ui: &mut Ui, algo: Algo, state: HashState<'_>) -> HashRowOutput {
    let p = pal(ui);
    let mut out = HashRowOutput::default();
    let full_w = ui.available_width().max(260.0);
    let pad_x = 10.0;
    let right_w = 76.0;
    let hex_w = (full_w - ALGO_COL - right_w - pad_x * 2.0).max(80.0);
    let mono = theme::font_mono(14.0);

    let galley: Option<Arc<Galley>> = match state {
        HashState::Done { hex, verdict } => {
            let color = match verdict {
                Verdict::Match => p.success,
                Verdict::Mismatch => p.error,
                Verdict::None => p.text,
            };
            Some(wrapped_galley(ui, hex, mono.clone(), color, hex_w))
        }
        _ => None,
    };
    let content_h = galley.as_ref().map_or(16.0, |g| g.size().y);
    let row_h = (content_h + 16.0).max(38.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(full_w, row_h), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return out;
    }

    let painter = ui.painter();
    let hovered = ui.rect_contains_pointer(rect);
    let row_bg = match state {
        HashState::Done {
            verdict: Verdict::Match,
            ..
        } => p.success_soft(),
        HashState::Done {
            verdict: Verdict::Mismatch,
            ..
        } => p.error_soft(),
        _ if hovered => p.elevated.gamma_multiply(0.6),
        _ => Color32::TRANSPARENT,
    };
    if row_bg != Color32::TRANSPARENT {
        painter.rect_filled(rect, RADIUS_WIDGET, row_bg);
    }

    // Nome dell'algoritmo.
    let name_color = match state {
        HashState::Done {
            verdict: Verdict::Match,
            ..
        } => p.success,
        HashState::Done {
            verdict: Verdict::Mismatch,
            ..
        } => p.error,
        _ => p.text_muted,
    };
    painter.text(
        egui::pos2(rect.left() + pad_x, rect.center().y),
        Align2::LEFT_CENTER,
        algo.name(),
        theme::font_semibold(13.0),
        name_color,
    );

    let hex_left = rect.left() + pad_x + ALGO_COL;
    let line_rect = Rect::from_min_size(egui::pos2(hex_left, rect.center().y - 7.0), Vec2::new(hex_w, 14.0));
    let t = anim::now(ui.ctx());
    match state {
        HashState::Pending => {
            let w = skeleton_width(algo, hex_w, &mono, ui);
            anim::shimmer_rect(ui, Rect::from_min_size(line_rect.min, Vec2::new(w, 12.0)), t);
        }
        HashState::Computing => {
            let spin = Rect::from_center_size(egui::pos2(hex_left + 8.0, rect.center().y), Vec2::splat(16.0));
            anim::paint_spinner(ui, spin, p.primary);
            let w = skeleton_width(algo, hex_w - 26.0, &mono, ui);
            let r = Rect::from_min_size(egui::pos2(hex_left + 26.0, rect.center().y - 6.0), Vec2::new(w, 12.0));
            anim::shimmer_rect(ui, r, t);
        }
        HashState::NotComputed => {
            painter.text(
                egui::pos2(hex_left, rect.center().y),
                Align2::LEFT_CENTER,
                t!("single.not_computed"),
                theme::font_regular(13.0),
                p.text_faint,
            );
        }
        HashState::Cancelled => {
            painter.text(
                egui::pos2(hex_left, rect.center().y),
                Align2::LEFT_CENTER,
                t!("single.cancelled_hash"),
                theme::font_regular(13.0),
                p.text_faint,
            );
        }
        HashState::Unavailable => {
            painter.text(
                egui::pos2(hex_left, rect.center().y),
                Align2::LEFT_CENTER,
                "—",
                theme::font_regular(14.0),
                p.text_faint,
            );
        }
        HashState::Done { hex, verdict } => {
            if let Some(g) = galley {
                let r = Rect::from_min_size(egui::pos2(hex_left, rect.center().y - g.size().y / 2.0), g.size());
                ui.put(r, Label::new(WidgetText::Galley(g)).selectable(true));
            }
            // Pulsante copia (a destra) e badge di verifica.
            let copy_rect = Rect::from_center_size(egui::pos2(rect.right() - 22.0, rect.center().y), Vec2::splat(30.0));
            let copy = ui.put(
                copy_rect,
                IconButton::new(icons::COPY).size(30.0).tooltip(t!("single.copy_hash")),
            );
            if copy.clicked() {
                ui.ctx().copy_text(hex.to_owned());
                out.copied = true;
            }
            if verdict != Verdict::None {
                let badge_c = egui::pos2(copy_rect.left() - 18.0, rect.center().y);
                paint_verdict_dot(ui, badge_c, verdict, 11.0);
            }
        }
    }
    let _ = response;
    out
}

/// Larghezza plausibile dello scheletro: quella che occuperà l'hash una volta calcolato.
fn skeleton_width(algo: Algo, max_w: f32, font: &egui::FontId, ui: &Ui) -> f32 {
    let char_w = ui.ctx().fonts_mut(|f| f.glyph_width(font, '0'));
    (algo.hex_len() as f32 * char_w).min(max_w).max(40.0)
}

/// Galley con a capo in qualsiasi punto (gli hash non contengono spazi).
pub fn wrapped_galley(ui: &Ui, text: &str, font: egui::FontId, color: Color32, width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple(text.to_owned(), font, color, width);
    job.wrap.break_anywhere = true;
    ui.painter().layout_job(job)
}

/// Galley su una sola riga, troncata con "…".
pub fn truncated_galley(ui: &Ui, text: &str, font: egui::FontId, color: Color32, width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple(text.to_owned(), font, color, width.max(10.0));
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    ui.painter().layout_job(job)
}

/// Disegna un cerchietto con ✓ o ✗.
pub fn paint_verdict_dot(ui: &Ui, center: egui::Pos2, verdict: Verdict, radius: f32) {
    let p = pal(ui);
    let (bg, fg, icon) = match verdict {
        Verdict::Match => (p.success, p.on_primary, icons::CHECK),
        Verdict::Mismatch => (p.error, p.on_primary, icons::X),
        Verdict::None => return,
    };
    ui.painter().circle_filled(center, radius, bg);
    ui.painter()
        .text(center, Align2::CENTER_CENTER, icon, theme::font_icon(radius * 1.3), fg);
}

// ---------------------------------------------------------------------------------------------
// Card, badge, titoli
// ---------------------------------------------------------------------------------------------

/// Frame standard delle card: superficie, bordo sottile e ombra morbida.
pub fn card_frame(ui: &Ui) -> Frame {
    let p = pal(ui);
    Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(RADIUS_CARD)
        .inner_margin(Margin::same(18))
        .shadow(Shadow {
            offset: [0, 2],
            blur: 12,
            spread: 0,
            color: p.shadow,
        })
}

/// Card con contenuto.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    card_frame(ui).show(ui, add)
}

/// Pillola colorata con testo.
pub fn badge(ui: &mut Ui, text: impl Into<String>, color: Color32) -> Response {
    badge_impl(ui, None, text.into(), color)
}

/// Badge con icona davanti al testo.
pub fn icon_badge(ui: &mut Ui, icon: &str, text: impl Into<String>, color: Color32) -> Response {
    badge_impl(ui, Some(icon), text.into(), color)
}

fn badge_impl(ui: &mut Ui, icon: Option<&str>, text: String, color: Color32) -> Response {
    let p = pal(ui);
    let galley = ui.painter().layout_no_wrap(text, theme::font_semibold(11.5), color);
    let icon_g = icon.map(|i| ui.painter().layout_no_wrap(i.to_owned(), theme::font_icon(13.0), color));
    let icon_w = icon_g.as_ref().map_or(0.0, |g| g.size().x + 5.0);
    let size = Vec2::new(galley.size().x + icon_w + 16.0, (galley.size().y + 6.0).max(20.0));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let fill = color.gamma_multiply(if p.dark { 0.20 } else { 0.12 });
        ui.painter()
            .rect_filled(rect, CornerRadius::same((rect.height() / 2.0) as u8), fill);
        let mut x = rect.left() + 8.0;
        if let Some(g) = icon_g {
            ui.painter()
                .galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g.clone(), color);
            x += g.size().x + 5.0;
        }
        ui.painter()
            .galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
    }
    resp
}

/// Titolo di sezione con icona.
pub fn section_title(ui: &mut Ui, icon: &str, text: impl Into<String>) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).font(theme::font_icon(18.0)).color(p.primary));
        ui.label(
            RichText::new(text.into())
                .font(theme::font_semibold(15.0))
                .color(p.text),
        );
    });
}

/// Etichetta tenue e piccola (didascalie).
pub fn caption(ui: &mut Ui, text: impl Into<String>) -> Response {
    let p = pal(ui);
    ui.label(
        RichText::new(text.into())
            .font(theme::font_regular(12.5))
            .color(p.text_muted),
    )
}

/// Tasto di una scorciatoia (stile "kbd").
pub fn kbd(ui: &mut Ui, text: &str) -> Response {
    let p = pal(ui);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), theme::font_mono_medium(12.0), p.text);
    let size = Vec2::new((galley.size().x + 12.0).max(24.0), 22.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect(
            rect,
            5,
            p.elevated,
            Stroke::new(1.0, p.border_strong),
            StrokeKind::Inside,
        );
        painter.hline(
            rect.x_range().shrink(3.0),
            rect.bottom() - 1.0,
            Stroke::new(1.0, p.border_strong),
        );
        painter.galley(rect.center() - galley.size() / 2.0, galley, p.text);
    }
    resp
}

/// Sequenza di tasti (`Ctrl` `+` `O`).
pub fn shortcut_keys(ui: &mut Ui, keys: &[&str]) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                ui.label(RichText::new("+").font(theme::font_regular(12.0)).color(p.text_faint));
            }
            kbd(ui, k);
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Griglia informazioni file
// ---------------------------------------------------------------------------------------------

/// Una voce della griglia informazioni.
pub struct InfoItem {
    pub icon: &'static str,
    pub label: String,
    pub value: String,
    pub sub: Option<String>,
}

/// Griglia di metadati in 2–4 colonne a seconda della larghezza.
pub fn info_grid(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, items: &[InfoItem]) {
    let p = pal(ui);
    let w = ui.available_width();
    let cols = if w > 820.0 {
        4
    } else if w > 420.0 {
        2
    } else {
        1
    }
    .min(items.len().max(1));
    let gap = 12.0;
    let col_w = ((w - gap * (cols as f32 - 1.0)) / cols as f32).floor();
    ui.push_id(id, |ui| {
        for chunk in items.chunks(cols) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for item in chunk {
                    ui.allocate_ui_with_layout(Vec2::new(col_w, 0.0), Layout::top_down(Align::Min), |ui| {
                        ui.set_width(col_w);
                        Frame::new()
                            .fill(p.elevated.gamma_multiply(0.55))
                            .corner_radius(RADIUS_WIDGET)
                            .inner_margin(Margin::symmetric(12, 10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.set_min_height(56.0);
                                ui.spacing_mut().item_spacing.y = 3.0;
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    ui.label(RichText::new(item.icon).font(theme::font_icon(14.0)).color(p.accent));
                                    ui.label(
                                        RichText::new(item.label.as_str())
                                            .font(theme::font_medium(12.0))
                                            .color(p.text_muted),
                                    );
                                });
                                if item.value == super::PENDING_VALUE {
                                    // Metadati non ancora disponibili: scheletro animato.
                                    let (r, _) = ui.allocate_exact_size(Vec2::new(120.0, 18.0), Sense::hover());
                                    let bar = Rect::from_min_size(r.min + Vec2::new(0.0, 4.0), Vec2::new(120.0, 11.0));
                                    anim::shimmer_rect(ui, bar, anim::now(ui.ctx()));
                                } else {
                                    ui.add(
                                        Label::new(
                                            RichText::new(item.value.as_str())
                                                .font(theme::font_medium(14.5))
                                                .color(p.text),
                                        )
                                        .wrap(),
                                    );
                                }
                                if let Some(sub) = &item.sub {
                                    ui.add(
                                        Label::new(
                                            RichText::new(sub.as_str())
                                                .font(theme::font_regular(12.0))
                                                .color(p.text_muted),
                                        )
                                        .wrap(),
                                    );
                                }
                            });
                    });
                }
            });
            ui.add_space(gap - ui.spacing().item_spacing.y);
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Pulsanti
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

/// Pulsante con icona opzionale, disegnato secondo la palette.
pub struct Btn {
    kind: ButtonKind,
    icon: Option<&'static str>,
    text: String,
    small: bool,
    min_width: f32,
}

impl Btn {
    pub fn new(kind: ButtonKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            icon: None,
            text: text.into(),
            small: false,
            min_width: 0.0,
        }
    }
    pub fn primary(text: impl Into<String>) -> Self {
        Self::new(ButtonKind::Primary, text)
    }
    pub fn secondary(text: impl Into<String>) -> Self {
        Self::new(ButtonKind::Secondary, text)
    }
    pub fn ghost(text: impl Into<String>) -> Self {
        Self::new(ButtonKind::Ghost, text)
    }
    pub fn danger(text: impl Into<String>) -> Self {
        Self::new(ButtonKind::Danger, text)
    }
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }
    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = w;
        self
    }
}

impl Widget for Btn {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = pal(ui);
        let (h, font_size, pad) = if self.small {
            (28.0, 13.0, 10.0)
        } else {
            (34.0, 14.0, 14.0)
        };
        let font = theme::font_medium(font_size);
        let text_galley = ui
            .painter()
            .layout_no_wrap(self.text.clone(), font, Color32::PLACEHOLDER);
        let icon_galley = self.icon.map(|i| {
            ui.painter()
                .layout_no_wrap(i.to_owned(), theme::font_icon(font_size + 3.0), Color32::PLACEHOLDER)
        });
        let gap = if icon_galley.is_some() && !self.text.is_empty() {
            7.0
        } else {
            0.0
        };
        let content_w = text_galley.size().x + icon_galley.as_ref().map_or(0.0, |g| g.size().x) + gap;
        let width = (content_w + pad * 2.0).max(self.min_width).max(h);
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, h), Sense::click());
        if ui.is_rect_visible(rect) {
            let hovered = response.hovered();
            let pressed = response.is_pointer_button_down_on();
            let (fill, fg, stroke) = match self.kind {
                ButtonKind::Primary => (
                    if pressed {
                        p.primary_fill
                    } else if hovered {
                        p.primary_fill_hover
                    } else {
                        p.primary_fill
                    },
                    p.on_primary,
                    Stroke::NONE,
                ),
                ButtonKind::Secondary => (
                    if pressed {
                        p.elevated_active
                    } else if hovered {
                        p.elevated_hover
                    } else {
                        p.surface
                    },
                    p.text,
                    Stroke::new(
                        1.0,
                        if hovered {
                            p.border_strong
                        } else {
                            p.border_strong.gamma_multiply(0.9)
                        },
                    ),
                ),
                ButtonKind::Ghost => (
                    if pressed {
                        p.elevated_active
                    } else if hovered {
                        p.elevated_hover
                    } else {
                        Color32::TRANSPARENT
                    },
                    p.text,
                    Stroke::NONE,
                ),
                ButtonKind::Danger => (
                    if hovered {
                        p.error.gamma_multiply(0.22)
                    } else {
                        p.error.gamma_multiply(0.12)
                    },
                    p.error,
                    Stroke::NONE,
                ),
            };
            let painter = ui.painter();
            painter.rect(rect, RADIUS_WIDGET, fill, stroke, StrokeKind::Inside);
            if response.has_focus() {
                painter.rect_stroke(
                    rect.expand(2.0),
                    RADIUS_WIDGET + 2,
                    Stroke::new(1.5, p.primary),
                    StrokeKind::Outside,
                );
            }
            let mut x = rect.center().x - content_w / 2.0;
            if let Some(g) = icon_galley {
                let pos = egui::pos2(x, rect.center().y - g.size().y / 2.0);
                x += g.size().x + gap;
                painter.galley(pos, g, fg);
            }
            let pos = egui::pos2(x, rect.center().y - text_galley.size().y / 2.0);
            painter.galley(pos, text_galley, fg);
        }
        let label = self.text;
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        response
    }
}

/// Pulsante quadrato con sola icona Phosphor.
pub struct IconButton {
    icon: &'static str,
    size: f32,
    tooltip: Option<String>,
    selected: bool,
}

impl IconButton {
    pub fn new(icon: &'static str) -> Self {
        Self {
            icon,
            size: 32.0,
            tooltip: None,
            selected: false,
        }
    }
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
    pub fn tooltip(mut self, tip: impl Into<String>) -> Self {
        self.tooltip = Some(tip.into());
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl Widget for IconButton {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = pal(ui);
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(self.size), Sense::click());
        if ui.is_rect_visible(rect) {
            let hovered = response.hovered();
            let pressed = response.is_pointer_button_down_on();
            let fill = if self.selected {
                p.primary_soft()
            } else if pressed {
                p.elevated_active
            } else if hovered {
                p.elevated_hover
            } else {
                Color32::TRANSPARENT
            };
            let fg = if self.selected {
                p.primary
            } else {
                if hovered {
                    p.text
                } else {
                    p.text_muted
                }
            };
            let painter = ui.painter();
            painter.rect_filled(rect, RADIUS_WIDGET, fill);
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                self.icon,
                theme::font_icon(self.size * 0.56),
                fg,
            );
        }
        let response = match self.tooltip {
            Some(tip) => response.on_hover_text(tip),
            None => response,
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, self.icon));
        response
    }
}

/// Pulsante quadrato con icona (forma breve).
pub fn icon_button(ui: &mut Ui, icon: &'static str, tooltip: impl Into<String>) -> Response {
    ui.add(IconButton::new(icon).tooltip(tooltip))
}

// ---------------------------------------------------------------------------------------------
// Interruttore e barra di avanzamento
// ---------------------------------------------------------------------------------------------

/// Interruttore on/off in stile moderno.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let p = pal(ui);
    let size = Vec2::new(38.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, ""));
    if ui.is_rect_visible(rect) {
        let how_on = if anim::enabled(ui.ctx()) {
            ui.ctx().animate_bool_with_time(response.id, *on, 0.15)
        } else if *on {
            1.0
        } else {
            0.0
        };
        let track = theme::mix(p.border_strong, p.primary_fill, how_on);
        let radius = rect.height() / 2.0;
        ui.painter().rect_filled(rect, CornerRadius::same(radius as u8), track);
        let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
        let c = egui::pos2(knob_x, rect.center().y);
        ui.painter()
            .circle_filled(c + Vec2::new(0.0, 1.0), radius - 3.0, p.shadow);
        ui.painter().circle_filled(c, radius - 3.0, Color32::WHITE);
        if response.hovered() {
            ui.painter().rect_stroke(
                rect.expand(1.5),
                CornerRadius::same(radius as u8 + 1),
                Stroke::new(1.0, p.primary.gamma_multiply(0.5)),
                StrokeKind::Outside,
            );
        }
    }
    response
}

/// Riga impostazione: titolo, descrizione e interruttore a destra.
pub fn toggle_row(ui: &mut Ui, title: &str, description: &str, value: &mut bool) -> Response {
    let p = pal(ui);
    let mut resp = None;
    ui.horizontal(|ui| {
        let w = ui.available_width();
        ui.allocate_ui_with_layout(Vec2::new(w - 54.0, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(title).font(theme::font_medium(14.0)).color(p.text));
            if !description.is_empty() {
                ui.add(
                    Label::new(
                        RichText::new(description)
                            .font(theme::font_regular(12.5))
                            .color(p.text_muted),
                    )
                    .wrap(),
                );
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            resp = Some(toggle(ui, value));
        });
    });
    resp.expect("toggle shown")
}

/// Barra di avanzamento con angoli arrotondati e riflesso animato.
/// `fraction = None` mostra una barra indeterminata.
pub fn progress_bar(ui: &mut Ui, fraction: Option<f32>, width: f32, height: f32, animate: bool) -> Response {
    let p = pal(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    paint_progress(ui, rect, fraction, animate, p);
    response
}

/// Disegna una barra di avanzamento in `rect`.
pub fn paint_progress(ui: &Ui, rect: Rect, fraction: Option<f32>, animate: bool, p: &Palette) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let radius = CornerRadius::same((rect.height() / 2.0) as u8);
    let painter = ui.painter();
    painter.rect_filled(rect, radius, p.skeleton);
    let animated = animate && anim::enabled(ui.ctx());
    let t = anim::now(ui.ctx());
    match fraction {
        Some(f) => {
            let f = f.clamp(0.0, 1.0);
            if f > 0.0 {
                let w = (rect.width() * f).max(rect.height());
                let fill_rect = Rect::from_min_size(rect.min, Vec2::new(w, rect.height()));
                painter.rect_filled(fill_rect, radius, p.primary);
                if animated {
                    anim::paint_sweep(
                        ui,
                        fill_rect,
                        t,
                        Color32::WHITE.gamma_multiply(0.35),
                        rect.height() / 2.0,
                    );
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
                }
            }
        }
        None => {
            // Segmento che scorre avanti e indietro.
            let seg = rect.width() * 0.3;
            let phase = if animated { ((t * 0.8) % 1.0) as f32 } else { 0.35 };
            let x = rect.left() - seg + phase * (rect.width() + seg);
            let seg_rect = Rect::from_min_max(
                egui::pos2(x.max(rect.left()), rect.top()),
                egui::pos2((x + seg).min(rect.right()), rect.bottom()),
            );
            if seg_rect.width() > 0.0 {
                painter.rect_filled(seg_rect, radius, p.primary);
            }
            if animated {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
            }
        }
    }
}

/// Icona Phosphor adatta al tipo di file (dall'estensione).
pub fn file_icon(extension: Option<&str>) -> &'static str {
    let ext = extension.unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "tif" | "tiff" | "ico" | "heic" | "avif" | "raw" => {
            icons::FILE_IMAGE
        }
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "opus" | "wma" => icons::FILE_AUDIO,
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "m4v" | "mpg" | "mpeg" => icons::FILE_VIDEO,
        "zip" | "7z" | "rar" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "iso" | "dmg" | "cab" => icons::FILE_ZIP,
        "pdf" => icons::FILE_PDF,
        "rs" | "py" | "js" | "ts" | "c" | "h" | "cpp" | "hpp" | "java" | "go" | "rb" | "sh" | "html" | "css"
        | "json" | "xml" | "yml" | "yaml" | "toml" | "php" | "kt" | "swift" | "cs" => icons::FILE_CODE,
        "txt" | "md" | "log" | "ini" | "cfg" | "conf" | "rtf" => icons::FILE_TEXT,
        "csv" => icons::FILE_CSV,
        "doc" | "docx" | "odt" => icons::FILE_DOC,
        "xls" | "xlsx" | "ods" => icons::FILE_XLS,
        "ttf" | "otf" | "woff" | "woff2" => icons::TEXT_AA,
        "exe" | "msi" | "appimage" | "deb" | "rpm" | "apk" | "app" | "bin" => icons::APP_WINDOW,
        "md5" | "sha1" | "sha256" | "sha512" | "sfv" | "b2" | "blake3" => icons::FINGERPRINT,
        _ => icons::FILE,
    }
}

/// Tessera arrotondata con un'icona (intestazioni).
pub fn icon_tile(ui: &mut Ui, icon: &str, size: f32, fg: Color32, bg: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same((size * 0.28) as u8), bg);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            icon,
            theme::font_icon(size * 0.55),
            fg,
        );
    }
    resp
}

// ---------------------------------------------------------------------------------------------
// Toast
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
struct Toast {
    kind: ToastKind,
    text: String,
    created: Option<f64>,
    duration: f32,
}

#[derive(Debug, Clone, Default)]
struct ToastQueue {
    items: Vec<Toast>,
}

fn toast_id() -> Id {
    Id::new("hasher.toasts")
}

const TOAST_LIFETIME: f32 = 2.5;
const TOAST_FADE: f32 = 0.25;

/// Accoda un toast (visibile in basso a destra, si chiude da solo dopo 2,5 s).
pub fn toast(ctx: &Context, kind: ToastKind, text: impl Into<String>) {
    let text = text.into();
    let duration = if matches!(kind, ToastKind::Error) {
        TOAST_LIFETIME * 2.0
    } else {
        TOAST_LIFETIME
    };
    ctx.data_mut(|d| {
        let q = d.get_temp_mut_or_default::<ToastQueue>(toast_id());
        // Evita duplicati identici consecutivi (es. doppio clic su "copia").
        q.items.retain(|t| t.text != text);
        q.items.push(Toast {
            kind,
            text,
            created: None,
            duration,
        });
        if q.items.len() > 5 {
            q.items.remove(0);
        }
    });
    ctx.request_repaint();
}

/// Disegna i toast in sovrimpressione. Da chiamare una volta per frame, dopo il resto dell'interfaccia.
pub fn show_toasts(ctx: &Context, bottom_offset: f32) {
    let now = ctx.input(|i| i.time);
    let mut queue = ctx.data(|d| d.get_temp::<ToastQueue>(toast_id())).unwrap_or_default();
    for t in &mut queue.items {
        if t.created.is_none() {
            t.created = Some(now);
        }
    }
    queue
        .items
        .retain(|t| (now - t.created.unwrap_or(now)) as f32 <= t.duration + TOAST_FADE);
    if queue.items.is_empty() {
        ctx.data_mut(|d| d.insert_temp(toast_id(), queue));
        return;
    }
    let animated = anim::enabled(ctx);
    egui::Area::new(Id::new("hasher.toasts.area"))
        .order(Order::Tooltip)
        .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-18.0, -bottom_offset))
        .interactable(false)
        .show(ctx, |ui| {
            ui.with_layout(Layout::bottom_up(Align::Max), |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                for t in queue.items.iter().rev() {
                    let age = (now - t.created.unwrap_or(now)) as f32;
                    let alpha = if !animated {
                        1.0
                    } else if age < TOAST_FADE {
                        age / TOAST_FADE
                    } else if age > t.duration {
                        1.0 - (age - t.duration) / TOAST_FADE
                    } else {
                        1.0
                    }
                    .clamp(0.0, 1.0);
                    toast_ui(ui, t, alpha);
                }
            });
        });
    ctx.data_mut(|d| d.insert_temp(toast_id(), queue));
    ctx.request_repaint_after(std::time::Duration::from_millis(if animated { 30 } else { 250 }));
}

fn toast_ui(ui: &mut Ui, t: &Toast, alpha: f32) {
    let p = pal(ui);
    let (icon, color) = match t.kind {
        ToastKind::Success => (icons::CHECK_CIRCLE, p.success),
        ToastKind::Info => (icons::INFO, p.primary),
        ToastKind::Warning => (icons::WARNING, p.warning),
        ToastKind::Error => (icons::WARNING_CIRCLE, p.error),
    };
    ui.scope(|ui| {
        ui.set_opacity(alpha);
        let offset = (1.0 - alpha) * 16.0;
        ui.add_space(0.0);
        Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border_strong))
            .corner_radius(10)
            .inner_margin(Margin {
                left: 12,
                right: 16,
                top: 10,
                bottom: 10,
            })
            .outer_margin(Margin {
                left: 0,
                right: offset as i8,
                top: 0,
                bottom: 0,
            })
            .shadow(Shadow {
                offset: [0, 6],
                blur: 18,
                spread: 0,
                color: p.shadow_strong,
            })
            .show(ui, |ui| {
                ui.set_max_width(380.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let (bar, _) = ui.allocate_exact_size(Vec2::new(3.0, 20.0), Sense::hover());
                    ui.painter().rect_filled(bar, 2, color);
                    ui.label(RichText::new(icon).font(theme::font_icon(18.0)).color(color));
                    ui.add(
                        Label::new(
                            RichText::new(t.text.as_str())
                                .font(theme::font_medium(13.5))
                                .color(p.text),
                        )
                        .wrap(),
                    );
                });
            });
    });
}

/// Freccia (chevron) sottile per i `ComboBox`, al posto del triangolo predefinito.
pub fn combo_caret(ui: &Ui, rect: Rect, _visuals: &egui::style::WidgetVisuals, open: bool) {
    let p = pal(ui);
    let c = rect.center();
    let w = rect.width().min(rect.height()).min(12.0) * 0.45;
    let h = w * 0.55;
    let (a, b, d) = if open {
        (
            egui::pos2(c.x - w, c.y + h / 2.0),
            egui::pos2(c.x, c.y - h / 2.0),
            egui::pos2(c.x + w, c.y + h / 2.0),
        )
    } else {
        (
            egui::pos2(c.x - w, c.y - h / 2.0),
            egui::pos2(c.x, c.y + h / 2.0),
            egui::pos2(c.x + w, c.y - h / 2.0),
        )
    };
    ui.painter()
        .add(egui::Shape::line(vec![a, b, d], Stroke::new(1.6, p.text_muted)));
}

/// Casella di spunta con il colore primario quando attiva.
pub fn check(ui: &mut Ui, checked: &mut bool, text: impl Into<String>) -> Response {
    let p = pal(ui);
    let galley = ui
        .painter()
        .layout_no_wrap(text.into(), theme::font_regular(14.0), p.text);
    let box_s = 18.0;
    let gap = 9.0;
    let size = Vec2::new(box_s + gap + galley.size().x, galley.size().y.max(26.0));
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    let label = galley.text().to_owned();
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *checked, &label));
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let b = Rect::from_min_size(
            egui::pos2(rect.left(), rect.center().y - box_s / 2.0),
            Vec2::splat(box_s),
        );
        let painter = ui.painter();
        if *checked {
            let fill = if hovered { p.primary_fill_hover } else { p.primary_fill };
            painter.rect_filled(b, 5, fill);
            let pt = |x: f32, y: f32| egui::pos2(b.left() + x * b.width(), b.top() + y * b.height());
            painter.add(egui::Shape::line(
                vec![pt(0.26, 0.52), pt(0.44, 0.70), pt(0.76, 0.33)],
                Stroke::new(2.0, p.on_primary),
            ));
        } else {
            let stroke = if hovered { p.primary } else { p.border_strong };
            painter.rect(b, 5, p.field, Stroke::new(1.3, stroke), StrokeKind::Inside);
        }
        painter.galley(
            egui::pos2(b.right() + gap, rect.center().y - galley.size().y / 2.0),
            galley,
            p.text,
        );
    }
    response
}

/// Casella di spunta per un algoritmo, con il badge "lento" per gli algoritmi che rallentano
/// l'intero calcolo (`Algo::is_slow`).
pub fn algo_check(ui: &mut Ui, algo: Algo, on: &mut bool) -> Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let resp = check(ui, on, algo.name());
        if algo.is_slow() {
            let p = pal(ui);
            icon_badge(ui, icons::HOURGLASS_MEDIUM, t!("app.slow"), p.warning).on_hover_text(t!("app.slow_tooltip"));
        }
        resp
    })
    .inner
}
