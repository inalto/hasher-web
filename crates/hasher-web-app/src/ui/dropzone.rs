//! Stato vuoto (zona di rilascio) e sovrimpressione durante il trascinamento di file.

use super::anim;
use super::layout;
use super::theme::{self, pal, pal_ctx};
use super::widgets::{self, Btn};
use super::{Action, HasherApp};
use egui::{
    Align, Align2, Context, Id, Label, LayerId, Layout, Modifiers, Order, Pos2, Rect, RichText, Sense, Shape, Stroke,
    Ui, UiBuilder, Vec2,
};
use egui_phosphor::regular as icons;
use rust_i18n::t;
use std::f32::consts::{FRAC_PI_2, PI};

/// Percorso chiuso di un rettangolo arrotondato (per i bordi tratteggiati).
pub(crate) fn rounded_rect_path(rect: Rect, radius: f32) -> Vec<Pos2> {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let segs = 10;
    let corners = [
        (Pos2::new(rect.right() - r, rect.top() + r), -FRAC_PI_2),
        (Pos2::new(rect.right() - r, rect.bottom() - r), 0.0),
        (Pos2::new(rect.left() + r, rect.bottom() - r), FRAC_PI_2),
        (Pos2::new(rect.left() + r, rect.top() + r), PI),
    ];
    let mut pts = vec![Pos2::new(rect.left() + r, rect.top())];
    for (c, start) in corners {
        for i in 0..=segs {
            let a = start + FRAC_PI_2 * i as f32 / segs as f32;
            pts.push(c + r * Vec2::new(a.cos(), a.sin()));
        }
    }
    pts.push(Pos2::new(rect.left() + r, rect.top()));
    pts
}

/// Bordo tratteggiato arrotondato; `offset` fa "scorrere" i trattini.
pub(crate) fn dashed_rounded_rect(shapes: &mut Vec<Shape>, rect: Rect, radius: f32, stroke: Stroke, offset: f32) {
    let path = rounded_rect_path(rect, radius);
    shapes.extend(Shape::dashed_line_with_offset(&path, stroke, &[9.0], &[7.0], offset));
}

/// Altezza del contenuto della schermata iniziale nella versione desktop.
fn wide_content_height(install: bool, touch: bool) -> f32 {
    let mut h = 380.0;
    if install {
        h += 44.0;
    }
    if touch {
        // Senza i tre suggerimenti su Shift, Ctrl e Ctrl+V.
        h -= 104.0;
    }
    h
}

/// Schermata iniziale: invito a trascinare file e cartelle.
///
/// Sul desktop la zona è centrata e ha dimensioni fisse; in modalità compatta, o se lo spazio in
/// altezza non basta (telefono in orizzontale), il contenuto si impila e scorre.
pub(crate) fn empty_state(app: &mut HasherApp, ui: &mut Ui) {
    let compact = layout::compact(ui.ctx());
    let touch = layout::is_touch();
    let avail = ui.available_rect_before_wrap();
    let needed = wide_content_height(app.can_install, touch) + 24.0;
    if !compact && avail.height() >= needed {
        empty_state_wide(app, ui, touch);
    } else {
        empty_state_scroll(app, ui, compact, touch);
    }
}

