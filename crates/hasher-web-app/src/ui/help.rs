//! Finestra di aiuto: guida, scorciatoie da tastiera, informazioni e licenze.

use super::theme::{self, pal};
use super::widgets;
use super::{HasherApp, HelpTab};
use egui::{
    Align, Align2, Color32, Context, Frame, Id, Label, Layout, Margin, RichText, ScrollArea, Sense, Stroke, Ui, Vec2,
};
use egui_phosphor::regular as icons;
use rust_i18n::t;

pub(crate) fn window(app: &mut HasherApp, ctx: &Context) {
    if !app.ui.help_open {
        return;
    }
    let mut open = true;
    let screen = ctx.content_rect();
    let height = (screen.height() - 120.0).clamp(320.0, 600.0);
    egui::Window::new(RichText::new(t!("help.title")).font(theme::font_semibold(17.0)))
        .id(Id::new("hasher.help.window"))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .fixed_size(Vec2::new(640.0, height))
        .open(&mut open)
        .show(ctx, |ui| {
            tabs(ui, &mut app.ui.help_tab);
            ui.add_space(12.0);
            ScrollArea::vertical()
                .id_salt("hasher.help.scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width() - 10.0);
                    match app.ui.help_tab {
                        HelpTab::Guide => guide(ui),
                        HelpTab::Shortcuts => shortcuts(ui),
                        HelpTab::About => about(ui),
                    }
                });
        });
    if !open {
        app.ui.help_open = false;
    }
}

/// Controllo segmentato per le schede.
fn tabs(ui: &mut Ui, tab: &mut HelpTab) {
    let p = pal(ui);
    let items = [
        (HelpTab::Guide, icons::BOOK_OPEN, t!("help.tab_guide")),
        (HelpTab::Shortcuts, icons::KEYBOARD, t!("help.tab_shortcuts")),
        (HelpTab::About, icons::INFO, t!("help.tab_about")),
    ];
    let total_w = ui.available_width();
    let (bar, _) = ui.allocate_exact_size(Vec2::new(total_w, 38.0), Sense::hover());
    ui.painter().rect_filled(bar, 10, p.elevated);
    let w = (bar.width() - 8.0) / items.len() as f32;
    for (i, (t, icon, label)) in items.into_iter().enumerate() {
        let r = egui::Rect::from_min_size(bar.min + Vec2::new(4.0 + i as f32 * w, 4.0), Vec2::new(w, 30.0));
        let resp = ui.interact(r, Id::new(("hasher.help.tab", i)), Sense::click());
        if resp.clicked() {
            *tab = t;
        }
        let selected = *tab == t;
        if selected {
            ui.painter().rect_filled(r, 8, p.surface);
            ui.painter()
                .rect_stroke(r, 8, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, 8, p.elevated_hover);
        }
        let color = if selected { p.primary } else { p.text_muted };
        let icon_g = ui
            .painter()
            .layout_no_wrap(icon.to_owned(), theme::font_icon(16.0), color);
        let text_g = ui
            .painter()
            .layout_no_wrap(label.into_owned(), theme::font_medium(13.5), color);
        let w_all = icon_g.size().x + 7.0 + text_g.size().x;
        let x = r.center().x - w_all / 2.0;
        ui.painter().galley(
            egui::pos2(x, r.center().y - icon_g.size().y / 2.0),
            icon_g.clone(),
            color,
        );
        let tx = x + icon_g.size().x + 7.0;
        ui.painter()
            .galley(egui::pos2(tx, r.center().y - text_g.size().y / 2.0), text_g, color);
    }
}

fn guide_item(ui: &mut Ui, icon: &str, title: &str, text: &str) {
    let p = pal(ui);
    ui.horizontal_top(|ui| {
        widgets::icon_tile(ui, icon, 36.0, p.primary, p.primary_soft());
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(RichText::new(title).font(theme::font_semibold(15.0)).color(p.text));
            ui.add(
                Label::new(
                    RichText::new(text)
                        .font(theme::font_regular(13.5))
                        .line_height(Some(19.5))
                        .color(p.text_muted),
                )
                .wrap(),
            );
        });
    });
    ui.add_space(14.0);
}

fn guide(ui: &mut Ui) {
    let sections = [
        (icons::UPLOAD_SIMPLE, "drop"),
        (icons::FILES, "views"),
        (icons::SHIELD_CHECK, "verify"),
        (icons::LIST_CHECKS, "checksum"),
        (icons::GIT_DIFF, "compare"),
        (icons::EXPORT, "export"),
        (icons::USB, "portable"),
    ];
    for (icon, key) in sections {
        guide_item(
            ui,
            icon,
            &t!(format!("help.guide_{key}_title")),
            &t!(format!("help.guide_{key}_text")),
        );
    }
}

