//! Finestra impostazioni: lingua, tema, algoritmi predefiniti, opzioni di calcolo, animazioni.

use super::theme::{self, pal};
use super::topbar::{algos_in, group_label, GROUP_COLUMNS};
use super::widgets::{self, Btn};
use super::{apply_language, HasherApp};
use crate::backend::algo::Algo;
use crate::backend::settings::{Language, Settings, ThemeMode};
use egui::{Align, Align2, Context, Frame, Id, Label, Layout, Margin, RichText, ScrollArea, Slider, Ui, Vec2};
use egui_phosphor::regular as icons;
use rust_i18n::t;

fn section(ui: &mut Ui, icon: &str, title: impl Into<String>, add: impl FnOnce(&mut Ui)) {
    let p = pal(ui);
    ui.add_space(6.0);
    widgets::section_title(ui, icon, title);
    ui.add_space(4.0);
    Frame::new()
        .fill(p.elevated.gamma_multiply(0.45))
        .corner_radius(10)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
    ui.add_space(8.0);
}

pub(crate) fn window(app: &mut HasherApp, ctx: &Context) {
    if !app.ui.settings_open {
        return;
    }
    if app.ui.settings_draft.is_none() {
        app.ui.settings_draft = Some(app.settings.clone());
    }
    let mut open = true;
    let mut save = false;
    let mut cancel = false;
    let screen = ctx.content_rect();
    let height = (screen.height() - 120.0).clamp(300.0, 640.0);
    let path = super::guard(Settings::path)
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let portable = super::guard(Settings::is_portable).unwrap_or(false);

    egui::Window::new(RichText::new(t!("settings.title")).font(theme::font_semibold(17.0)))
        .id(Id::new("hasher.settings.window"))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .fixed_size(Vec2::new(580.0, height))
        .open(&mut open)
        .show(ctx, |ui| {
            let p = pal(ui);
            let Some(draft) = app.ui.settings_draft.as_mut() else {
                return;
            };
            let footer_h = 92.0;
            ScrollArea::vertical()
                .id_salt("hasher.settings.scroll")
                .max_height(height - footer_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width() - 10.0);
                    body(ui, draft);
                });

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(icons::FLOPPY_DISK)
                        .font(theme::font_icon(14.0))
                        .color(p.text_muted),
                );
                ui.add(
                    Label::new(
                        RichText::new(path.as_str())
                            .font(theme::font_mono(11.0))
                            .color(p.text_muted),
                    )
                    .truncate(),
                )
                .on_hover_text(path.as_str());
            });
            ui.horizontal(|ui| {
                if portable {
                    widgets::icon_badge(ui, icons::USB, t!("settings.portable"), p.success);
                } else {
                    widgets::icon_badge(ui, icons::USER, t!("settings.user_profile"), p.accent);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(Btn::primary(t!("settings.save")).icon(icons::CHECK)).clicked() {
                        save = true;
                    }
                    if ui.add(Btn::ghost(t!("app.cancel"))).clicked() {
                        cancel = true;
                    }
                });
            });
        });

    if save {
        app.close_settings(ctx, true);
    } else if cancel || !open {
        app.close_settings(ctx, false);
    }
}

