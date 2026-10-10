//! Esportazione in formato DOCX (Word / LibreOffice Writer) con `docx-rs`.
//!
//! Layout: titolo, righe di metadati, poi per ogni file un'intestazione col nome, una tabella
//! a due colonne con le proprietà e una tabella algoritmo/hash (hash in Consolas 9 pt). Dal
//! secondo file in poi ogni sezione inizia su una nuova pagina.

use super::ExportOptions;
use super::{app_signature, file_rows, meta_rows, FileRow};
use crate::result::FileResult;
use anyhow::Context;
use docx_rs::{
    Docx, LineSpacing, PageMargin, Paragraph, Run, RunFonts, Shading, Table, TableCell, TableLayoutType, TableRow,
    WidthType,
};
use std::io::{Cursor, Write};

/// Colori del marchio (esadecimale senza `#`).
const BLUE: &str = "144781";
const LIGHT: &str = "EAF0F7";
const MONO_FONT: &str = "Consolas";

/// Margini della pagina (twip): 2 cm per lato su A4 → 9638 twip utili.
const MARGIN: i32 = 1134;
const TABLE_WIDTH: usize = 9638;
const COL_LABEL: usize = 2300;
const COL_VALUE: usize = TABLE_WIDTH - COL_LABEL;

/// Dimensioni dei caratteri in mezzi punti.
const SIZE_TITLE: usize = 40;
const SIZE_HEADING: usize = 28;
const SIZE_BODY: usize = 20;
const SIZE_HASH: usize = 18;

fn text_run(text: &str, size: usize) -> Run {
    Run::new().add_text(text).size(size)
}

fn cell(run: Run) -> TableCell {
    TableCell::new().add_paragraph(Paragraph::new().add_run(run))
}

/// Tabella a due colonne con larghezze fisse.
fn two_col_table(rows: Vec<TableRow>) -> Table {
    Table::new(rows)
        .set_grid(vec![COL_LABEL, COL_VALUE])
        .width(TABLE_WIDTH, WidthType::Dxa)
        .layout(TableLayoutType::Fixed)
}

fn label_cell(text: &str) -> TableCell {
    cell(text_run(text, SIZE_BODY).bold())
        .width(COL_LABEL, WidthType::Dxa)
        .shading(Shading::new().fill(LIGHT))
}

fn value_cell(run: Run) -> TableCell {
    cell(run).width(COL_VALUE, WidthType::Dxa)
}

fn header_cell(text: &str, width: usize) -> TableCell {
    cell(text_run(text, SIZE_BODY).bold().color("FFFFFF"))
        .width(width, WidthType::Dxa)
        .shading(Shading::new().fill(BLUE))
}

fn property_row(label: &str, value: &str) -> TableRow {
    TableRow::new(vec![label_cell(label), value_cell(text_run(value, SIZE_BODY))])
}

fn properties_table(row: &FileRow, opts: &ExportOptions) -> Table {
    let l = &opts.labels;
    let mut rows = vec![
        property_row(&l.path, &row.path),
        property_row(&l.size, &row.size_human),
        property_row(&l.size_bytes, &row.size.to_string()),
        property_row(&l.created, &row.created),
        property_row(&l.modified, &row.modified),
    ];
    if !row.attributes.is_empty() {
        rows.push(property_row(&l.attributes, &row.attributes));
    }
    if !row.permissions.is_empty() {
        rows.push(property_row(&l.permissions, &row.permissions));
    }
    two_col_table(rows)
}

fn hashes_table(row: &FileRow, opts: &ExportOptions) -> Table {
    let mut rows = vec![TableRow::new(vec![
        header_cell(&opts.labels.algorithm, COL_LABEL),
        header_cell(&opts.labels.hash, COL_VALUE),
    ])];
    for (algo, hash) in opts.algos.iter().zip(&row.hashes) {
        let mono = Run::new()
            .add_text(hash)
            .size(SIZE_HASH)
            .fonts(RunFonts::new().ascii(MONO_FONT).hi_ansi(MONO_FONT).cs(MONO_FONT));
        rows.push(TableRow::new(vec![
            cell(text_run(algo.name(), SIZE_BODY)).width(COL_LABEL, WidthType::Dxa),
            value_cell(mono),
        ]));
    }
    two_col_table(rows)
}

pub fn write(out: &mut impl Write, results: &[FileResult], opts: &ExportOptions) -> anyhow::Result<()> {
    let mut docx = Docx::new()
        .page_margin(PageMargin::new().top(MARGIN).bottom(MARGIN).left(MARGIN).right(MARGIN))
        .add_paragraph(Paragraph::new().add_run(text_run(&opts.labels.title, SIZE_TITLE).bold().color(BLUE)));

    for (k, v) in meta_rows(results.len(), opts) {
        docx = docx.add_paragraph(
            Paragraph::new()
                .add_run(text_run(&format!("{k}: "), SIZE_BODY).bold())
                .add_run(text_run(&v, SIZE_BODY)),
        );
    }

    for (i, row) in file_rows(results, opts).iter().enumerate() {
        // Intestazione del file; dal secondo in poi inizia su una nuova pagina.
        let mut heading = Paragraph::new().add_run(text_run(&row.name, SIZE_HEADING).bold().color(BLUE));
        if i > 0 {
            heading = heading.page_break_before(true);
        } else {
            heading = heading.line_spacing(LineSpacing::new().before(360));
        }
        docx = docx.add_paragraph(heading).add_table(properties_table(row, opts));
        // Word fonde due tabelle consecutive: serve un paragrafo vuoto in mezzo.
        docx = docx.add_paragraph(Paragraph::new()).add_table(hashes_table(row, opts));
    }

    // Una tabella non può chiudere il documento: paragrafo finale con la firma.
    docx = docx
        .add_paragraph(Paragraph::new())
        .add_paragraph(Paragraph::new().add_run(text_run(&app_signature(opts), SIZE_BODY).italic().color("5B6B82")));

    // Proprietà del documento: data di creazione = data di generazione (UTC, W3CDTF).
    let ts = opts
        .generated_at
        .with_timezone(&chrono::Utc)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    docx = docx
        .created_at(&ts)
        .updated_at(&ts)
        .custom_property("Author", &opts.company)
        .custom_property("Generator", app_signature(opts));

    let mut buf = Cursor::new(Vec::new());
    docx.build().pack(&mut buf).context("cannot pack the document")?;
    out.write_all(&buf.into_inner()).context("cannot write")?;
    Ok(())
}
