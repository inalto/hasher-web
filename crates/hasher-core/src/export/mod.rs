//! Esportazione dei risultati in vari formati. [`export_to_vec`] è il punto d'ingresso unico:
//! produce i byte del documento, che l'interfaccia salva su disco (desktop) o offre in download
//! (web, tramite `Blob`). Nessun accesso al file system.

use crate::algo::Algo;
use crate::result::FileResult;
use anyhow::Context;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

pub mod checksum_fmt;
pub mod csv_fmt;
pub mod docx_fmt;
pub mod json_fmt;
pub mod txt_fmt;
pub mod xlsx_fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Txt,
    Csv,
    Json,
    Xlsx,
    Docx,
    /// File di checksum in formato GNU (`hex  nome`) per un singolo algoritmo.
    Checksum(Algo),
}

impl ExportFormat {
    pub const BASIC: [ExportFormat; 5] = [
        ExportFormat::Txt,
        ExportFormat::Csv,
        ExportFormat::Json,
        ExportFormat::Xlsx,
        ExportFormat::Docx,
    ];

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Txt => "txt",
            ExportFormat::Csv => "csv",
            ExportFormat::Json => "json",
            ExportFormat::Xlsx => "xlsx",
            ExportFormat::Docx => "docx",
            ExportFormat::Checksum(a) => a.checksum_extension(),
        }
    }

    /// Chiave di traduzione del nome del formato (`export.format.<id>`).
    pub fn id(self) -> &'static str {
        match self {
            ExportFormat::Txt => "txt",
            ExportFormat::Csv => "csv",
            ExportFormat::Json => "json",
            ExportFormat::Xlsx => "xlsx",
            ExportFormat::Docx => "docx",
            ExportFormat::Checksum(_) => "checksum",
        }
    }

    /// Tipo MIME del documento prodotto (per il `Blob` di download).
    pub fn mime(self) -> &'static str {
        match self {
            ExportFormat::Txt | ExportFormat::Checksum(_) => "text/plain; charset=utf-8",
            ExportFormat::Csv => "text/csv; charset=utf-8",
            ExportFormat::Json => "application/json",
            ExportFormat::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ExportFormat::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        }
    }

    /// Nome di file proposto: `hasher-AAAAMMGG-HHMMSS.<ext>` (checksum: `SHA256SUMS`-like non usato,
    /// per coerenza con il desktop si usa la stessa forma).
    pub fn suggested_file_name(self, when: DateTime<Local>) -> String {
        format!("hasher-{}.{}", when.format("%Y%m%d-%H%M%S"), self.extension())
    }
}

/// Etichette già tradotte dall'interfaccia (l'export non dipende da rust-i18n).
#[derive(Debug, Clone, Default)]
pub struct ExportLabels {
    pub title: String,
    pub generated_by: String,
    pub generated_at: String,
    pub file: String,
    pub path: String,
    pub size: String,
    pub size_bytes: String,
    pub created: String,
    pub modified: String,
    pub attributes: String,
    pub permissions: String,
    pub algorithm: String,
    pub hash: String,
    pub files_count: String,
    pub elapsed: String,
    pub not_available: String,
}

#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Algoritmi da esportare, nell'ordine delle colonne.
    pub algos: Vec<Algo>,
    pub uppercase_hex: bool,
    pub labels: ExportLabels,
    pub app_name: String,
    pub app_version: String,
    pub company: String,
    pub generated_at: DateTime<Local>,
    /// Formato data `chrono` (es. `%d/%m/%Y %H:%M:%S`).
    pub date_format: String,
}

impl ExportOptions {
    /// Formatta una data con `date_format`; se il formato non è valido ricade su RFC 3339
    /// invece di andare in panic (chrono segnala l'errore solo in fase di scrittura).
    pub fn fmt_date(&self, d: Option<DateTime<Local>>) -> String {
        use std::fmt::Write as _;
        let Some(d) = d else {
            return self.labels.not_available.clone();
        };
        let mut s = String::new();
        if write!(s, "{}", d.format(&self.date_format)).is_ok() {
            s
        } else {
            d.to_rfc3339()
        }
    }
}

