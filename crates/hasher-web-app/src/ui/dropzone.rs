//! Stato vuoto (zona di rilascio) e sovrimpressione durante il trascinamento di file.

use super::anim;
use super::theme::{self, pal, pal_ctx};
use super::widgets::{self, Btn};
use super::{Action, HasherApp};
use egui::{
    Align, Align2, Context, Id, LayerId, Layout, Order, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, UiBuilder, Vec2,
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

/// Schermata iniziale: invito a trascinare file e cartelle.
pub(crate) fn empty_state(app: &mut HasherApp, ui: &mut Ui) {
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

    let content_h = 380.0;
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
                RichText::new(t!("drop.title"))
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

            // Suggerimenti.
            let hints: [(&[&str], String); 3] = [
                (&["Shift"], t!("drop.hint_shift").into_owned()),
                (&["Ctrl"], t!("drop.hint_ctrl").into_owned()),
                (&["Ctrl", "V"], t!("drop.hint_paste").into_owned()),
            ];
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
        },
    );

    if zone_resp.clicked() {
        app.actions.push(Action::OpenFiles);
    }
    let _ = zone_resp.on_hover_cursor(egui::CursorIcon::PointingHand);
}

/// Sovrimpressione a tutta finestra mentre l'utente trascina file sopra l'app.
pub(crate) fn overlay(ctx: &Context, forced: bool) {
    let (count, mods) = ctx.input(|i| (i.raw.hovered_files.len(), i.modifiers));
    let count = if forced { count.max(1) } else { count };
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
