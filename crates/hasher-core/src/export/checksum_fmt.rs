//! Esportazione in formato file di checksum, riutilizzabile per la verifica:
//! - GNU coreutils (`hex  nome`, due spazi) con intestazione di commento `# …`;
//! - SFV (`nome HEX`, CRC32 maiuscolo) con commenti `; …` quando l'algoritmo è CRC32.
//!
//! I file privi dell'algoritmo richiesto vengono saltati e contati in un commento finale.
//! Nel formato GNU, un nome contenente `\`, ritorno a capo o `\r` segue la convenzione di
//! coreutils: riga prefissata da `\` e sequenze di escape `\\`, `\n`, `\r`.

use super::{app_signature, generated_at_text, ExportOptions};
use crate::algo::{to_hex, Algo};
use crate::result::FileResult;
use std::io::Write;

/// Nome nel formato GNU coreutils; il flag indica se la riga va prefissata da `\`.
fn gnu_name(name: &str) -> (bool, String) {
    if !name.contains(['\\', '\n', '\r']) {
        return (false, name.to_string());
    }
    let escaped = name.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r");
    (true, escaped)
}

pub fn write(out: &mut impl Write, algo: Algo, results: &[FileResult], opts: &ExportOptions) -> std::io::Result<()> {
    let sfv = algo == Algo::Crc32;
    let comment = if sfv { ';' } else { '#' };

    writeln!(out, "{comment} {}", app_signature(opts))?;
    writeln!(
        out,
        "{comment} {}: {}",
        opts.labels.generated_at,
        generated_at_text(opts)
    )?;

    let mut skipped = 0usize;
    for r in results {
        let Some(digest) = r.digests.get(&algo) else {
            skipped += 1;
            continue;
        };
        if sfv {
            // Convenzione SFV: CRC32 sempre maiuscolo, `nome CRC`.
            writeln!(out, "{} {}", r.info.name, to_hex(digest, true))?;
        } else {
            let (escaped, name) = gnu_name(&r.info.name);
            let prefix = if escaped { "\\" } else { "" };
            writeln!(out, "{prefix}{}  {name}", to_hex(digest, opts.uppercase_hex))?;
        }
    }

    if skipped > 0 {
        writeln!(
            out,
            "{comment} {} {}: {skipped}",
            algo.name(),
            opts.labels.not_available
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnu_names_are_escaped() {
        assert_eq!(gnu_name("plain.txt"), (false, "plain.txt".into()));
        assert_eq!(gnu_name("a\\b\nc"), (true, "a\\\\b\\nc".into()));
    }
}