fn empty_state_wide(app: &mut HasherApp, ui: &mut Ui, touch: bool) {
    let p = pal(ui);
    let avail = ui.available_rect_before_wrap();
    let zone = Rect::from_center_size(
        avail.center(),
        Vec2::new(avail.width().min(880.0), avail.height().min(560.0)),
    );
    let zone_resp = ui.interact(zone, Id::new("hasher.dropzone"), Sense::click());
    let hovered = zone_resp.hovered();
    let hover_t = anim::fade_in(ui.ctx(), Id::new("hasher.dropzone.hover"), hovered);

    let painter = ui.painter();
    painter.rect_filled(zone, 20, p.surface);
    painter.rect_filled(zone, 20, p.primary.gamma_multiply(0.04 * hover_t));
    let mut shapes = Vec::new();
    let border = theme::mix(p.border_strong, p.primary, 0.35 + 0.65 * hover_t);
    dashed_rounded_rect(&mut shapes, zone.shrink(1.0), 20.0, Stroke::new(2.0, border), 0.0);
    painter.extend(shapes);

    let install = app.can_install;
    let content_h = wide_content_height(install, touch);
    let top = zone.center().y - content_h / 2.0;
    let inner = Rect::from_min_max(
        Pos2::new(zone.left() + 24.0, top.max(zone.top() + 12.0)),
        Pos2::new(zone.right() - 24.0, zone.bottom()),
    );
    ui.scope_builder(
        UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Center)),
        |ui| {
            // Icona che pulsa dolcemente.
            let pulse = anim::pulse(ui.ctx(), Id::new("hasher.dropzone.pulse"), 2.6);
            let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(112.0), Sense::hover());
            let c = icon_rect.center();
            let painter = ui.painter();
            painter.circle_filled(
                c,
                46.0 + 8.0 * pulse,
                p.primary.gamma_multiply(0.06 + 0.04 * (1.0 - pulse)),
            );
            painter.circle_filled(c, 40.0, p.primary_soft());
            painter.text(
                c + Vec2::new(0.0, -2.0 * pulse),
                Align2::CENTER_CENTER,
                icons::UPLOAD_SIMPLE,
                theme::font_icon(40.0 + 3.0 * pulse),
                p.primary,
            );
            ui.add_space(14.0);
            ui.label(
                RichText::new(drop_title(touch))
                    .font(theme::font_semibold(24.0))
                    .color(p.text),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new(t!("drop.subtitle"))
                    .font(theme::font_regular(15.0))
                    .color(p.text_muted),
            );
            ui.add_space(20.0);

            // Pulsanti centrati.
            let bw = 190.0;
            let row_w = bw * 2.0 + 12.0;
            ui.allocate_ui_with_layout(Vec2::new(row_w, 36.0), Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if ui
                    .add(Btn::primary(t!("drop.open_files")).icon(icons::FILE_PLUS).min_width(bw))
                    .clicked()
                {
                    app.actions.push(Action::OpenFiles);
                }
                if ui
                    .add(
                        Btn::secondary(t!("drop.open_folder"))
                            .icon(icons::FOLDER_OPEN)
                            .min_width(bw),
                    )
                    .clicked()
                {
                    app.actions.push(Action::OpenFolder);
                }
            });
            ui.add_space(26.0);

            // Suggerimenti (non su touch: niente tastiera né trascinamento).
            let hints = if touch { Vec::new() } else { hints() };
            for (keys, text) in hints {
                let w = 470.0_f32.min(ui.available_width());
                ui.allocate_ui_with_layout(Vec2::new(w, 24.0), Layout::left_to_right(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let key_w: f32 = keys.len() as f32 * 44.0;
                    ui.allocate_ui_with_layout(
                        Vec2::new(key_w.max(80.0), 24.0),
                        Layout::right_to_left(Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for (i, k) in keys.iter().rev().enumerate() {
                                if i > 0 {
                                    ui.label(RichText::new("+").font(theme::font_regular(12.0)).color(p.text_faint));
                                }
                                widgets::kbd(ui, k);
                            }
                        },
                    );
                    ui.label(RichText::new(text).font(theme::font_regular(13.0)).color(p.text_muted));
                });
                ui.add_space(2.0);
            }

            // Installazione come app (solo se il browser la offre).
            if install {
                ui.add_space(14.0);
                if ui
                    .add(Btn::ghost(t!("drop.install")).icon(icons::DOWNLOAD_SIMPLE).small())
                    .on_hover_text(t!("drop.install_tooltip"))
                    .clicked()
                {
                    app.actions.push(Action::Install);
                }
            }
        },
    );

    if zone_resp.clicked() {
        app.actions.push(Action::OpenFiles);
    }
    let _ = zone_resp.on_hover_cursor(egui::CursorIcon::PointingHand);
}

/// Titolo della schermata iniziale: su touch non si trascina, si tocca "Apri file".
fn drop_title(touch: bool) -> String {
    if touch {
        t!("drop.title_touch").into_owned()
    } else {
        t!("drop.title").into_owned()
    }
}

/// Suggerimenti da tastiera della schermata iniziale.
fn hints() -> Vec<(&'static [&'static str], String)> {
    vec![
        (&["Shift"], t!("drop.hint_shift").into_owned()),
        (&["Ctrl"], t!("drop.hint_ctrl").into_owned()),
        (&["Ctrl", "V"], t!("drop.hint_paste").into_owned()),
    ]
}