/// Produce il documento nel formato richiesto.
pub fn export_to_vec(format: ExportFormat, results: &[FileResult], opts: &ExportOptions) -> anyhow::Result<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    match format {
        ExportFormat::Txt => txt_fmt::write(&mut out, results, opts).context("TXT")?,
        ExportFormat::Csv => csv_fmt::write(&mut out, results, opts).context("CSV")?,
        ExportFormat::Json => json_fmt::write(&mut out, results, opts).context("JSON")?,
        ExportFormat::Xlsx => xlsx_fmt::write(&mut out, results, opts).context("XLSX")?,
        ExportFormat::Docx => docx_fmt::write(&mut out, results, opts).context("DOCX")?,
        ExportFormat::Checksum(algo) => checksum_fmt::write(&mut out, algo, results, opts).context("checksum")?,
    }
    Ok(out)
}

// ======================================================================================
// Helper condivisi dai formati (`pub(crate)`, usati solo dai sottomoduli `*_fmt`).
// ======================================================================================

/// Riga di dati già formattata per un file, condivisa dai formati tabellari (CSV, XLSX, TXT, DOCX).
pub(crate) struct FileRow {
    pub name: String,
    pub path: String,
    /// Dimensione esatta in byte.
    pub size: u64,
    pub size_human: String,
    /// Date già formattate con `opts.fmt_date` (etichetta `not_available` se assenti).
    pub created: String,
    pub modified: String,
    /// Codici brevi degli attributi (es. `RH`); vuoto se nessuno.
    pub attributes: String,
    /// Permessi in stile `ls -l`; vuoto se non disponibili.
    pub permissions: String,
    /// Un hash per ciascun algoritmo di `opts.algos` (stesso ordine); `not_available` se mancante.
    pub hashes: Vec<String>,
}

impl FileRow {
    pub(crate) fn new(r: &FileResult, opts: &ExportOptions) -> Self {
        let info = &r.info;
        FileRow {
            name: info.name.clone(),
            path: info.path.to_string_lossy().into_owned(),
            size: info.size,
            size_human: info.size_human(),
            created: opts.fmt_date(info.created),
            modified: opts.fmt_date(info.modified),
            attributes: info.attributes_short(),
            permissions: info.unix_mode_string().unwrap_or_default(),
            hashes: opts.algos.iter().map(|a| hex_or_na(r, *a, opts)).collect(),
        }
    }
}

/// Converte tutti i risultati in righe formattate.
pub(crate) fn file_rows(results: &[FileResult], opts: &ExportOptions) -> Vec<FileRow> {
    results.iter().map(|r| FileRow::new(r, opts)).collect()
}

/// Hash esadecimale dell'algoritmo (rispettando `uppercase_hex`) oppure l'etichetta `not_available`.
pub(crate) fn hex_or_na(r: &FileResult, algo: Algo, opts: &ExportOptions) -> String {
    r.hex(algo, opts.uppercase_hex)
        .unwrap_or_else(|| opts.labels.not_available.clone())
}

/// Firma del generatore: `Hasher 0.1.0 — Martini Multimedia s.a.s.`.
pub(crate) fn app_signature(opts: &ExportOptions) -> String {
    format!("{} {} \u{2014} {}", opts.app_name, opts.app_version, opts.company)
}

/// Data di generazione formattata con `opts.date_format`.
pub(crate) fn generated_at_text(opts: &ExportOptions) -> String {
    opts.fmt_date(Some(opts.generated_at))
}

