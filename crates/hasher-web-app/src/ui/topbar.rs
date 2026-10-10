//! Barra superiore: marchio, menu File/Strumenti/Aiuto, selettore algoritmi, tema, verifica,
//! impostazioni.

use super::theme::{self, pal};
use super::widgets::{self, IconButton, ToastKind};
use super::{Action, HasherApp, HelpTab};
use crate::backend::algo::{Algo, AlgoGroup};
use crate::backend::settings::ThemeMode;
use egui::containers::menu::{MenuButton, MenuConfig};
use egui::{
    Align, Button, Frame, Key, KeyboardShortcut, Layout, Margin, Modifiers, PopupCloseBehavior, RichText, Ui, Vec2,
};
use egui_phosphor::regular as icons;
use rust_i18n::t;

/// Gruppi di algoritmi, nell'ordine di presentazione, divisi in due colonne bilanciate.
pub(crate) const GROUP_COLUMNS: [&[AlgoGroup]; 2] = [
    &[AlgoGroup::Checksum, AlgoGroup::Md, AlgoGroup::Ripemd],
    &[AlgoGroup::Sha, AlgoGroup::Blake, AlgoGroup::P2p],
];

/// Chiave di traduzione del gruppo.
pub(crate) fn group_label(g: AlgoGroup) -> String {
    let id = match g {
        AlgoGroup::Checksum => "checksum",
        AlgoGroup::Md => "md",
        AlgoGroup::Sha => "sha",
        AlgoGroup::Ripemd => "ripemd",
        AlgoGroup::Blake => "blake",
        AlgoGroup::P2p => "p2p",
    };
    t!(format!("settings.groups.{id}")).into_owned()
}

pub(crate) fn algos_in(g: AlgoGroup) -> impl Iterator<Item = Algo> {
    Algo::ALL.into_iter().filter(move |a| a.group() == g)
}

fn shortcut(ctx: &egui::Context, mods: Modifiers, key: Key) -> String {
    ctx.format_shortcut(&KeyboardShortcut::new(mods, key))
}

fn menu_item(ui: &mut Ui, icon: &str, text: impl Into<String>, shortcut: Option<String>) -> bool {
    let p = pal(ui);
    let mut b = Button::new((
        RichText::new(icon).font(theme::font_icon(16.0)).color(p.accent),
        RichText::new(text.into()).font(theme::font_regular(14.0)),
    ));
    if let Some(s) = shortcut {
        b = b.shortcut_text(RichText::new(s).font(theme::font_regular(12.5)).color(p.text_muted));
    }
    ui.add(b.min_size(Vec2::new(250.0, 30.0))).clicked()
}

pub(crate) fn show(app: &mut HasherApp, ui: &mut Ui) {
    let p = pal(ui);
    egui::Panel::top("hasher.topbar")
        .exact_size(60.0)
        .frame(Frame::new().fill(p.surface).inner_margin(Margin::symmetric(16, 0)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                brand(ui);
                ui.add_space(18.0);
                let (sep, _) = ui.allocate_exact_size(Vec2::new(1.0, 26.0), egui::Sense::hover());
                ui.painter().rect_filled(sep, 0, p.border);
                ui.add_space(6.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    right_side(app, ui);
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        menus(app, ui);
                    });
                });
            });
        });
}

fn brand(ui: &mut Ui) {
    let p = pal(ui);
    let (tile, _) = ui.allocate_exact_size(Vec2::splat(38.0), egui::Sense::hover());
    ui.painter().rect(
        tile,
        10,
        p.brand_tile,
        egui::Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    let img =
        egui::Image::new(egui::include_image!("../../assets/brand/mark.svg")).fit_to_exact_size(Vec2::splat(26.0));
    img.paint_at(ui, egui::Rect::from_center_size(tile.center(), Vec2::splat(26.0)));
    ui.add_space(4.0);
    ui.vertical(|ui| {
        ui.add_space(10.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(RichText::new(t!("app.name")).font(theme::font_bold(17.0)).color(p.text));
        ui.label(
            RichText::new(t!("app.company_short"))
                .font(theme::font_regular(11.5))
                .color(p.text_muted),
        );
    });
}

fn menus(app: &mut HasherApp, ui: &mut Ui) {
    let running = app.is_running();
    let has_entries = !app.entries.is_empty();
    let any_done = app.any_done();
    let ctx = ui.ctx().clone();
    let sc = |mods: Modifiers, key: Key| Some(shortcut(&ctx, mods, key));
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button(RichText::new(t!("menu.file")).font(theme::font_medium(14.0)), |ui| {
            if menu_item(
                ui,
                icons::FILE_PLUS,
                t!("menu.open_files"),
                sc(Modifiers::COMMAND, Key::O),
            ) {
                app.actions.push(Action::OpenFiles);
            }
            if menu_item(
                ui,
                icons::FOLDER_OPEN,
                t!("menu.open_folder"),
                sc(Modifiers::COMMAND | Modifiers::SHIFT, Key::O),
            ) {
                app.actions.push(Action::OpenFolder);
            }
            if menu_item(ui, icons::LIST_CHECKS, t!("menu.open_checksum"), None) {
                app.actions.push(Action::OpenChecksum);
            }
            ui.separator();
            ui.add_enabled_ui(any_done, |ui| {
                if menu_item(ui, icons::EXPORT, t!("menu.export"), sc(Modifiers::COMMAND, Key::E)) {
                    app.actions.push(Action::ShowExport);
                }
            });
            ui.add_enabled_ui(has_entries, |ui| {
                if menu_item(ui, icons::BROOM, t!("menu.clear"), sc(Modifiers::COMMAND, Key::L)) {
                    app.actions.push(Action::ClearList);
                }
            });
            ui.separator();
            if menu_item(ui, icons::SIGN_OUT, t!("menu.quit"), None) {
                app.actions.push(Action::Quit);
            }
        });
        ui.menu_button(RichText::new(t!("menu.tools")).font(theme::font_medium(14.0)), |ui| {
            if menu_item(
                ui,
                icons::SHIELD_CHECK,
                t!("menu.verify"),
                sc(Modifiers::COMMAND, Key::V),
            ) {
                app.verify_open = true;
                app.verify.focus_request = true;
            }
            if menu_item(ui, icons::GIT_DIFF, t!("menu.compare"), None) {
                if app.compare_pair().is_some() {
                    app.verify_open = true;
                } else {
                    app.notify(ToastKind::Info, t!("toast.compare_hint"));
                }
            }
            ui.add_enabled_ui(running, |ui| {
                if menu_item(
                    ui,
                    icons::STOP_CIRCLE,
                    t!("menu.cancel"),
                    sc(Modifiers::NONE, Key::Escape),
                ) {
                    app.actions.push(Action::Cancel);
                }
            });
            ui.separator();
            if menu_item(
                ui,
                icons::GEAR_SIX,
                t!("menu.settings"),
                sc(Modifiers::COMMAND, Key::Comma),
            ) {
                app.actions.push(Action::OpenSettings);
            }
        });
        ui.menu_button(RichText::new(t!("menu.help")).font(theme::font_medium(14.0)), |ui| {
            if menu_item(ui, icons::BOOK_OPEN, t!("menu.guide"), sc(Modifiers::NONE, Key::F1)) {
                app.actions.push(Action::OpenHelp(HelpTab::Guide));
            }
            if menu_item(ui, icons::KEYBOARD, t!("menu.shortcuts"), None) {
                app.actions.push(Action::OpenHelp(HelpTab::Shortcuts));
            }
            ui.separator();
            if menu_item(ui, icons::INFO, t!("menu.about"), None) {
                app.actions.push(Action::OpenHelp(HelpTab::About));
            }
        });
    });
}

