//! Barra di stato: messaggio, avanzamento globale con velocità e tempo stimato, annulla, credito.

use super::anim;
use super::format;
use super::theme::{self, pal};
use super::widgets::{self, Btn};
use super::{Action, HasherApp, RunStatus};
use egui::{Align, Frame, Layout, Margin, RichText, Ui, Vec2};
use egui_phosphor::regular as icons;
use rust_i18n::t;

pub(crate) fn show(app: &mut HasherApp, ui: &mut Ui) {
    let p = pal(ui);
    egui::Panel::bottom("hasher.statusbar")
        .exact_size(40.0)
        .frame(Frame::new().fill(p.surface).inner_margin(Margin::symmetric(16, 0)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                status_text(app, ui);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.add(egui::Hyperlink::from_label_and_url(
                        RichText::new(t!("app.credit"))
                            .font(theme::font_regular(12.0))
                            .color(p.text_faint),
                        crate::COMPANY_URL,
                    ));
                    if !app.is_running()
                        && app.cancelled_count() > 0
                        && ui
                            .add(Btn::secondary(t!("app.resume")).icon(icons::ARROW_CLOCKWISE).small())
                            .on_hover_text(t!("app.resume_tooltip"))
                            .clicked()
                    {
                        app.actions.push(Action::ResumeCancelled);
                    }
                    if app.is_running() {
                        let (sep, _) = ui.allocate_exact_size(Vec2::new(1.0, 18.0), egui::Sense::hover());
                        ui.painter().rect_filled(sep, 0, p.border);
                        if ui
                            .add(Btn::danger(t!("status.cancel")).icon(icons::STOP_CIRCLE).small())
                            .clicked()
                        {
                            app.actions.push(Action::Cancel);
                        }
                        progress(app, ui);
                    }
                });
            });
        });
}

fn status_text(app: &HasherApp, ui: &mut Ui) {
    let p = pal(ui);
    let font = theme::font_medium(13.0);
    match &app.status {
        RunStatus::Running => {
            anim::spinner_ring(ui, 16.0, p.primary);
            let text = if app.files_total == 0 {
                t!("status.scanning").into_owned()
            } else {
                let done = anim::animated_number(ui.ctx(), egui::Id::new("hasher.status.done"), app.files_done as f64);
                let key = super::format::plural_key("status.computing", app.files_total as usize);
                t!(key, done = done.round() as u64, total = app.files_total).into_owned()
            };
            ui.label(RichText::new(text).font(font).color(p.text));
        }
        RunStatus::Done {
            ok,
            failed,
            cancelled,
            elapsed,
        } => {
            let (icon, color, text) = if *cancelled {
                (
                    icons::STOP_CIRCLE,
                    p.warning,
                    t!(format::plural_key("status.cancelled", *ok), n = ok).into_owned(),
                )
            } else if *failed > 0 {
                (
                    icons::WARNING,
                    p.warning,
                    t!(
                        format::plural_key("status.done_errors", *failed),
                        n = ok,
                        errors = failed
                    )
                    .into_owned(),
                )
            } else {
                (
                    icons::CHECK_CIRCLE,
                    p.success,
                    t!(
                        format::plural_key("status.done", *ok),
                        n = ok,
                        time = format::duration(*elapsed)
                    )
                    .into_owned(),
                )
            };
            ui.label(RichText::new(icon).font(theme::font_icon(16.0)).color(color));
            ui.label(RichText::new(text).font(font).color(p.text));
        }
        RunStatus::Idle => {
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.0, p.success);
            ui.painter()
                .circle_filled(dot.center(), 6.5, p.success.gamma_multiply(0.18));
            let text = if app.entries.is_empty() {
                t!("status.ready").into_owned()
            } else {
                t!(
                    format::plural_key("status.files", app.entries.len()),
                    n = app.entries.len()
                )
                .into_owned()
            };
            ui.label(RichText::new(text).font(font).color(p.text_muted));
        }
    }
}

/// Barra globale con velocità e tempo stimato (layout da destra a sinistra).
fn progress(app: &HasherApp, ui: &mut Ui) {
    let p = pal(ui);
    let speed = app.speed.bytes_per_sec;
    let remaining = app.bytes_total.saturating_sub(app.bytes_done) as f64;
    let eta = if speed > 0.0 {
        format::eta(remaining / speed)
    } else {
        "—".to_owned()
    };
    let small = theme::font_regular(12.5);
    ui.label(
        RichText::new(t!("status.eta", eta = eta))
            .font(small.clone())
            .color(p.text_muted),
    );
    ui.label(
        RichText::new(format::speed(speed))
            .font(theme::font_mono(12.5))
            .color(p.text),
    );
    ui.label(
        RichText::new(icons::LIGHTNING)
            .font(theme::font_icon(14.0))
            .color(p.accent),
    );
    ui.label(
        RichText::new(format::percent(app.fraction))
            .font(theme::font_semibold(12.5))
            .color(p.primary),
    );
    let w = (ui.available_width() * 0.45).clamp(80.0, 300.0);
    let fraction = if app.bytes_total == 0 && app.files_total == 0 {
        None
    } else {
        Some(app.fraction)
    };
    widgets::progress_bar(ui, fraction, w, 8.0, true);
}
