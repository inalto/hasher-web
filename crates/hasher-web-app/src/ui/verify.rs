//! Pannello laterale di verifica: hash incollato, confronto fra due file, file di checksum.

use super::anim;
use super::theme::{self, pal, RADIUS_WIDGET};
use super::widgets::{self, Btn, IconButton};
use super::{Action, CheckStatus, HashInput, HasherApp};
use crate::backend::algo::Algo;
use egui::{
    Align, Color32, Frame, Id, Label, Layout, Margin, RichText, ScrollArea, Stroke, TextEdit, Ui, Vec2, ViewportCommand,
};
use egui_phosphor::regular as icons;
use rust_i18n::t;

pub(crate) fn panel(app: &mut HasherApp, ui: &mut Ui) {
    let p = pal(ui);
    let mut open = app.verify_open;
    egui::Panel::right("hasher.verify")
        .resizable(true)
        .default_size(372.0)
        .size_range(320.0..=600.0)
        .frame(Frame::new().fill(p.surface).inner_margin(Margin {
            left: 18,
            right: 14,
            top: 16,
            bottom: 12,
        }))
        .show_collapsible(ui, &mut open, |ui| {
            ScrollArea::vertical()
                .id_salt("hasher.verify.scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width() - 4.0);
                    content(app, ui);
                });
        });
    app.verify_open = open;
}

fn result_card(ui: &mut Ui, color: Color32, soft: Color32, icon: &str, title: &str, add: impl FnOnce(&mut Ui)) {
    let p = pal(ui);
    Frame::new()
        .fill(soft)
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.45)))
        .corner_radius(10)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(30.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 15.0, color);
                ui.painter().text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    icon,
                    theme::font_icon(17.0),
                    p.on_primary,
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(RichText::new(title).font(theme::font_semibold(15.5)).color(color));
                    add(ui);
                });
            });
        });
}

fn content(app: &mut HasherApp, ui: &mut Ui) {
    let p = pal(ui);
    // Intestazione.
    ui.horizontal(|ui| {
        widgets::icon_tile(ui, icons::SHIELD_CHECK, 34.0, p.primary, p.primary_soft());
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(
                RichText::new(t!("verify.title"))
                    .font(theme::font_semibold(17.0))
                    .color(p.text),
            );
            ui.label(
                RichText::new(t!("verify.subtitle"))
                    .font(theme::font_regular(12.0))
                    .color(p.text_muted),
            );
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_button(ui, icons::X, t!("app.close")).clicked() {
                app.verify_open = false;
            }
        });
    });
    ui.add_space(14.0);

    // Hash atteso.
    ui.label(
        RichText::new(t!("verify.expected_label"))
            .font(theme::font_medium(13.0))
            .color(p.text_muted),
    );
    ui.add_space(2.0);
    let edit = TextEdit::multiline(&mut app.verify.input)
        .id(Id::new("hasher.verify.input"))
        .hint_text(RichText::new(t!("verify.placeholder")).color(p.text_faint))
        .font(theme::font_mono(13.0))
        .desired_rows(3)
        .desired_width(f32::INFINITY)
        .margin(Margin::symmetric(10, 8));
    let resp = ui.add(edit);
    if app.verify.focus_request {
        resp.request_focus();
        app.verify.focus_request = false;
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui
            .add(Btn::secondary(t!("verify.paste")).icon(icons::CLIPBOARD_TEXT).small())
            .clicked()
        {
            app.verify.paste_replace = true;
            ui.ctx().send_viewport_cmd(ViewportCommand::RequestPaste);
        }
        if !app.verify.input.is_empty() && ui.add(Btn::ghost(t!("verify.clear")).icon(icons::X).small()).clicked() {
            app.verify.input.clear();
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add(
                    IconButton::new(icons::LIST_CHECKS)
                        .size(28.0)
                        .tooltip(t!("menu.open_checksum")),
                )
                .clicked()
            {
                app.actions.push(Action::OpenChecksum);
            }
        });
    });
    ui.add_space(10.0);
    hash_result(app, ui);

    if app.compare_pair().is_some() {
        ui.add_space(18.0);
        compare_section(app, ui);
    }
    if app.verify.checksum.is_some() {
        ui.add_space(18.0);
        checksum_section(app, ui);
    }
    if app.verify.parsed == HashInput::Empty && app.compare_pair().is_none() && app.verify.checksum.is_none() {
        ui.add_space(18.0);
        tips(ui);
    }
}

