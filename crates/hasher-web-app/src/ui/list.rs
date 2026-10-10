//! Vista elenco (più file / cartelle): barra strumenti, tabella virtualizzata con righe
//! espandibili, selezione multipla.
//!
//! La tabella è disegnata a mano invece di usare `egui_extras::TableBuilder`: le righe espanse
//! contengono una card a tutta larghezza di altezza variabile (metadati + tutte le righe hash),
//! cosa che `TableBuilder` non supporta (nessuna cella su più colonne; una colonna non ritagliata
//! si allargherebbe alla larghezza del dettaglio).

use super::anim;
use super::single;
use super::theme::{self, pal, RADIUS_CARD};
use super::widgets::{self, Btn, IconButton, Verdict};
use super::{Action, EntryState, HasherApp};
use crate::backend::algo::Algo;
use egui::{
    Align, Align2, Color32, CornerRadius, Frame, Id, Layout, Margin, Pos2, Rect, RichText, ScrollArea, Sense, Shadow,
    Stroke, TextEdit, Ui, UiBuilder, Vec2,
};
use egui_phosphor::regular as icons;
use rust_i18n::t;

const ROW_H: f32 = 46.0;
const HEADER_H: f32 = 34.0;
const PAD: f32 = 14.0;

/// Posizioni orizzontali delle colonne (relative al bordo sinistro della tabella).
struct Cols {
    status: (f32, f32),
    name: (f32, f32),
    size: (f32, f32),
    modified: Option<(f32, f32)>,
    hash: (f32, f32),
    expand: (f32, f32),
}

impl Cols {
    fn new(width: f32) -> Cols {
        let status_w = 34.0;
        let size_w = 92.0;
        let expand_w = 40.0;
        let modified_w = if width > 860.0 { 152.0 } else { 0.0 };
        let hash_w = (width * 0.36).clamp(170.0, 470.0);
        let gap = 12.0;
        let fixed = PAD
            + status_w
            + gap
            + size_w
            + gap
            + modified_w
            + if modified_w > 0.0 { gap } else { 0.0 }
            + hash_w
            + gap
            + expand_w
            + 6.0;
        let name_w = (width - fixed - gap).max(120.0);
        let mut x = PAD;
        let mut take = |w: f32| {
            let r = (x, x + w);
            x += w + gap;
            r
        };
        let status = take(status_w);
        let name = take(name_w);
        let size = take(size_w);
        let modified = if modified_w > 0.0 { Some(take(modified_w)) } else { None };
        let hash = take(hash_w);
        let expand = take(expand_w);
        Cols {
            status,
            name,
            size,
            modified,
            hash,
            expand,
        }
    }
}

fn col_rect(row: Rect, c: (f32, f32)) -> Rect {
    Rect::from_x_y_ranges((row.left() + c.0)..=(row.left() + c.1), row.y_range())
}

pub(crate) fn show(app: &mut HasherApp, ui: &mut Ui) {
    toolbar(app, ui);
    ui.add_space(10.0);
    let p = pal(ui);
    Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(RADIUS_CARD)
        .shadow(Shadow {
            offset: [0, 2],
            blur: 12,
            spread: 0,
            color: p.shadow,
        })
        .show(ui, |ui| {
            let size = ui.available_size();
            ui.set_min_size(size);
            ui.set_max_width(size.x);
            table(app, ui);
        });
}