/// Schermata iniziale compatta (o in poco spazio verticale): icona 64 px, testi che vanno a capo,
/// pulsanti impilati a tutta larghezza; la zona tratteggiata avvolge il contenuto e tutto scorre.
fn empty_state_scroll(app: &mut HasherApp, ui: &mut Ui, compact: bool, touch: bool) {
    let p = pal(ui);
    let ctx = ui.ctx().clone();
    let measured = Id::new("hasher.dropzone.content_h");
    let prev_h: f32 = ctx.data(|d| d.get_temp(measured)).unwrap_or(0.0);
    let view_h = ui.available_height();
    egui::ScrollArea::vertical()
        .id_salt("hasher.dropzone.scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let left = ui.max_rect().left();
            let width = ui.available_width();
            let top = ui.cursor().top();
            let pad = if compact { 16.0 } else { 24.0 };
            let pad_top = ((view_h - prev_h) / 2.0).max(pad);
            // Sfondo e bordo vanno sotto il contenuto, ma la loro altezza si conosce solo dopo.
            let bg = ui.painter().add(Shape::Noop);
            let border = ui.painter().add(Shape::Noop);
            let inner = Rect::from_min_size(
                Pos2::new(left + pad, top + pad_top),
                Vec2::new(width - pad * 2.0, 10_000.0),
            );
            let used = ui
                .scope_builder(
                    UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Center)),
                    |ui| scroll_content(app, ui, compact, touch),
                )
                .response
                .rect;
            let content_h = used.height();
            if (content_h - prev_h).abs() > 0.5 {
                ctx.data_mut(|d| d.insert_temp(measured, content_h));
                ctx.request_repaint();
            }
            let zone_h = (pad_top + content_h + pad).max(view_h - 1.0);
            let zone = Rect::from_min_size(Pos2::new(left, top), Vec2::new(width, zone_h));
            if zone.bottom() > used.bottom() {
                ui.allocate_space(Vec2::new(
                    width,
                    zone.bottom() - used.bottom() - ui.spacing().item_spacing.y,
                ));
            }
            ui.painter()
                .set(bg, Shape::rect_filled(zone, if compact { 16 } else { 20 }, p.surface));
            let mut shapes = Vec::new();
            let stroke = Stroke::new(2.0, theme::mix(p.border_strong, p.primary, 0.35));
            dashed_rounded_rect(
                &mut shapes,
                zone.shrink(1.0),
                if compact { 16.0 } else { 20.0 },
                stroke,
                0.0,
            );
            ui.painter().set(border, Shape::Vec(shapes));
        });
}

fn scroll_content(app: &mut HasherApp, ui: &mut Ui, compact: bool, touch: bool) {
    let p = pal(ui);
    // Icona 64 px che pulsa dolcemente.
    let pulse = anim::pulse(ui.ctx(), Id::new("hasher.dropzone.pulse"), 2.6);
    let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(72.0), Sense::hover());
    let c = icon_rect.center();
    let painter = ui.painter();
    painter.circle_filled(
        c,
        28.0 + 5.0 * pulse,
        p.primary.gamma_multiply(0.06 + 0.04 * (1.0 - pulse)),
    );
    painter.circle_filled(c, 24.0, p.primary_soft());
    painter.text(
        c + Vec2::new(0.0, -pulse),
        Align2::CENTER_CENTER,
        icons::UPLOAD_SIMPLE,
        theme::font_icon(24.0 + 2.0 * pulse),
        p.primary,
    );
    ui.add_space(10.0);
    ui.add(
        Label::new(
            RichText::new(drop_title(touch))
                .font(theme::font_semibold(if compact { 22.0 } else { 24.0 }))
                .color(p.text),
        )
        .wrap(),
    );
    ui.add_space(2.0);
    ui.add(
        Label::new(
            RichText::new(t!("drop.subtitle"))
                .font(theme::font_regular(14.0))
                .color(p.text_muted),
        )
        .wrap(),
    );
    ui.add_space(18.0);

    // Pulsanti: impilati a tutta larghezza se lo spazio è poco, affiancati altrimenti.
    let open_files = Btn::primary(t!("drop.open_files")).icon(icons::FILE_PLUS);
    let open_folder = Btn::secondary(t!("drop.open_folder")).icon(icons::FOLDER_OPEN);
    let bw = 190.0;
    if ui.available_width() >= bw * 2.0 + 12.0 {
        ui.allocate_ui_with_layout(
            Vec2::new(bw * 2.0 + 12.0, 36.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if ui.add(open_files.min_width(bw)).clicked() {
                    app.actions.push(Action::OpenFiles);
                }
                if ui.add(open_folder.min_width(bw)).clicked() {
                    app.actions.push(Action::OpenFolder);
                }
            },
        );
    } else {
        let w = ui.available_width().min(420.0);
        if ui.add(open_files.min_width(w)).clicked() {
            app.actions.push(Action::OpenFiles);
        }
        ui.add_space(4.0);
        if ui.add(open_folder.min_width(w)).clicked() {
            app.actions.push(Action::OpenFolder);
        }
    }

    // Suggerimenti da tastiera (non su touch), su più righe se serve.
    if !touch {
        ui.add_space(16.0);
        for (keys, text) in hints() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (i, k) in keys.iter().enumerate() {
                    if i > 0 {
                        ui.label(RichText::new("+").font(theme::font_regular(12.0)).color(p.text_faint));
                    }
                    widgets::kbd(ui, k);
                }
                ui.add_space(4.0);
                ui.label(RichText::new(text).font(theme::font_regular(13.0)).color(p.text_muted));
            });
        }
    }

    // Installazione come app (solo se il browser la offre).
    if app.can_install {
        ui.add_space(14.0);
        if ui
            .add(Btn::ghost(t!("drop.install")).icon(icons::DOWNLOAD_SIMPLE).small())
            .on_hover_text(t!("drop.install_tooltip"))
            .clicked()
        {
            app.actions.push(Action::Install);
        }
    }
}