fn hash_result(app: &HasherApp, ui: &mut Ui) {
    let p = pal(ui);
    match &app.verify.parsed {
        HashInput::Empty => {}
        HashInput::NotHex => {
            result_card(ui, p.warning, p.warning_soft(), "!", &t!("verify.not_hex"), |ui| {
                widgets::caption(ui, t!("verify.not_hex_hint"));
            });
        }
        HashInput::BadLength(n) => {
            result_card(ui, p.warning, p.warning_soft(), "!", &t!("verify.bad_length"), |ui| {
                widgets::caption(ui, t!("verify.bad_length_hint", n = n));
            });
        }
        HashInput::Valid { hex, candidates } => {
            // Algoritmi riconosciuti dalla lunghezza.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(5.0, 5.0);
                ui.label(
                    RichText::new(t!("verify.detected", n = hex.len()))
                        .font(theme::font_regular(12.5))
                        .color(p.text_muted),
                );
                for a in candidates {
                    widgets::badge(ui, a.name(), p.accent);
                }
            });
            ui.add_space(10.0);
            let matches: Vec<(&str, &[Algo])> = app
                .entries
                .iter()
                .filter(|e| !e.match_algos.is_empty())
                .map(|e| (e.name.as_str(), e.match_algos.as_slice()))
                .collect();
            let done = app.entries.iter().filter(|e| e.is_done()).count();
            if !matches.is_empty() {
                result_card(
                    ui,
                    p.success,
                    p.success_soft(),
                    icons::CHECK,
                    &t!("verify.match"),
                    |ui| {
                        for (name, algos) in matches.iter().take(6) {
                            let algos: Vec<&str> = algos.iter().map(|a| a.name()).collect();
                            ui.add(
                                Label::new(
                                    RichText::new(format!("{} — {}", algos.join(", "), name))
                                        .font(theme::font_medium(13.0))
                                        .color(p.text),
                                )
                                .wrap(),
                            );
                        }
                        if matches.len() > 6 {
                            widgets::caption(ui, t!("verify.and_more", n = matches.len() - 6));
                        }
                    },
                );
            } else if done > 0 {
                result_card(ui, p.error, p.error_soft(), icons::X, &t!("verify.no_match"), |ui| {
                    widgets::caption(
                        ui,
                        t!(super::format::plural_key("verify.no_match_hint", done), n = done),
                    );
                });
            } else if app.is_running() {
                Frame::new()
                    .fill(p.elevated.gamma_multiply(0.6))
                    .corner_radius(10)
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            anim::spinner_ring(ui, 18.0, p.primary);
                            ui.label(RichText::new(t!("verify.waiting")).color(p.text_muted));
                        });
                    });
            } else {
                widgets::caption(ui, t!("verify.drop_to_check"));
            }
        }
    }
}

