//! Esportazione in formato JSON (UTF-8, indentato). Le date sono RFC 3339, le chiavi degli
//! hash sono gli `Algo::id` nell'ordine di `opts.algos`; un hash mancante vale `null`.

use super::ExportOptions;
use crate::algo::Algo;
use crate::result::FileResult;
use anyhow::Context;
use serde::ser::{Serialize, SerializeMap, Serializer};
use std::io::Write;

#[derive(serde::Serialize)]
struct Report<'a> {
    app: App<'a>,
    generated_at: String,
    files: Vec<JsonFile>,
}

#[derive(serde::Serialize)]
struct App<'a> {
    name: &'a str,
    version: &'a str,
    company: &'a str,
}

#[derive(serde::Serialize)]
struct JsonFile {
    name: String,
    path: String,
    size: u64,
    size_human: String,
    created: Option<String>,
    modified: Option<String>,
    accessed: Option<String>,
    attributes: Vec<&'static str>,
    permissions: Option<String>,
    hashes: Hashes,
}

/// Mappa `id algoritmo -> hash` che conserva l'ordine di inserimento.
struct Hashes(Vec<(&'static str, Option<String>)>);

impl Serialize for Hashes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

fn json_file(r: &FileResult, algos: &[Algo], opts: &ExportOptions) -> JsonFile {
    let info = &r.info;
    JsonFile {
        name: info.name.clone(),
        path: info.path.to_string_lossy().into_owned(),
        size: info.size,
        size_human: info.size_human(),
        created: info.created.map(|d| d.to_rfc3339()),
        modified: info.modified.map(|d| d.to_rfc3339()),
        accessed: info.accessed.map(|d| d.to_rfc3339()),
        attributes: info.attributes.iter().map(|a| a.id()).collect(),
        permissions: info.unix_mode_string(),
        hashes: Hashes(algos.iter().map(|a| (a.id(), r.hex(*a, opts.uppercase_hex))).collect()),
    }
}

pub fn write(out: &mut impl Write, results: &[FileResult], opts: &ExportOptions) -> anyhow::Result<()> {
    let report = Report {
        app: App {
            name: &opts.app_name,
            version: &opts.app_version,
            company: &opts.company,
        },
        generated_at: opts.generated_at.to_rfc3339(),
        files: results.iter().map(|r| json_file(r, &opts.algos, opts)).collect(),
    };
    serde_json::to_writer_pretty(&mut *out, &report).context("cannot serialise the report")?;
    out.write_all(b"\n").context("cannot write")?;
    Ok(())
}