/// Sovrimpressione a tutta finestra mentre l'utente trascina file sopra l'app.
/// `mods`: modificatori letti dal `dragover` (durante il trascinamento il browser non invia
/// eventi di tastiera a egui).
pub(crate) fn overlay(ctx: &Context, mods: Modifiers) {
    let count = ctx.input(|i| i.raw.hovered_files.len());
    let t = anim::fade_in(ctx, Id::new("hasher.drop.overlay"), count > 0);
    if t <= 0.001 {
        return;
    }
    let p = pal_ctx(ctx);
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("hasher.drop.overlay.layer")));
    painter.rect_filled(screen, 0, p.bg.gamma_multiply(0.97 * t));
    let inner = screen.shrink(18.0);
    painter.rect_filled(inner, 18, p.primary.gamma_multiply(0.10 * t));

    let pulse = anim::pulse(ctx, Id::new("hasher.drop.overlay.pulse"), 1.4);
    let offset = if anim::enabled(ctx) {
        (anim::now(ctx) * 24.0) as f32 % 16.0
    } else {
        0.0
    };
    let mut shapes = Vec::new();
    dashed_rounded_rect(
        &mut shapes,
        inner,
        18.0,
        Stroke::new(2.5, p.primary.gamma_multiply(t * (0.75 + 0.25 * pulse))),
        offset,
    );
    painter.extend(shapes);

    let c = inner.center() - Vec2::new(0.0, 30.0);
    painter.circle_filled(c, (54.0 + 6.0 * pulse) * t, p.primary.gamma_multiply(0.18 * t));
    painter.circle_filled(c, 44.0 * t, p.primary_fill.gamma_multiply(t));
    painter.text(
        c,
        Align2::CENTER_CENTER,
        icons::DOWNLOAD_SIMPLE,
        theme::font_icon(40.0),
        p.on_primary.gamma_multiply(t),
    );
    painter.text(
        c + Vec2::new(0.0, 80.0),
        Align2::CENTER_CENTER,
        t!("drop.release"),
        theme::font_semibold(26.0),
        p.text.gamma_multiply(t),
    );
    let sub = if count > 1 {
        if mods.ctrl || mods.command {
            t!("drop.release_compare_two").into_owned()
        } else {
            t!("drop.release_many", n = count).into_owned()
        }
    } else if mods.shift {
        t!("drop.release_compare_prev").into_owned()
    } else {
        t!("drop.release_hint").into_owned()
    };
    painter.text(
        c + Vec2::new(0.0, 112.0),
        Align2::CENTER_CENTER,
        sub,
        theme::font_regular(15.0),
        p.text_muted.gamma_multiply(t),
    );
}