fn compare_section(app: &HasherApp, ui: &mut Ui) {
    let p = pal(ui);
    let Some((a, b)) = app.compare_pair() else { return };
    widgets::section_title(ui, icons::GIT_DIFF, t!("compare.title"));
    ui.add_space(4.0);
    for (label, e) in [("A", a), ("B", b)] {
        ui.horizontal(|ui| {
            widgets::badge(ui, label, p.primary);
            ui.add(Label::new(RichText::new(&e.name).font(theme::font_medium(13.0)).color(p.text)).truncate())
                .on_hover_text(e.path.to_string_lossy());
        });
    }
    ui.add_space(8.0);
    let mut identical = 0;
    let mut different = 0;
    let mut pending = 0;
    let rows: Vec<(Algo, Option<bool>)> = app
        .shown_algos()
        .iter()
        .map(|algo| {
            let r = match (a.hex(*algo), b.hex(*algo)) {
                (Some(x), Some(y)) => Some(x == y),
                _ => None,
            };
            match r {
                Some(true) => identical += 1,
                Some(false) => different += 1,
                None => pending += 1,
            }
            (*algo, r)
        })
        .collect();

    if pending == 0 || a.is_done() && b.is_done() {
        if different == 0 && identical > 0 {
            result_card(
                ui,
                p.success,
                p.success_soft(),
                icons::EQUALS,
                &t!("compare.identical_title"),
                |ui| {
                    widgets::caption(
                        ui,
                        t!(
                            super::format::plural_key("compare.identical_hint", identical),
                            n = identical
                        ),
                    );
                },
            );
        } else if different > 0 {
            result_card(
                ui,
                p.error,
                p.error_soft(),
                icons::NOT_EQUALS,
                &t!("compare.different_title"),
                |ui| {
                    widgets::caption(
                        ui,
                        t!(
                            super::format::plural_key("compare.different_hint", different),
                            n = different
                        ),
                    );
                },
            );
        }
        ui.add_space(8.0);
    }

    Frame::new()
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(RADIUS_WIDGET)
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (i, (algo, r)) in rows.iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(algo.name())
                            .font(theme::font_semibold(12.5))
                            .color(p.text_muted),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| match r {
                        Some(true) => {
                            ui.label(
                                RichText::new(t!("compare.identical"))
                                    .font(theme::font_medium(12.5))
                                    .color(p.success),
                            );
                            ui.label(
                                RichText::new(icons::CHECK_CIRCLE)
                                    .font(theme::font_icon(16.0))
                                    .color(p.success),
                            );
                        }
                        Some(false) => {
                            ui.label(
                                RichText::new(t!("compare.different"))
                                    .font(theme::font_medium(12.5))
                                    .color(p.error),
                            );
                            ui.label(
                                RichText::new(icons::X_CIRCLE)
                                    .font(theme::font_icon(16.0))
                                    .color(p.error),
                            );
                        }
                        None => {
                            ui.label(
                                RichText::new(t!("compare.pending"))
                                    .font(theme::font_regular(12.5))
                                    .color(p.text_faint),
                            );
                            anim::spinner_ring(ui, 14.0, p.primary);
                        }
                    });
                });
            }
        });
}