fn shortcuts(ui: &mut Ui) {
    let p = pal(ui);
    let is_mac = ui.ctx().os() == egui::os::OperatingSystem::Mac;
    let cmd = if is_mac { "⌘" } else { "Ctrl" };
    let shift = "Shift";
    let del = t!("shortcuts.key_delete").into_owned();
    let click = t!("shortcuts.key_click").into_owned();
    let drop = t!("shortcuts.key_drop").into_owned();
    let rows: Vec<(Vec<&str>, String)> = vec![
        (vec![cmd, "O"], t!("shortcuts.open_files").into_owned()),
        (vec![cmd, shift, "O"], t!("shortcuts.open_folder").into_owned()),
        (vec![cmd, "E"], t!("shortcuts.export").into_owned()),
        (vec![cmd, "V"], t!("shortcuts.paste").into_owned()),
        (vec![cmd, "L"], t!("shortcuts.clear").into_owned()),
        (vec![cmd, ","], t!("shortcuts.settings").into_owned()),
        (vec!["F1"], t!("shortcuts.help").into_owned()),
        (vec!["Esc"], t!("shortcuts.escape").into_owned()),
        (vec![del.as_str()], t!("shortcuts.delete").into_owned()),
        (vec![cmd, click.as_str()], t!("shortcuts.ctrl_click").into_owned()),
        (vec![shift, click.as_str()], t!("shortcuts.shift_click").into_owned()),
        (vec![shift, drop.as_str()], t!("shortcuts.shift_drop").into_owned()),
        (vec![cmd, drop.as_str()], t!("shortcuts.ctrl_drop").into_owned()),
    ];
    Frame::new()
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(10)
        .inner_margin(Margin::symmetric(14, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (i, (keys, text)) in rows.iter().enumerate() {
                if i > 0 {
                    let r = ui.available_rect_before_wrap();
                    ui.painter()
                        .hline(r.x_range(), r.top(), Stroke::new(1.0, p.border.gamma_multiply(0.7)));
                    ui.add_space(1.0);
                }
                ui.horizontal(|ui| {
                    ui.set_min_height(34.0);
                    ui.allocate_ui_with_layout(Vec2::new(210.0, 30.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.set_min_width(210.0);
                        widgets::shortcut_keys(ui, keys);
                    });
                    ui.add(Label::new(RichText::new(text).font(theme::font_regular(13.5)).color(p.text)).wrap());
                });
            }
        });
}

fn about(ui: &mut Ui) {
    let p = pal(ui);
    ui.vertical_centered(|ui| {
        ui.add_space(6.0);
        if ui.visuals().dark_mode {
            // In tema scuro si usa la versione invertita del logo, che porta con sé il proprio sfondo.
            ui.add(
                egui::Image::new(egui::include_image!("../../assets/brand/logo-inverted.svg"))
                    .fit_to_exact_size(Vec2::new(300.0, 200.0))
                    .corner_radius(14),
            );
        } else {
            // Il wordmark navy resta leggibile su una tessera bianca.
            Frame::new()
                .fill(Color32::WHITE)
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(14)
                .inner_margin(Margin::symmetric(28, 20))
                .show(ui, |ui| {
                    ui.add(
                        egui::Image::new(egui::include_image!("../../assets/brand/logo.svg"))
                            .fit_to_exact_size(Vec2::new(240.0, 98.0)),
                    );
                });
        }
        ui.add_space(14.0);
        ui.label(RichText::new(t!("app.name")).font(theme::font_bold(24.0)).color(p.text));
        ui.label(
            RichText::new(t!("help.version", version = crate::APP_VERSION))
                .font(theme::font_regular(13.0))
                .color(p.text_muted),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(t!("app.tagline"))
                .font(theme::font_regular(14.0))
                .color(p.text),
        );
        ui.add_space(10.0);
        ui.label(
            RichText::new(t!("help.developed_by", company = crate::COMPANY))
                .font(theme::font_medium(13.5))
                .color(p.text),
        );
        ui.hyperlink_to(
            RichText::new(crate::COMPANY_URL).font(theme::font_regular(13.5)),
            crate::COMPANY_URL,
        );
        ui.add_space(18.0);
    });

    widgets::section_title(ui, icons::SCALES, t!("help.components"));
    ui.add_space(6.0);
    let components = [
        ("egui / eframe", "MIT / Apache-2.0"),
        ("RustCrypto (MD, SHA, SHA-3, RIPEMD, BLAKE2)", "MIT / Apache-2.0"),
        ("BLAKE3", "CC0-1.0 / Apache-2.0"),
        ("crc32fast, crc, xxhash-rust", "MIT / Apache-2.0"),
        ("rust_xlsxwriter", "MIT / Apache-2.0"),
        ("docx-rs", "MIT"),
        ("rfd, open, rust-i18n", "MIT"),
        ("Inter, JetBrains Mono", "SIL OFL 1.1"),
        ("Phosphor Icons", "MIT"),
    ];
    Frame::new()
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(10)
        .inner_margin(Margin::symmetric(14, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (i, (name, licence)) in components.iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(*name).font(theme::font_regular(13.5)).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        widgets::badge(ui, *licence, p.accent);
                    });
                });
            }
        });
    ui.add_space(10.0);
    widgets::caption(ui, t!("help.licence_note"));
}
