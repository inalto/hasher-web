//! Esportazione in formato TXT: rapporto leggibile, UTF-8, righe terminate da `\n`.

use super::{app_signature, file_rows, meta_rows, ExportOptions};
use crate::result::FileResult;
use std::io::Write;

/// Larghezza della riga di separazione.
const RULE_WIDTH: usize = 72;

/// Numero di caratteri (non byte) di una stringa, per l'allineamento a colonne.
fn width(s: &str) -> usize {
    s.chars().count()
}

/// `etichetta<padding>: valore`
fn field(out: &mut impl Write, label: &str, label_width: usize, value: &str) -> std::io::Result<()> {
    let pad = label_width.saturating_sub(width(label));
    writeln!(out, "{label}{}: {value}", " ".repeat(pad))
}

pub fn write(out: &mut impl Write, results: &[FileResult], opts: &ExportOptions) -> std::io::Result<()> {
    let l = &opts.labels;
    let rule = "-".repeat(RULE_WIDTH);

    // Intestazione.
    writeln!(out, "{}", l.title)?;
    writeln!(out, "{}", "=".repeat(width(&l.title).max(3)))?;
    for (k, v) in meta_rows(results.len(), opts) {
        writeln!(out, "{k}: {v}")?;
    }
    writeln!(out, "{rule}")?;

    let rows = file_rows(results, opts);

    // Larghezza delle etichette delle proprietà e dei nomi degli algoritmi.
    let prop_labels = [
        &l.file,
        &l.path,
        &l.size,
        &l.size_bytes,
        &l.created,
        &l.modified,
        &l.attributes,
        &l.permissions,
    ];
    let prop_w = prop_labels.iter().map(|s| width(s)).max().unwrap_or(0);
    let algo_w = opts.algos.iter().map(|a| width(a.name())).max().unwrap_or(0);

    for row in &rows {
        writeln!(out)?;
        field(out, &l.file, prop_w, &row.name)?;
        field(out, &l.path, prop_w, &row.path)?;
        field(out, &l.size, prop_w, &row.size_human)?;
        field(out, &l.size_bytes, prop_w, &row.size.to_string())?;
        field(out, &l.created, prop_w, &row.created)?;
        field(out, &l.modified, prop_w, &row.modified)?;
        if !row.attributes.is_empty() {
            field(out, &l.attributes, prop_w, &row.attributes)?;
        }
        if !row.permissions.is_empty() {
            field(out, &l.permissions, prop_w, &row.permissions)?;
        }
        writeln!(out)?;
        for (algo, hash) in opts.algos.iter().zip(&row.hashes) {
            field(out, algo.name(), algo_w, hash)?;
        }
    }

    // Piè di pagina.
    writeln!(out)?;
    writeln!(out, "{rule}")?;
    writeln!(out, "{}", app_signature(opts))?;
    Ok(())
}