fn checksum_section(app: &mut HasherApp, ui: &mut Ui) {
    let p = pal(ui);
    let Some(cs) = &app.verify.checksum else { return };
    let (ok, failed, missing, pending) = cs.counts();
    widgets::section_title(ui, icons::LIST_CHECKS, t!("verify.checksum_title"));
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(icons::FILE_TEXT)
                .font(theme::font_icon(14.0))
                .color(p.accent),
        );
        ui.add(Label::new(RichText::new(&cs.name).font(theme::font_medium(13.0)).color(p.text)).truncate())
            .on_hover_text(cs.path.to_string_lossy());
    });
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        widgets::icon_badge(ui, icons::CHECK, t!("verify.count_ok", n = ok), p.success);
        widgets::icon_badge(
            ui,
            icons::X,
            t!(super::format::plural_key("verify.count_failed", failed), n = failed),
            p.error,
        );
        widgets::icon_badge(
            ui,
            icons::QUESTION,
            t!(super::format::plural_key("verify.count_missing", missing), n = missing),
            p.warning,
        );
        if pending > 0 {
            widgets::icon_badge(
                ui,
                icons::HOURGLASS_MEDIUM,
                t!("verify.count_pending", n = pending),
                p.text_muted,
            );
        }
    });
    ui.add_space(8.0);
    if pending == 0 && !cs.rows.is_empty() {
        if failed == 0 && missing == 0 {
            result_card(
                ui,
                p.success,
                p.success_soft(),
                icons::CHECK,
                &t!("verify.checksum_all_ok"),
                |ui| {
                    widgets::caption(
                        ui,
                        t!(super::format::plural_key("verify.checksum_all_ok_hint", ok), n = ok),
                    );
                },
            );
        } else {
            result_card(
                ui,
                p.error,
                p.error_soft(),
                icons::X,
                &t!("verify.checksum_problems"),
                |ui| {
                    widgets::caption(
                        ui,
                        t!("verify.checksum_problems_hint", failed = failed, missing = missing),
                    );
                },
            );
        }
        ui.add_space(8.0);
    }

    let mut remove = false;
    Frame::new()
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(RADIUS_WIDGET)
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (i, r) in cs.rows.iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                let (icon, color, label) = match &r.status {
                    CheckStatus::Pending => (icons::HOURGLASS_MEDIUM, p.text_faint, t!("verify.status_pending")),
                    CheckStatus::Ok(_) => (icons::CHECK_CIRCLE, p.success, t!("verify.status_ok")),
                    CheckStatus::Failed => (icons::X_CIRCLE, p.error, t!("verify.status_failed")),
                    CheckStatus::Missing => (icons::QUESTION, p.warning, t!("verify.status_missing")),
                    CheckStatus::Error(_) => (icons::WARNING_CIRCLE, p.error, t!("verify.status_error")),
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon).font(theme::font_icon(16.0)).color(color));
                    let w = ui.available_width() - 90.0;
                    ui.allocate_ui_with_layout(Vec2::new(w.max(60.0), 0.0), Layout::top_down(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        let resp = ui.add(
                            Label::new(
                                RichText::new(&r.entry.file_name)
                                    .font(theme::font_medium(13.0))
                                    .color(p.text),
                            )
                            .truncate(),
                        );
                        if let CheckStatus::Error(e) = &r.status {
                            resp.on_hover_text(e);
                        }
                        let algo = match &r.status {
                            CheckStatus::Ok(a) => a.name().to_owned(),
                            _ => r
                                .entry
                                .candidate_algos()
                                .first()
                                .map(|a| a.name().to_owned())
                                .unwrap_or_default(),
                        };
                        ui.add(
                            Label::new(
                                RichText::new(format!("{algo} · {}", r.entry.hex))
                                    .font(theme::font_mono(11.0))
                                    .color(p.text_faint),
                            )
                            .truncate(),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        widgets::badge(ui, label, color);
                    });
                });
            }
        });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui
            .add(Btn::ghost(t!("verify.checksum_close")).icon(icons::X).small())
            .clicked()
        {
            remove = true;
        }
    });
    if remove {
        app.verify.checksum = None;
    }
}

fn tips(ui: &mut Ui) {
    let p = pal(ui);
    Frame::new()
        .fill(p.elevated.gamma_multiply(0.5))
        .corner_radius(10)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            for (icon, title, text) in [
                (
                    icons::CLIPBOARD_TEXT,
                    t!("verify.tip_paste_title"),
                    t!("verify.tip_paste"),
                ),
                (
                    icons::LIST_CHECKS,
                    t!("verify.tip_checksum_title"),
                    t!("verify.tip_checksum"),
                ),
                (
                    icons::GIT_DIFF,
                    t!("verify.tip_compare_title"),
                    t!("verify.tip_compare"),
                ),
            ] {
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new(icon).font(theme::font_icon(18.0)).color(p.primary));
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(RichText::new(title).font(theme::font_semibold(13.0)).color(p.text));
                        ui.add(
                            Label::new(RichText::new(text).font(theme::font_regular(12.5)).color(p.text_muted)).wrap(),
                        );
                    });
                });
            }
        });
}
