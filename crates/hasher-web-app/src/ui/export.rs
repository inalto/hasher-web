//! Finestra di esportazione: scelta del formato, dialogo di salvataggio nativo, scrittura.

use super::theme::{self, pal, RADIUS_WIDGET};
use super::widgets::{self, Btn, ToastKind};
use super::{guard, HasherApp};
use crate::backend::algo::Algo;
use crate::backend::export::{self, ExportFormat, ExportLabels, ExportOptions};
use crate::backend::settings::Settings;
use egui::{Align, Align2, Context, Id, Layout, RichText, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as icons;
use rust_i18n::t;

/// Numero di formati "di base" + il file di checksum.
const CHOICES: usize = ExportFormat::BASIC.len() + 1;

#[derive(Debug, Clone)]
pub(crate) struct ExportDialog {
    /// Indice in `ExportFormat::BASIC`; `BASIC.len()` = file di checksum.
    pub choice: usize,
    pub checksum_algo: Algo,
    pub selected_only: bool,
    pub has_selection: bool,
}

impl ExportDialog {
    pub fn from_settings(s: &Settings) -> Self {
        let mut d = ExportDialog {
            choice: 0,
            checksum_algo: Algo::Sha256,
            selected_only: false,
            has_selection: false,
        };
        match s.last_export_format {
            Some(ExportFormat::Checksum(a)) => {
                d.choice = ExportFormat::BASIC.len();
                d.checksum_algo = a;
            }
            Some(f) => d.choice = ExportFormat::BASIC.iter().position(|x| *x == f).unwrap_or(0),
            None => {}
        }
        d
    }

    /// Prepara la finestra prima di mostrarla.
    pub fn prepare(&mut self, algos: &[Algo], has_selection: bool) {
        self.has_selection = has_selection;
        self.selected_only = has_selection;
        if !algos.contains(&self.checksum_algo) {
            self.checksum_algo = if algos.contains(&Algo::Sha256) {
                Algo::Sha256
            } else {
                algos[0]
            };
        }
    }

    pub fn format(&self) -> ExportFormat {
        ExportFormat::BASIC
            .get(self.choice)
            .copied()
            .unwrap_or(ExportFormat::Checksum(self.checksum_algo))
    }
}

fn format_icon(f: ExportFormat) -> &'static str {
    match f {
        ExportFormat::Txt => icons::FILE_TEXT,
        ExportFormat::Csv => icons::FILE_CSV,
        ExportFormat::Json => icons::BRACKETS_CURLY,
        ExportFormat::Xlsx => icons::FILE_XLS,
        ExportFormat::Docx => icons::FILE_DOC,
        ExportFormat::Checksum(_) => icons::FINGERPRINT,
    }
}

/// Etichette tradotte passate all'esportazione.
pub(crate) fn labels() -> ExportLabels {
    ExportLabels {
        title: t!("export.labels.title").into_owned(),
        generated_by: t!("export.labels.generated_by").into_owned(),
        generated_at: t!("export.labels.generated_at").into_owned(),
        file: t!("export.labels.file").into_owned(),
        path: t!("export.labels.path").into_owned(),
        size: t!("export.labels.size").into_owned(),
        size_bytes: t!("export.labels.size_bytes").into_owned(),
        created: t!("export.labels.created").into_owned(),
        modified: t!("export.labels.modified").into_owned(),
        attributes: t!("export.labels.attributes").into_owned(),
        permissions: t!("export.labels.permissions").into_owned(),
        algorithm: t!("export.labels.algorithm").into_owned(),
        hash: t!("export.labels.hash").into_owned(),
        files_count: t!("export.labels.files_count").into_owned(),
        elapsed: t!("export.labels.elapsed").into_owned(),
        not_available: t!("export.labels.not_available").into_owned(),
    }
}

