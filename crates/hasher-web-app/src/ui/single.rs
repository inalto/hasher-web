//! Vista file singolo: scheda con metadati, avanzamento e tutte le righe hash.

use super::format;
use super::theme::{self, pal};
use super::widgets::{self, Btn, HashState, InfoItem, ToastKind};
use super::{Action, EntryState, FileEntry, HasherApp};
use crate::backend::fileinfo::FileInfo;
use egui::{Align, Label, Layout, RichText, ScrollArea, Ui, Vec2};
use egui_phosphor::regular as icons;
use rust_i18n::t;

/// Colonna centrata di larghezza massima `max_w`.
pub(crate) fn centered_column<R>(ui: &mut Ui, max_w: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    // Spazio a destra per la barra di scorrimento flottante.
    let avail = ui.available_width() - 12.0;
    let w = avail.min(max_w);
    let pad = ((avail - w) / 2.0).max(0.0).floor();
    ui.horizontal_top(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_width(w);
            add(ui)
        })
        .inner
    })
    .inner
}

/// Voci della griglia informazioni per un file (con o senza metadati completi).
pub(crate) fn info_items(entry: &FileEntry) -> Vec<InfoItem> {
    let info: Option<&FileInfo> = entry.result().map(|r| &r.info);
    let pending = || super::PENDING_VALUE.to_owned();
    let mut items = vec![InfoItem {
        icon: icons::HARD_DRIVES,
        label: t!("single.size").into_owned(),
        value: format::size(entry.size),
        sub: Some(format::bytes_exact(entry.size)),
    }];
    items.push(InfoItem {
        icon: icons::CALENDAR_PLUS,
        label: t!("single.created").into_owned(),
        value: info.map_or_else(pending, |i| format::datetime(i.created)),
        sub: None,
    });
    items.push(InfoItem {
        icon: icons::CALENDAR_CHECK,
        label: t!("single.modified").into_owned(),
        value: info.map_or_else(pending, |i| format::datetime(i.modified)),
        sub: None,
    });
    let (value, sub) = match info {
        Some(i) => {
            let names: Vec<String> = i
                .attributes
                .iter()
                .map(|a| t!(format!("attr.{}", a.id())).into_owned())
                .collect();
            let value = if names.is_empty() {
                t!("single.no_attributes").into_owned()
            } else {
                names.join(", ")
            };
            let mut sub_parts = Vec::new();
            let short = i.attributes_short();
            if !short.is_empty() {
                sub_parts.push(short);
            }
            if let Some(m) = i.unix_mode_string() {
                sub_parts.push(m);
            }
            if let Some(m) = i.unix_mode {
                sub_parts.push(format!("{:04o}", m & 0o7777));
            }
            (
                value,
                if sub_parts.is_empty() {
                    None
                } else {
                    Some(sub_parts.join(" · "))
                },
            )
        }
        None => (pending(), None),
    };
    items.push(InfoItem {
        icon: icons::TAG,
        label: t!("single.attributes").into_owned(),
        value,
        sub,
    });
    items
}

/// Stato della riga hash per un algoritmo di un file.
pub(crate) fn hash_state<'a>(app: &HasherApp, entry: &'a FileEntry, algo: crate::backend::algo::Algo) -> HashState<'a> {
    match &entry.state {
        EntryState::Queued => HashState::Pending,
        EntryState::Computing { .. } => HashState::Computing,
        EntryState::Failed(_) => HashState::Unavailable,
        EntryState::Cancelled => HashState::Cancelled,
        EntryState::Done(_) => match entry.hex(algo) {
            Some(hex) => HashState::Done {
                hex,
                verdict: app.algo_verdict(entry, algo),
            },
            None => HashState::NotComputed,
        },
    }
}

/// Elenco delle righe hash di un file (usato anche dal dettaglio della vista elenco).
pub(crate) fn hash_rows(app: &mut HasherApp, ui: &mut Ui, index: usize) {
    let mut copied = None;
    {
        let entry = &app.entries[index];
        let p = pal(ui);
        for (i, algo) in app.shown_algos().iter().enumerate() {
            if i > 0 {
                let r = ui.available_rect_before_wrap();
                ui.painter().hline(
                    r.x_range().shrink(10.0),
                    r.top() - 1.0,
                    egui::Stroke::new(1.0, p.border.gamma_multiply(0.7)),
                );
            }
            let out = widgets::hash_row(ui, *algo, hash_state(app, entry, *algo));
            if out.copied {
                copied = Some(*algo);
            }
        }
    }
    if let Some(a) = copied {
        app.notify(ToastKind::Success, t!("toast.copied", algo = a.name()));
    }
}

pub(crate) fn show(app: &mut HasherApp, ui: &mut Ui) {
    ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        centered_column(ui, 1000.0, |ui| {
            ui.add_space(2.0);
            header_card(app, ui);
            ui.add_space(14.0);
            hashes_card(app, ui);
            ui.add_space(12.0);
        });
    });
}