fn toolbar(app: &mut HasherApp, ui: &mut Ui) {
    let p = pal(ui);
    let total: u64 = app.entries.iter().map(|e| e.size).sum();
    let selected = app.entries.iter().filter(|e| e.selected).count();
    let compact = ui.available_width() < 820.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let count = anim::animated_number(ui.ctx(), Id::new("hasher.list.count"), app.entries.len() as f64);
        let n = count.round() as usize;
        ui.label(
            RichText::new(t!(super::format::plural_key("list.files_count", n), n = n))
                .font(theme::font_semibold(17.0))
                .color(p.text),
        );
        widgets::badge(ui, super::format::size(total), p.accent);
        if selected > 0 {
            widgets::badge(
                ui,
                t!(super::format::plural_key("list.selected_count", selected), n = selected),
                p.primary,
            );
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let clear = if compact {
                ui.add(IconButton::new(icons::BROOM).size(32.0).tooltip(t!("list.clear")))
            } else {
                ui.add(Btn::ghost(t!("list.clear")).icon(icons::BROOM).small())
            };
            if clear.clicked() {
                app.actions.push(Action::ClearList);
            }
            if ui
                .add_enabled(
                    app.any_done(),
                    Btn::secondary(t!("list.export")).icon(icons::EXPORT).small(),
                )
                .clicked()
            {
                app.actions.push(Action::ShowExport);
            }
            let cancelled = app.cancelled_count();
            if cancelled > 0
                && !app.is_running()
                && ui
                    .add(
                        Btn::primary(t!("app.resume_count", n = cancelled))
                            .icon(icons::ARROW_CLOCKWISE)
                            .small(),
                    )
                    .on_hover_text(t!("app.resume_tooltip"))
                    .clicked()
            {
                app.actions.push(Action::ResumeCancelled);
            }
            if selected > 0 {
                let remove = if compact {
                    ui.add(
                        IconButton::new(icons::TRASH)
                            .size(32.0)
                            .tooltip(t!("list.remove_selected")),
                    )
                } else {
                    ui.add(Btn::ghost(t!("list.remove_selected")).icon(icons::TRASH).small())
                };
                if remove.clicked() {
                    app.actions.push(Action::RemoveSelected);
                }
            }
            ui.add_space(2.0);
            let search_w = (ui.available_width() - 12.0).clamp(110.0, 240.0);
            ui.add(
                TextEdit::singleline(&mut app.ui.filter)
                    .id(Id::new("hasher.list.filter"))
                    .hint_text(t!("list.filter_hint"))
                    .prefix(
                        RichText::new(icons::MAGNIFYING_GLASS)
                            .font(theme::font_icon(15.0))
                            .color(p.text_muted),
                    )
                    .desired_width(search_w)
                    .margin(Margin::symmetric(8, 5)),
            );
        });
    });
}

/// Selettore dell'algoritmo mostrato nella colonna hash (nell'intestazione della tabella).
fn column_algo_combo(app: &mut HasherApp, ui: &mut Ui, rect: Rect) {
    let p = pal(ui);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(
                Pos2::new(rect.left() - 8.0, rect.center().y - 13.0),
                Vec2::new(150.0, 26.0),
            ))
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.spacing_mut().interact_size.y = 24.0;
    child.spacing_mut().button_padding = Vec2::new(8.0, 3.0);
    let algos = app.algos.clone();
    egui::ComboBox::from_id_salt("hasher.list.column_algo")
        .selected_text(
            RichText::new(app.ui.column_algo.name().to_uppercase())
                .font(theme::font_semibold(11.5))
                .color(p.text_muted),
        )
        .width(140.0)
        .icon(widgets::combo_caret)
        .show_ui(&mut child, |ui| {
            for a in algos {
                ui.selectable_value(&mut app.ui.column_algo, a, a.name());
            }
        })
        .response
        .on_hover_text(t!("list.column_algo_tooltip"));
}

fn table(app: &mut HasherApp, ui: &mut Ui) {
    let p = pal(ui);
    let full = ui.max_rect();
    let cols = Cols::new(full.width());

    // Intestazione.
    let (header, _) = ui.allocate_exact_size(Vec2::new(full.width(), HEADER_H), Sense::hover());
    ui.painter().rect_filled(
        header,
        CornerRadius {
            nw: RADIUS_CARD,
            ne: RADIUS_CARD,
            sw: 0,
            se: 0,
        },
        p.elevated.gamma_multiply(if p.dark { 0.7 } else { 0.6 }),
    );
    ui.painter()
        .hline(header.x_range(), header.bottom(), Stroke::new(1.0, p.border));
    let hfont = theme::font_semibold(11.5);
    let head = |ui: &Ui, c: (f32, f32), text: String, align: Align2| {
        let r = col_rect(header, c);
        let pos = match align {
            Align2::RIGHT_CENTER => r.right_center(),
            _ => r.left_center(),
        };
        ui.painter().text(pos, align, text, hfont.clone(), p.text_muted);
    };
    head(ui, cols.name, t!("list.col_name").to_uppercase(), Align2::LEFT_CENTER);
    head(ui, cols.size, t!("list.col_size").to_uppercase(), Align2::RIGHT_CENTER);
    if let Some(m) = cols.modified {
        head(ui, m, t!("list.col_modified").to_uppercase(), Align2::LEFT_CENTER);
    }
    column_algo_combo(app, ui, col_rect(header, cols.hash));

    // Righe filtrate.
    let filter = app.ui.filter.trim().to_lowercase();
    let visible: Vec<usize> = app
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            filter.is_empty() || e.name_lc.contains(&filter) || e.dir_text.to_lowercase().contains(&filter)
        })
        .map(|(i, _)| i)
        .collect();

    if app.entries.is_empty() {
        scanning_placeholder(ui, &cols);
        return;
    }
    if visible.is_empty() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(icons::FUNNEL)
                    .font(theme::font_icon(28.0))
                    .color(p.text_faint),
            );
            ui.label(RichText::new(t!("list.no_match")).color(p.text_muted));
        });
        return;
    }

    let height_of = |app: &HasherApp, i: usize| -> f32 {
        let e = &app.entries[i];
        if e.expanded {
            ROW_H + e.detail_h.max(160.0) + 14.0
        } else {
            ROW_H
        }
    };

    ScrollArea::vertical()
        .id_salt("hasher.list.scroll")
        .auto_shrink([false, false])
        .content_margin(Margin::ZERO)
        .show_viewport(ui, |ui, viewport| {
            let total: f32 = visible.iter().map(|&i| height_of(app, i)).sum();
            ui.set_height(total.max(1.0));
            let top = ui.max_rect().top();
            let left = ui.max_rect().left();
            let width = ui.max_rect().width();
            let mut y = 0.0;
            let last = *visible.last().unwrap_or(&0);
            let compare = app.compare_pair().map(|(a, b)| (a.id, b.id));
            for &i in &visible {
                let h = height_of(app, i);
                if y + h >= viewport.min.y && y <= viewport.max.y {
                    let rect = Rect::from_min_size(Pos2::new(left, top + y), Vec2::new(width, h));
                    let id = app.entries[i].id;
                    ui.scope_builder(UiBuilder::new().max_rect(rect).id_salt(("hasher.row", id)), |ui| {
                        row(app, ui, i, rect, &cols, i == last, compare);
                    });
                }
                y += h;
            }
        });
}