fn body(ui: &mut Ui, draft: &mut Settings) {
    let p = pal(ui);

    section(ui, icons::TRANSLATE, t!("settings.language"), |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!("settings.language_label")).color(p.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let current = draft.language;
                let label = |l: Language| {
                    if l == Language::Auto {
                        t!("settings.language_auto").into_owned()
                    } else {
                        l.native_name().to_owned()
                    }
                };
                egui::ComboBox::from_id_salt("hasher.settings.language")
                    .selected_text(label(current))
                    .width(200.0)
                    .icon(widgets::combo_caret)
                    .show_ui(ui, |ui| {
                        for l in Language::ALL {
                            if ui.selectable_value(&mut draft.language, l, label(l)).changed() {
                                apply_language(l);
                            }
                        }
                    });
            });
        });
    });

    section(ui, icons::PALETTE, t!("settings.theme"), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let w = ((ui.available_width() - 16.0) / 3.0).max(100.0);
            for (mode, icon, label) in [
                (ThemeMode::System, icons::MONITOR, t!("settings.theme_system")),
                (ThemeMode::Light, icons::SUN, t!("settings.theme_light")),
                (ThemeMode::Dark, icons::MOON, t!("settings.theme_dark")),
            ] {
                let selected = draft.theme == mode;
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 64.0), egui::Sense::click());
                if resp.clicked() {
                    draft.theme = mode;
                }
                let fill = if selected {
                    p.primary_soft()
                } else if resp.hovered() {
                    p.elevated_hover
                } else {
                    p.surface
                };
                let stroke = if selected {
                    egui::Stroke::new(1.5, p.primary)
                } else {
                    egui::Stroke::new(1.0, p.border_strong)
                };
                ui.painter().rect(rect, 10, fill, stroke, egui::StrokeKind::Inside);
                let fg = if selected { p.primary } else { p.text };
                ui.painter().text(
                    rect.center() - Vec2::new(0.0, 10.0),
                    Align2::CENTER_CENTER,
                    icon,
                    theme::font_icon(22.0),
                    fg,
                );
                ui.painter().text(
                    rect.center() + Vec2::new(0.0, 15.0),
                    Align2::CENTER_CENTER,
                    label,
                    theme::font_medium(13.0),
                    fg,
                );
            }
        });
    });

    section(ui, icons::HASH, t!("settings.default_algos"), |ui| {
        widgets::caption(ui, t!("settings.default_algos_hint"));
        ui.add_space(6.0);
        ui.columns(2, |cols| {
            for (ci, groups) in GROUP_COLUMNS.iter().enumerate() {
                let ui = &mut cols[ci];
                for g in groups.iter() {
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(group_label(*g))
                            .font(theme::font_semibold(11.5))
                            .color(p.accent),
                    );
                    for a in algos_in(*g) {
                        let mut on = draft.default_algos.contains(&a);
                        if widgets::algo_check(ui, a, &mut on).changed() {
                            if on {
                                draft.default_algos.push(a);
                            } else if draft.default_algos.len() > 1 {
                                draft.default_algos.retain(|x| *x != a);
                            }
                            draft.default_algos.sort();
                            draft.default_algos.dedup();
                        }
                    }
                }
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add(Btn::ghost(t!("settings.select_all")).small()).clicked() {
                draft.default_algos = Algo::ALL.to_vec();
            }
            if ui.add(Btn::ghost(t!("settings.select_none")).small()).clicked() {
                // Almeno un algoritmo deve restare attivo.
                draft.default_algos = vec![Algo::Sha256];
            }
            if ui.add(Btn::ghost(t!("settings.select_default")).small()).clicked() {
                draft.default_algos = Algo::DEFAULT.to_vec();
            }
        });
    });

    section(ui, icons::SLIDERS, t!("settings.behaviour"), |ui| {
        ui.spacing_mut().item_spacing.y = 12.0;
        widgets::toggle_row(
            ui,
            &t!("settings.uppercase"),
            &t!("settings.uppercase_hint"),
            &mut draft.uppercase_hex,
        );
        widgets::toggle_row(
            ui,
            &t!("settings.recurse"),
            &t!("settings.recurse_hint"),
            &mut draft.recurse_folders,
        );
        widgets::toggle_row(
            ui,
            &t!("settings.symlinks"),
            &t!("settings.symlinks_hint"),
            &mut draft.follow_symlinks,
        );
        widgets::toggle_row(
            ui,
            &t!("settings.hidden"),
            &t!("settings.hidden_hint"),
            &mut draft.include_hidden,
        );
        widgets::toggle_row(
            ui,
            &t!("settings.animations"),
            &t!("settings.animations_hint"),
            &mut draft.animations,
        );
    });

    section(ui, icons::CPU, t!("settings.performance"), |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(
                    RichText::new(t!("settings.parallel"))
                        .font(theme::font_medium(14.0))
                        .color(p.text),
                );
                widgets::caption(ui, t!("settings.parallel_hint"));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = if draft.parallel_files == 0 {
                    t!("settings.parallel_auto").into_owned()
                } else {
                    draft.parallel_files.to_string()
                };
                ui.label(RichText::new(label).font(theme::font_semibold(13.5)).color(p.primary));
                let mut v = draft.parallel_files.min(8) as u32;
                if ui.add(Slider::new(&mut v, 0..=8).show_value(false)).changed() {
                    draft.parallel_files = v as usize;
                }
            });
        });
    });
}