fn header_card(app: &mut HasherApp, ui: &mut Ui) {
    let p = pal(ui);
    let entry = &app.entries[0];
    let id = entry.id;
    widgets::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            let icon = widgets::file_icon(entry.extension.as_deref());
            widgets::icon_tile(ui, icon, 52.0, p.primary, p.primary_soft());
            ui.add_space(6.0);
            let right_w = 170.0;
            let text_w = (ui.available_width() - right_w).max(120.0);
            ui.allocate_ui_with_layout(Vec2::new(text_w, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.add(
                    Label::new(
                        RichText::new(&entry.name)
                            .font(theme::font_semibold(21.0))
                            .color(p.text),
                    )
                    .wrap(),
                );
                ui.add(
                    Label::new(
                        RichText::new(entry.path.to_string_lossy())
                            .font(theme::font_regular(13.0))
                            .color(p.text_muted),
                    )
                    .wrap(),
                );
                ui.add_space(4.0);
                status_badge(entry, ui);
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                if let Some(parent) = entry.path.parent() {
                    if ui
                        .add(
                            Btn::secondary(t!("single.open_folder"))
                                .icon(icons::FOLDER_OPEN)
                                .small(),
                        )
                        .clicked()
                    {
                        app.actions.push(Action::OpenDir(parent.to_path_buf()));
                    }
                }
                if matches!(entry.state, EntryState::Cancelled)
                    && ui
                        .add(Btn::primary(t!("app.resume")).icon(icons::ARROW_CLOCKWISE).small())
                        .clicked()
                {
                    app.actions.push(Action::ResumeCancelled);
                }
            });
        });

        if let EntryState::Computing { bytes_done, started } = entry.state {
            ui.add_space(14.0);
            let frac = entry.fraction();
            let elapsed = (super::anim::now(ui.ctx()) - started).max(0.001);
            let speed = bytes_done as f64 / elapsed;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format::percent(frac))
                        .font(theme::font_semibold(13.0))
                        .color(p.primary),
                );
                ui.label(
                    RichText::new(t!(
                        "single.progress_detail",
                        done = format::size(bytes_done),
                        total = format::size(entry.size),
                        speed = format::speed(speed)
                    ))
                    .font(theme::font_regular(12.5))
                    .color(p.text_muted),
                );
            });
            ui.add_space(4.0);
            let w = ui.available_width();
            widgets::progress_bar(ui, Some(frac), w, 8.0, true);
        }
        if let EntryState::Failed(err) = &entry.state {
            ui.add_space(12.0);
            egui::Frame::new()
                .fill(p.error_soft())
                .corner_radius(theme::RADIUS_WIDGET)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(icons::WARNING_CIRCLE)
                                .font(theme::font_icon(18.0))
                                .color(p.error),
                        );
                        ui.add(Label::new(RichText::new(err).color(p.error)).wrap());
                    });
                });
        }

        ui.add_space(16.0);
        let items = info_items(entry);
        widgets::info_grid(ui, ("single.info", id), &items);
    });
}

fn status_badge(entry: &FileEntry, ui: &mut Ui) {
    let p = pal(ui);
    match &entry.state {
        EntryState::Queued => {
            widgets::icon_badge(ui, icons::HOURGLASS_MEDIUM, t!("list.state_queued"), p.text_muted);
        }
        EntryState::Computing { .. } => {
            widgets::icon_badge(ui, icons::CIRCLE_NOTCH, t!("list.state_computing"), p.primary);
        }
        EntryState::Done(r) => {
            let text = t!("single.done_in", time = format::duration(r.elapsed));
            widgets::icon_badge(ui, icons::CHECK_CIRCLE, text, p.success);
        }
        EntryState::Failed(_) => {
            widgets::icon_badge(ui, icons::WARNING_CIRCLE, t!("list.state_failed"), p.error);
        }
        EntryState::Cancelled => {
            widgets::icon_badge(ui, icons::STOP_CIRCLE, t!("list.state_cancelled"), p.text_muted);
        }
    }
}

fn hashes_card(app: &mut HasherApp, ui: &mut Ui) {
    let p = pal(ui);
    let done = app.entries[0].is_done();
    let id = app.entries[0].id;
    let n = app.shown_algos().len();
    widgets::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            widgets::section_title(ui, icons::FINGERPRINT, t!("single.hashes_title"));
            widgets::badge(ui, t!(format::plural_key("app.algos_count", n), n = n), p.accent);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(done, Btn::secondary(t!("single.export")).icon(icons::EXPORT).small())
                    .clicked()
                {
                    app.actions.push(Action::ShowExport);
                }
                if ui
                    .add_enabled(done, Btn::ghost(t!("single.copy_all")).icon(icons::COPY).small())
                    .clicked()
                {
                    app.actions.push(Action::CopyAll(id));
                }
            });
        });
        ui.add_space(10.0);
        hash_rows(app, ui, 0);
    });
}