/// Coppie etichetta/valore dell'intestazione: generato da, generato il, numero di file.
pub(crate) fn meta_rows(files_count: usize, opts: &ExportOptions) -> Vec<(String, String)> {
    vec![
        (opts.labels.generated_by.clone(), app_signature(opts)),
        (opts.labels.generated_at.clone(), generated_at_text(opts)),
        (opts.labels.files_count.clone(), files_count.to_string()),
    ]
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::fileinfo::FileInfo;
    use crate::hasher::hash_bytes;
    use chrono::TimeZone;
    use std::collections::BTreeMap;
    use std::time::Duration;

    pub(crate) fn sample_options(algos: &[Algo]) -> ExportOptions {
        ExportOptions {
            algos: algos.to_vec(),
            uppercase_hex: false,
            labels: ExportLabels {
                title: "Rapporto hash".into(),
                generated_by: "Generato da".into(),
                generated_at: "Data".into(),
                file: "File".into(),
                path: "Percorso".into(),
                size: "Dimensione".into(),
                size_bytes: "Dimensione (byte)".into(),
                created: "Creato".into(),
                modified: "Modificato".into(),
                attributes: "Attributi".into(),
                permissions: "Permessi".into(),
                algorithm: "Algoritmo".into(),
                hash: "Hash".into(),
                files_count: "Numero di file".into(),
                elapsed: "Tempo di calcolo".into(),
                not_available: "n/d".into(),
            },
            app_name: "Hasher".into(),
            app_version: "0.1.0".into(),
            company: "Martini Multimedia s.a.s.".into(),
            generated_at: Local.with_ymd_and_hms(2026, 10, 10, 12, 0, 0).unwrap(),
            date_format: "%d/%m/%Y %H:%M:%S".into(),
        }
    }

    pub(crate) fn sample_results(algos: &[Algo]) -> Vec<FileResult> {
        [("docs/a.txt", &b"abc"[..]), ("b = c.bin", &b""[..])]
            .iter()
            .enumerate()
            .map(|(i, (path, data))| FileResult {
                id: i as u64 + 1,
                info: FileInfo::basic(
                    *path,
                    data.len() as u64,
                    Some(Local.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap()),
                ),
                digests: algos
                    .iter()
                    .map(|a| (*a, hash_bytes(*a, data)))
                    .collect::<BTreeMap<_, _>>(),
                elapsed: Duration::from_millis(3),
            })
            .collect()
    }

    #[test]
    fn every_format_produces_a_document() {
        let algos = [Algo::Md5, Algo::Sha256];
        let results = sample_results(&algos);
        let opts = sample_options(&algos);
        for f in ExportFormat::BASIC
            .iter()
            .copied()
            .chain([ExportFormat::Checksum(Algo::Sha256)])
        {
            let bytes = export_to_vec(f, &results, &opts).unwrap_or_else(|e| panic!("{f:?}: {e:#}"));
            assert!(!bytes.is_empty(), "{f:?}");
            match f {
                ExportFormat::Xlsx | ExportFormat::Docx => assert_eq!(&bytes[..2], b"PK", "{f:?} è uno zip"),
                ExportFormat::Json => {
                    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(
                        v["files"][0]["hashes"]["sha256"],
                        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                    );
                }
                ExportFormat::Csv => assert!(bytes.starts_with(b"\xEF\xBB\xBF")),
                ExportFormat::Txt => assert!(String::from_utf8_lossy(&bytes).contains("ba7816bf8f01cfea")),
                ExportFormat::Checksum(_) => {
                    assert!(String::from_utf8_lossy(&bytes)
                        .contains("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  a.txt"))
                }
            }
        }
    }

    #[test]
    fn suggested_names_and_mime() {
        let when = Local.with_ymd_and_hms(2026, 10, 10, 9, 5, 7).unwrap();
        assert_eq!(
            ExportFormat::Xlsx.suggested_file_name(when),
            "hasher-20261010-090507.xlsx"
        );
        assert_eq!(
            ExportFormat::Checksum(Algo::Sha256).suggested_file_name(when),
            "hasher-20261010-090507.sha256"
        );
        assert!(ExportFormat::Docx.mime().contains("wordprocessingml"));
    }
}