fn scanning_placeholder(ui: &mut Ui, cols: &Cols) {
    let p = pal(ui);
    let t = anim::now(ui.ctx());
    let full = ui.available_rect_before_wrap();
    for k in 0..4 {
        let r = Rect::from_min_size(
            Pos2::new(full.left(), full.top() + k as f32 * ROW_H),
            Vec2::new(full.width(), ROW_H),
        );
        let alpha = 1.0 - k as f32 * 0.22;
        let name = col_rect(r, cols.name);
        let mut ui2 = ui.new_child(UiBuilder::new().max_rect(r));
        ui2.set_opacity(alpha);
        anim::shimmer_rect(
            &ui2,
            Rect::from_min_size(
                Pos2::new(name.left(), r.center().y - 6.0),
                Vec2::new(name.width() * 0.6, 12.0),
            ),
            t,
        );
        let hash = col_rect(r, cols.hash);
        anim::shimmer_rect(
            &ui2,
            Rect::from_min_size(
                Pos2::new(hash.left(), r.center().y - 5.0),
                Vec2::new(hash.width() * 0.8, 10.0),
            ),
            t,
        );
        ui2.painter()
            .hline(r.x_range(), r.bottom(), Stroke::new(1.0, p.border.gamma_multiply(0.6)));
    }
    ui.allocate_space(Vec2::new(full.width(), ROW_H * 4.0 + 20.0));
    ui.vertical_centered(|ui| {
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() - 200.0).max(0.0) / 2.0);
            anim::spinner_ring(ui, 18.0, p.primary);
            ui.label(RichText::new(t!("status.scanning")).color(p.text_muted));
        });
    });
}