fn right_side(app: &mut HasherApp, ui: &mut Ui) {
    ui.spacing_mut().item_spacing.x = 6.0;
    if ui
        .add(IconButton::new(icons::GEAR_SIX).size(34.0).tooltip(t!("menu.settings")))
        .clicked()
    {
        app.actions.push(Action::OpenSettings);
    }
    if ui
        .add(
            IconButton::new(icons::SHIELD_CHECK)
                .size(34.0)
                .selected(app.verify_open)
                .tooltip(t!("verify.toggle_tooltip")),
        )
        .clicked()
    {
        app.actions.push(Action::ToggleVerify);
    }
    let (icon, label) = match app.effective_theme() {
        ThemeMode::System => (icons::MONITOR, t!("settings.theme_system")),
        ThemeMode::Light => (icons::SUN, t!("settings.theme_light")),
        ThemeMode::Dark => (icons::MOON, t!("settings.theme_dark")),
    };
    if ui
        .add(
            IconButton::new(icon)
                .size(34.0)
                .tooltip(t!("app.theme_tooltip", theme = label)),
        )
        .clicked()
    {
        app.actions.push(Action::CycleTheme);
    }
    ui.add_space(4.0);
    algo_selector(app, ui);
}

fn algo_selector(app: &mut HasherApp, ui: &mut Ui) {
    let p = pal(ui);
    let label = (
        RichText::new(icons::HASH).font(theme::font_icon(16.0)).color(p.primary),
        RichText::new(t!(
            super::format::plural_key("app.algos_count", app.algos.len()),
            n = app.algos.len()
        ))
        .font(theme::font_medium(13.5)),
        RichText::new(icons::CARET_DOWN)
            .font(theme::font_icon(12.0))
            .color(p.text_muted),
    );
    let button = Button::new(label).min_size(Vec2::new(0.0, 32.0));
    let config = MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside);
    let (resp, _) = MenuButton::from_button(button).config(config).ui(ui, |ui| {
        ui.set_min_width(380.0);
        ui.add_space(4.0);
        ui.label(
            RichText::new(t!("app.algos_title"))
                .font(theme::font_semibold(14.0))
                .color(p.text),
        );
        ui.label(
            RichText::new(t!("app.algos_hint"))
                .font(theme::font_regular(12.0))
                .color(p.text_muted),
        );
        ui.add_space(6.0);
        ui.columns(2, |cols| {
            for (ci, groups) in GROUP_COLUMNS.iter().enumerate() {
                let ui = &mut cols[ci];
                for g in groups.iter() {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(group_label(*g))
                            .font(theme::font_semibold(11.5))
                            .color(p.accent),
                    );
                    for a in algos_in(*g) {
                        let mut on = app.algos.contains(&a);
                        if widgets::algo_check(ui, a, &mut on).changed() {
                            app.actions.push(Action::ToggleAlgo(a));
                        }
                    }
                }
            }
        });
        ui.add_space(8.0);
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(t!("app.algos_reset")).clicked() {
                app.actions.push(Action::ResetAlgos);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let can = !app.inputs.is_empty() && !app.is_running();
                if ui
                    .add_enabled(
                        can,
                        Button::new((
                            RichText::new(icons::ARROW_CLOCKWISE).font(theme::font_icon(15.0)),
                            t!("app.recompute"),
                        )),
                    )
                    .clicked()
                {
                    app.actions.push(Action::Recompute);
                    ui.close();
                }
            });
        });
    });
    if app.ui.open_algo_menu {
        app.ui.open_algo_menu = false;
        egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&resp));
    }
    resp.on_hover_text(t!("app.algos_tooltip"));
}
