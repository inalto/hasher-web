//! Esportazione in formato XLSX (Excel / LibreOffice) con `rust_xlsxwriter`.
//!
//! Foglio "Hashes": intestazione in grassetto (testo bianco su blu `#144781`), prima riga
//! bloccata, filtro automatico, una colonna per algoritmo con font monospazio.
//! Foglio "Info": metadati della generazione.

use super::{app_signature, file_rows, generated_at_text, ExportOptions};
use crate::result::FileResult;
use anyhow::Context;
use rust_xlsxwriter::{Color, DocProperties, Format, FormatAlign, Workbook, Worksheet, XlsxError};
use std::io::Write;

/// Blu primario del marchio.
const HEADER_BLUE: u32 = 0x144781;
const MONO_FONT: &str = "Consolas";

/// Larghezza di una colonna hash: proporzionale alla lunghezza dell'esadecimale, tra 16 e 70.
fn hash_col_width(hex_len: usize) -> f64 {
    (hex_len as f64 * 1.1).clamp(16.0, 70.0)
}

pub fn write(out: &mut impl Write, results: &[FileResult], opts: &ExportOptions) -> anyhow::Result<()> {
    let mut workbook = Workbook::new();
    build_hashes_sheet(workbook.add_worksheet(), results, opts).context("cannot build the \"Hashes\" sheet")?;
    build_info_sheet(workbook.add_worksheet(), results.len(), opts).context("cannot build the \"Info\" sheet")?;

    let props = DocProperties::new()
        .set_title(&opts.labels.title)
        .set_author(&opts.company)
        .set_company(&opts.company);
    workbook.set_properties(&props);

    let bytes = workbook.save_to_buffer().context("cannot build the workbook")?;
    out.write_all(&bytes).context("cannot write")?;
    Ok(())
}

fn build_hashes_sheet(ws: &mut Worksheet, results: &[FileResult], opts: &ExportOptions) -> Result<(), XlsxError> {
    ws.set_name("Hashes")?;

    let header_fmt = Format::new()
        .set_bold()
        .set_font_color(Color::White)
        .set_background_color(Color::RGB(HEADER_BLUE))
        .set_align(FormatAlign::VerticalCenter);
    let int_fmt = Format::new().set_num_format("#,##0");
    let mono_fmt = Format::new().set_font_name(MONO_FONT).set_font_size(10);

    // Intestazione.
    let l = &opts.labels;
    let fixed = [
        (&l.file, 32.0),
        (&l.path, 50.0),
        (&l.size_bytes, 16.0),
        (&l.size, 12.0),
        (&l.created, 20.0),
        (&l.modified, 20.0),
        (&l.attributes, 12.0),
        (&l.permissions, 12.0),
    ];
    for (col, (title, width)) in fixed.iter().enumerate() {
        let col = col as u16;
        ws.write_string_with_format(0, col, *title, &header_fmt)?;
        ws.set_column_width(col, *width)?;
    }
    let first_algo_col = fixed.len() as u16;
    for (i, algo) in opts.algos.iter().enumerate() {
        let col = first_algo_col + i as u16;
        ws.write_string_with_format(0, col, algo.name(), &header_fmt)?;
        ws.set_column_width(col, hash_col_width(algo.hex_len()))?;
    }
    ws.set_row_height(0, 20)?;

    // Righe: ogni valore è scritto come stringa (mai come formula, anche se inizia con `=`).
    for (i, row) in file_rows(results, opts).into_iter().enumerate() {
        let r = i as u32 + 1;
        ws.write_string(r, 0, &row.name)?;
        ws.write_string(r, 1, &row.path)?;
        ws.write_number_with_format(r, 2, row.size as f64, &int_fmt)?;
        ws.write_string(r, 3, &row.size_human)?;
        ws.write_string(r, 4, &row.created)?;
        ws.write_string(r, 5, &row.modified)?;
        ws.write_string(r, 6, &row.attributes)?;
        ws.write_string(r, 7, &row.permissions)?;
        for (j, hash) in row.hashes.iter().enumerate() {
            ws.write_string_with_format(r, first_algo_col + j as u16, hash, &mono_fmt)?;
        }
    }

    // Prima riga bloccata e filtro automatico sull'intera tabella.
    ws.set_freeze_panes(1, 0)?;
    let last_row = results.len() as u32;
    let n_algos = opts.algos.len() as u16;
    let last_col = if n_algos == 0 {
        first_algo_col - 1
    } else {
        first_algo_col + n_algos - 1
    };
    ws.autofilter(0, 0, last_row, last_col)?;
    Ok(())
}

fn build_info_sheet(ws: &mut Worksheet, files_count: usize, opts: &ExportOptions) -> Result<(), XlsxError> {
    ws.set_name("Info")?;
    let key_fmt = Format::new().set_bold();

    let mut rows: Vec<(String, String)> = vec![
        (opts.labels.title.clone(), String::new()),
        (opts.labels.generated_by.clone(), app_signature(opts)),
        (opts.labels.generated_at.clone(), generated_at_text(opts)),
        (opts.labels.files_count.clone(), files_count.to_string()),
    ];
    // Algoritmi inclusi nel foglio.
    let algos: Vec<&str> = opts.algos.iter().map(|a| a.name()).collect();
    rows.push((opts.labels.algorithm.clone(), algos.join(", ")));

    for (i, (k, v)) in rows.iter().enumerate() {
        ws.write_string_with_format(i as u32, 0, k, &key_fmt)?;
        if !v.is_empty() {
            ws.write_string(i as u32, 1, v)?;
        }
    }
    ws.set_column_width(0, 28)?;
    ws.set_column_width(1, 70)?;
    Ok(())
}