fn row(
    app: &mut HasherApp,
    ui: &mut Ui,
    i: usize,
    rect: Rect,
    cols: &Cols,
    is_last: bool,
    compare: Option<(super::FileId, super::FileId)>,
) {
    let p = pal(ui);
    let ctx = ui.ctx().clone();
    let mods = ui.input(|inp| inp.modifiers);
    let (id, appeared, expanded, selected, verdict) = {
        let e = &app.entries[i];
        (e.id, e.appeared, e.expanded, e.selected, e.verdict)
    };
    let alpha = anim::appear(&ctx, appeared, 0.35);
    ui.set_opacity(alpha);

    let r = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW_H));
    let resp = ui.interact(r, Id::new(("hasher.row.click", id)), Sense::click());
    let hovered = resp.hovered();

    // Sfondo della riga.
    let bg = if selected {
        p.primary_soft()
    } else {
        match verdict {
            Verdict::Match => p.success_soft(),
            Verdict::Mismatch => p.error_soft(),
            Verdict::None if hovered => p.elevated.gamma_multiply(0.55),
            Verdict::None => Color32::TRANSPARENT,
        }
    };
    if bg != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect.shrink2(Vec2::new(4.0, 1.0)), 8, bg);
    }
    if selected {
        let bar = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, r.top() + 9.0),
            Vec2::new(3.0, ROW_H - 18.0),
        );
        ui.painter().rect_filled(bar, 2, p.primary);
    }
    if !is_last || expanded {
        ui.painter().hline(
            rect.x_range().shrink(PAD),
            rect.bottom() - 0.5,
            Stroke::new(1.0, p.border.gamma_multiply(0.8)),
        );
    }

    let t = anim::now(&ctx);
    let column_algo = app.ui.column_algo;
    {
        let e = &app.entries[i];
        // Stato.
        let sc = col_rect(r, cols.status).center();
        match &e.state {
            EntryState::Queued => {
                ui.painter().text(
                    sc,
                    Align2::CENTER_CENTER,
                    icons::CIRCLE_DASHED,
                    theme::font_icon(18.0),
                    p.text_faint,
                );
            }
            EntryState::Computing { .. } => {
                anim::paint_spinner(ui, Rect::from_center_size(sc, Vec2::splat(18.0)), p.primary);
            }
            EntryState::Done(_) => {
                let (icon, color) = match verdict {
                    Verdict::Match => (icons::SEAL_CHECK, p.success),
                    Verdict::Mismatch => (icons::X_CIRCLE, p.error),
                    Verdict::None => (icons::CHECK_CIRCLE, p.success),
                };
                ui.painter()
                    .text(sc, Align2::CENTER_CENTER, icon, theme::font_icon(19.0), color);
            }
            EntryState::Failed(_) => {
                ui.painter().text(
                    sc,
                    Align2::CENTER_CENTER,
                    icons::WARNING_CIRCLE,
                    theme::font_icon(19.0),
                    p.error,
                );
            }
            EntryState::Cancelled => {
                ui.painter().text(
                    sc,
                    Align2::CENTER_CENTER,
                    icons::STOP_CIRCLE,
                    theme::font_icon(19.0),
                    p.text_faint,
                );
            }
        }

        // Nome (+ cartella).
        let nr = col_rect(r, cols.name);
        let icon = widgets::file_icon(e.extension.as_deref());
        ui.painter().text(
            Pos2::new(nr.left() + 9.0, nr.center().y),
            Align2::CENTER_CENTER,
            icon,
            theme::font_icon(18.0),
            p.accent,
        );
        let text_left = nr.left() + 26.0;
        let text_w = nr.right() - text_left;
        let name_g = widgets::truncated_galley(ui, &e.name, theme::font_medium(14.0), p.text, text_w);
        let dir_g = widgets::truncated_galley(ui, &e.dir_text, theme::font_regular(11.5), p.text_faint, text_w);
        let block_h = name_g.size().y + dir_g.size().y;
        let y0 = nr.center().y - block_h / 2.0;
        ui.painter().galley(Pos2::new(text_left, y0), name_g, p.text);
        ui.painter()
            .galley(Pos2::new(text_left, y0 + block_h - dir_g.size().y), dir_g, p.text_faint);

        // Dimensione.
        let szr = col_rect(r, cols.size);
        ui.painter().text(
            szr.right_center(),
            Align2::RIGHT_CENTER,
            &e.size_text,
            theme::font_regular(13.0),
            p.text_muted,
        );

        // Data di modifica.
        if let Some(m) = cols.modified {
            let mr = col_rect(r, m);
            let text = if e.modified_text.is_empty() {
                "—"
            } else {
                e.modified_text.as_str()
            };
            ui.painter().text(
                mr.left_center(),
                Align2::LEFT_CENTER,
                text,
                theme::font_regular(13.0),
                p.text_muted,
            );
        }
    }

    // Colonna hash / avanzamento.
    let hr = col_rect(r, cols.hash);
    let mut copy: Option<(String, Algo)> = None;
    {
        let e = &app.entries[i];
        match &e.state {
            EntryState::Queued => {
                anim::shimmer_rect(
                    ui,
                    Rect::from_min_size(
                        Pos2::new(hr.left(), hr.center().y - 5.0),
                        Vec2::new(hr.width() * 0.75, 10.0),
                    ),
                    t,
                );
            }
            EntryState::Computing { .. } => {
                let frac = e.fraction();
                let bar = Rect::from_min_size(
                    Pos2::new(hr.left(), hr.center().y - 3.0),
                    Vec2::new(hr.width() - 46.0, 6.0),
                );
                widgets::paint_progress(ui, bar, Some(frac), true, p);
                ui.painter().text(
                    Pos2::new(hr.right(), hr.center().y),
                    Align2::RIGHT_CENTER,
                    super::format::percent(frac),
                    theme::font_semibold(12.0),
                    p.primary,
                );
            }
            EntryState::Done(_) => match e.hex(column_algo) {
                Some(hex) => {
                    // In elenco il rosso segnala solo i file davvero in errore (checksum fallito o
                    // confronto); con un hash incollato quasi tutti i file "non corrispondono".
                    let in_compare = compare.is_some_and(|(a, b)| a == e.id || b == e.id);
                    let color = match app.algo_verdict(e, column_algo) {
                        Verdict::Match => p.success,
                        Verdict::Mismatch if in_compare || e.verdict == Verdict::Mismatch => p.error,
                        _ => p.text,
                    };
                    let g = widgets::truncated_galley(ui, hex, theme::font_mono(13.0), color, hr.width() - 34.0);
                    ui.painter()
                        .galley(Pos2::new(hr.left(), hr.center().y - g.size().y / 2.0), g, color);
                    let br = Rect::from_center_size(Pos2::new(hr.right() - 13.0, hr.center().y), Vec2::splat(28.0));
                    if ui
                        .put(
                            br,
                            IconButton::new(icons::COPY).size(28.0).tooltip(t!("single.copy_hash")),
                        )
                        .clicked()
                    {
                        copy = Some((hex.to_owned(), column_algo));
                    }
                }
                None => {
                    ui.painter().text(
                        hr.left_center(),
                        Align2::LEFT_CENTER,
                        t!("single.not_computed"),
                        theme::font_regular(12.5),
                        p.text_faint,
                    );
                }
            },
            EntryState::Failed(err) => {
                let g = widgets::truncated_galley(ui, err, theme::font_regular(12.5), p.error, hr.width());
                ui.painter()
                    .galley(Pos2::new(hr.left(), hr.center().y - g.size().y / 2.0), g, p.error);
            }
            EntryState::Cancelled => {
                ui.painter().text(
                    hr.left_center(),
                    Align2::LEFT_CENTER,
                    t!("list.state_cancelled"),
                    theme::font_regular(12.5),
                    p.text_faint,
                );
            }
        }
    }

    // Espandi/comprimi.
    let er = col_rect(r, cols.expand);
    let caret = if expanded { icons::CARET_UP } else { icons::CARET_DOWN };
    let tip = if expanded {
        t!("list.collapse")
    } else {
        t!("list.expand")
    };
    if ui
        .put(
            Rect::from_center_size(er.center(), Vec2::splat(30.0)),
            IconButton::new(caret).size(30.0).tooltip(tip),
        )
        .clicked()
    {
        app.actions.push(Action::ToggleExpand(id));
    }

    if let Some((hex, algo)) = copy {
        app.actions.push(Action::Copy {
            text: hex,
            toast: t!("toast.copied", algo = algo.name()).into_owned(),
        });
    }
    if resp.clicked() {
        app.actions.push(Action::RowClick { id, mods });
    }
    resp.context_menu(|ui| {
        let e = &app.entries[i];
        if let Some(hex) = e.hex(column_algo) {
            if ui.button(t!("list.ctx_copy", algo = column_algo.name())).clicked() {
                app.actions.push(Action::Copy {
                    text: hex.to_owned(),
                    toast: t!("toast.copied", algo = column_algo.name()).into_owned(),
                });
            }
        }
        if e.is_done() && ui.button(t!("single.copy_all")).clicked() {
            app.actions.push(Action::CopyAll(id));
        }
        if let Some(parent) = e.path.parent() {
            if ui.button(t!("single.open_folder")).clicked() {
                app.actions.push(Action::OpenDir(parent.to_path_buf()));
            }
        }
        ui.separator();
        if ui.button(t!("list.remove")).clicked() {
            app.actions.push(Action::Remove(id));
        }
    });

    // Dettaglio espanso.
    if expanded {
        let detail_max = Rect::from_min_max(
            Pos2::new(rect.left() + PAD, r.bottom() + 4.0),
            Pos2::new(rect.right() - PAD, r.bottom() + 4.0 + 10_000.0),
        );
        let inner = ui.scope_builder(
            UiBuilder::new().max_rect(detail_max).id_salt(("hasher.detail", id)),
            |ui| {
                Frame::new()
                    .fill(if p.dark {
                        p.bg.lerp_to_gamma(p.surface, 0.5)
                    } else {
                        p.bg
                    })
                    .stroke(Stroke::new(1.0, p.border))
                    .corner_radius(10)
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let items = single::info_items(&app.entries[i]);
                        widgets::info_grid(ui, ("hasher.detail.info", id), &items);
                        ui.add_space(6.0);
                        single::hash_rows(app, ui, i);
                    });
            },
        );
        let h = inner.response.rect.height();
        if (h - app.entries[i].detail_h).abs() > 0.5 {
            app.entries[i].detail_h = h;
            ctx.request_repaint();
        }
    }
}