/// Mostra la finestra di esportazione (se aperta).
pub(crate) fn window(app: &mut HasherApp, ctx: &Context) {
    if !app.ui.export_open {
        return;
    }
    let mut open = true;
    let mut go = false;
    let mut cancel = false;
    let computed: Vec<Algo> = Algo::ALL
        .iter()
        .copied()
        .filter(|a| {
            app.entries
                .iter()
                .any(|e| e.result().is_some_and(|r| r.digests.contains_key(a)))
        })
        .collect();
    let n_all = app.entries.iter().filter(|e| e.is_done()).count();
    let n_sel = app.entries.iter().filter(|e| e.is_done() && e.selected).count();

    egui::Window::new(RichText::new(t!("export.title")).font(theme::font_semibold(17.0)))
        .id(Id::new("hasher.export.window"))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .open(&mut open)
        .show(ctx, |ui| {
            let p = pal(ui);
            ui.set_width(460.0);
            let dlg = &mut app.ui.export;
            let n = if dlg.selected_only { n_sel } else { n_all };
            widgets::caption(ui, t!(super::format::plural_key("export.subtitle", n), n = n));
            ui.add_space(10.0);

            for i in 0..CHOICES {
                let f = if i < ExportFormat::BASIC.len() {
                    ExportFormat::BASIC[i]
                } else {
                    ExportFormat::Checksum(dlg.checksum_algo)
                };
                let selected = dlg.choice == i;
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 50.0), Sense::click());
                if resp.clicked() {
                    dlg.choice = i;
                }
                if ui.is_rect_visible(rect) {
                    let fill = if selected {
                        p.primary_soft()
                    } else if resp.hovered() {
                        p.elevated.gamma_multiply(0.7)
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    let stroke = if selected {
                        Stroke::new(1.5, p.primary)
                    } else {
                        Stroke::new(1.0, p.border)
                    };
                    ui.painter().rect(rect, RADIUS_WIDGET, fill, stroke, StrokeKind::Inside);
                    let icon_c = egui::pos2(rect.left() + 26.0, rect.center().y);
                    ui.painter().text(
                        icon_c,
                        Align2::CENTER_CENTER,
                        format_icon(f),
                        theme::font_icon(22.0),
                        if selected { p.primary } else { p.accent },
                    );
                    ui.painter().text(
                        egui::pos2(rect.left() + 50.0, rect.center().y - 8.0),
                        Align2::LEFT_CENTER,
                        t!(format!("export.format.{}", f.id())),
                        theme::font_semibold(14.0),
                        p.text,
                    );
                    ui.painter().text(
                        egui::pos2(rect.left() + 50.0, rect.center().y + 9.0),
                        Align2::LEFT_CENTER,
                        t!(format!("export.format_desc.{}", f.id())),
                        theme::font_regular(12.0),
                        p.text_muted,
                    );
                    // Indicatore radio a destra.
                    let c = egui::pos2(rect.right() - 20.0, rect.center().y);
                    ui.painter().circle_stroke(
                        c,
                        8.0,
                        Stroke::new(1.5, if selected { p.primary } else { p.border_strong }),
                    );
                    if selected {
                        ui.painter().circle_filled(c, 4.5, p.primary);
                    }
                }
                ui.add_space(4.0);
            }

            if dlg.choice == ExportFormat::BASIC.len() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t!("export.checksum_algo")).color(p.text_muted));
                    egui::ComboBox::from_id_salt("hasher.export.algo")
                        .selected_text(dlg.checksum_algo.name())
                        .width(170.0)
                        .icon(widgets::combo_caret)
                        .show_ui(ui, |ui| {
                            for a in &computed {
                                ui.selectable_value(&mut dlg.checksum_algo, *a, a.name());
                            }
                        });
                });
            }

            if dlg.has_selection {
                ui.add_space(6.0);
                widgets::check(ui, &mut dlg.selected_only, t!("export.selected_only", n = n_sel));
            }

            ui.add_space(14.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(Btn::primary(t!("export.export_button")).icon(icons::EXPORT))
                    .clicked()
                {
                    go = true;
                }
                if ui.add(Btn::ghost(t!("app.cancel"))).clicked() {
                    cancel = true;
                }
            });
        });

    if !open || cancel {
        app.ui.export_open = false;
    }
    if go {
        app.ui.export_open = false;
        run_export(app, ctx);
    }
}

fn run_export(app: &mut HasherApp, ctx: &Context) {
    let format = app.ui.export.format();
    let results = app.done_results(app.ui.export.selected_only && app.ui.export.has_selection);
    if results.is_empty() {
        widgets::toast(ctx, ToastKind::Info, t!("toast.nothing_to_export"));
        return;
    }
    let algos: Vec<Algo> = match format {
        ExportFormat::Checksum(a) => vec![a],
        _ => app
            .algos
            .iter()
            .copied()
            .filter(|a| results.iter().any(|r| r.digests.contains_key(a)))
            .collect(),
    };
    let ext = format.extension();
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let file_name = match format {
        ExportFormat::Checksum(_) if results.len() == 1 => format!("{}.{ext}", results[0].info.name),
        ExportFormat::Checksum(_) => format!("checksums.{ext}"),
        _ if results.len() == 1 => format!("{}.hasher.{ext}", results[0].info.name),
        _ => format!("hasher-{stamp}.{ext}"),
    };
    let mut dlg = rfd::FileDialog::new()
        .set_title(t!("export.dialog_title"))
        .add_filter(t!(format!("export.format.{}", format.id())), &[ext])
        .set_file_name(file_name);
    if let Some(dir) = app.settings.last_export_dir.as_ref().filter(|d| d.is_dir()) {
        dlg = dlg.set_directory(dir);
    } else if let Some(dir) = results[0].info.path.parent() {
        dlg = dlg.set_directory(dir);
    }
    let Some(mut path) = dlg.save_file() else { return };
    if path.extension().is_none() {
        path.set_extension(ext);
    }
    let opts = ExportOptions {
        algos,
        uppercase_hex: app.settings.uppercase_hex,
        labels: labels(),
        app_name: crate::APP_NAME.to_owned(),
        app_version: crate::APP_VERSION.to_owned(),
        company: crate::COMPANY.to_owned(),
        generated_at: chrono::Local::now(),
        date_format: t!("export.date_format").into_owned(),
    };
    let shown = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match guard(|| export::export(&path, format, &results, &opts)) {
        Ok(Ok(())) => {
            widgets::toast(ctx, ToastKind::Success, t!("toast.exported", file = shown));
            app.settings.last_export_format = Some(format);
            app.settings.last_export_dir = path.parent().map(|p| p.to_path_buf());
            app.save_settings(ctx, false);
        }
        Ok(Err(e)) => widgets::toast(
            ctx,
            ToastKind::Error,
            t!("toast.export_error", error = format!("{e:#}")),
        ),
        Err(e) => widgets::toast(ctx, ToastKind::Error, t!("toast.export_error", error = e)),
    }
}
