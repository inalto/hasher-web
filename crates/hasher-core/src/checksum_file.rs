//! Lettura dei file di checksum (`.md5`, `.sha256`, `.sfv`, …) per la verifica in blocco.
//!
//! Formati riconosciuti:
//! - GNU coreutils: `hex  nome` oppure `hex *nome` (binario);
//! - BSD: `ALGO (nome) = hex`;
//! - SFV: `nome hex` con CRC32 a 8 cifre in fondo alla riga;
//! - righe di commento che iniziano con `;` o `#`, righe vuote.
//!
//! Nessun accesso al file system: `parse_bytes` riceve i byte già letti (sul web da `File`).
//!
//! Sono tollerati anche: BOM UTF-8 (e file UTF-16 con BOM), fine riga CRLF, spazi in coda, il
//! formato OpenSSL `ALGO(nome)= hex` e i nomi con escape di coreutils (riga che inizia con `\`).
//! Le righe non riconosciute vengono saltate (con un messaggio `debug` nel log).

use crate::algo::Algo;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecksumEntry {
    /// Algoritmo se deducibile (formato BSD o estensione del file); altrimenti si usa la lunghezza.
    pub algo: Option<Algo>,
    /// Hash esadecimale normalizzato (minuscolo).
    pub hex: String,
    /// Nome del file così come scritto nel file di checksum (relativo alla cartella del checksum).
    pub file_name: String,
    /// Numero di riga (1-based), per i messaggi d'errore.
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct ChecksumFile {
    pub path: PathBuf,
    /// Cartella rispetto alla quale risolvere i nomi dei file.
    pub base_dir: PathBuf,
    pub default_algo: Option<Algo>,
    pub entries: Vec<ChecksumEntry>,
}

impl ChecksumEntry {
    /// Algoritmi candidati per questa voce: quello dichiarato, altrimenti quelli con la stessa lunghezza.
    pub fn candidate_algos(&self) -> Vec<Algo> {
        match self.algo {
            Some(a) => vec![a],
            None => Algo::candidates_for_hex_len(self.hex.len()),
        }
    }
}

/// Estensioni riconosciute come file di checksum.
pub const CHECKSUM_EXTENSIONS: &[&str] = &[
    "sfv",
    "md4",
    "md5",
    "md5sum",
    "sha",
    "sha1",
    "sha1sum",
    "sha224",
    "sha256",
    "sha256sum",
    "sha384",
    "sha512",
    "sha512sum",
    "sha3",
    "sha3-256",
    "sha3-512",
    "blake2b",
    "b2",
    "b2sum",
    "blake2",
    "blake3",
    "b3",
    "ed2k",
    "crc32",
    "hash",
    "checksum",
    "sum",
];

/// `true` se l'estensione del percorso è una di `CHECKSUM_EXTENSIONS`.
pub fn is_checksum_file(path: &Path) -> bool {
    let by_extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| CHECKSUM_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false);
    by_extension || has_checksum_file_name(path)
}

/// Riconosce i nomi convenzionali senza estensione dedicata: `SHA256SUMS`, `MD5SUMS.txt`,
/// `sha512sum.txt`, `CHECKSUMS`, `checksums.txt`, `SHA256SUMS.asc`…
fn has_checksum_file_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    let stem = lower
        .strip_suffix(".txt")
        .or_else(|| lower.strip_suffix(".asc"))
        .unwrap_or(&lower);
    if stem.contains('.') {
        return false;
    }
    stem == "checksums"
        || stem == "checksum"
        || ((stem.ends_with("sums") || stem.ends_with("sum"))
            && ["sha", "md", "crc", "b2", "b3", "blake"]
                .iter()
                .any(|p| stem.starts_with(p)))
}

/// Analizza il testo di un file di checksum.
///
/// `default_algo` (tipicamente dedotto dall'estensione) viene assegnato alle voci senza etichetta
/// esplicita quando la lunghezza dell'hash corrisponde; con `Some(Algo::Crc32)` le righe sono
/// interpretate prima come SFV (`nome hex`).
pub fn parse_text(text: &str, default_algo: Option<Algo>) -> Vec<ChecksumEntry> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut entries = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        match parse_line(line, default_algo) {
            Some(parsed) => {
                let algo = parsed
                    .algo
                    .or_else(|| default_algo.filter(|a| a.hex_len() == parsed.hex.len()));
                entries.push(ChecksumEntry {
                    algo,
                    hex: parsed.hex,
                    file_name: parsed.name,
                    line: line_no,
                });
            }
            None => log::debug!("checksum file, line {line_no}: unrecognised line {line:?}"),
        }
    }
    entries
}

/// Analizza il contenuto di un file di checksum (deduce l'algoritmo dall'estensione o dal nome
/// quando possibile). `path` è il nome o il percorso relativo del file, usato per `base_dir`.
pub fn parse_bytes(path: &Path, bytes: &[u8]) -> ChecksumFile {
    let text = decode_text(bytes);
    let default_algo = algo_from_path(path);
    let base_dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let entries = parse_text(&text, default_algo);
    ChecksumFile {
        path: path.to_path_buf(),
        base_dir,
        default_algo,
        entries,
    }
}

/// Algoritmo dall'estensione (`.sha256`) oppure, in mancanza, dal nome (`SHA256SUMS`, `MD5SUMS`).
pub fn algo_from_path(path: &Path) -> Option<Algo> {
    if let Some(algo) = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(Algo::from_checksum_extension)
    {
        return Some(algo);
    }
    let stem = path.file_stem()?.to_str()?.to_ascii_lowercase();
    let stem = stem.strip_suffix('s').unwrap_or(&stem);
    if stem.ends_with("sum") {
        Algo::from_checksum_extension(stem)
    } else {
        None
    }
}

/// Testo del file: UTF-16 se c'è il BOM corrispondente, altrimenti UTF-8 con sostituzione dei
/// byte non validi.
fn decode_text(bytes: &[u8]) -> String {
    fn utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
        let units = bytes.as_chunks::<2>().0.iter().map(|c| unit(*c));
        char::decode_utf16(units)
            .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect()
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        utf16(rest, u16::from_le_bytes)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        utf16(rest, u16::from_be_bytes)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Voce riconosciuta in una riga.
struct Parsed {
    /// Algoritmo dichiarato dall'etichetta BSD.
    algo: Option<Algo>,
    hex: String,
    name: String,
}

/// Riga già priva di spazi iniziali/finali, non vuota e non di commento.
fn parse_line(line: &str, default_algo: Option<Algo>) -> Option<Parsed> {
    // coreutils: una `\` iniziale indica che il nome contiene sequenze di escape.
    let (escaped, body) = match line.strip_prefix('\\') {
        Some(rest) => (true, rest),
        None => (false, line),
    };
    let unescape_if_needed = |mut p: Parsed| {
        if escaped {
            p.name = unescape_name(&p.name);
        }
        p
    };
    match parse_bsd(body) {
        BsdLine::Entry(p) => return Some(unescape_if_needed(p)),
        BsdLine::Invalid(reason) => {
            log::debug!("checksum file: skipping BSD-style line ({reason}): {line:?}");
            return None;
        }
        BsdLine::NoMatch => {}
    }
    if default_algo == Some(Algo::Crc32) {
        parse_sfv(line).or_else(|| parse_gnu(body).map(unescape_if_needed))
    } else {
        parse_gnu(body).map(unescape_if_needed).or_else(|| parse_sfv(line))
    }
}

enum BsdLine {
    Entry(Parsed),
    /// La riga ha la forma BSD ma non è utilizzabile (algoritmo sconosciuto, hash non valido…).
    Invalid(&'static str),
    NoMatch,
}

/// `ALGO (nome) = hex` (coreutils `--tag`, BSD `md5`/`shasum`) oppure `ALGO(nome)= hex` (OpenSSL).
fn parse_bsd(line: &str) -> BsdLine {
    let Some(open) = line.find('(') else {
        return BsdLine::NoMatch;
    };
    let label = line[..open].trim_end();
    let label_ok = !label.is_empty()
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/' | '.'));
    if !label_ok {
        return BsdLine::NoMatch;
    }
    let rest = &line[open + 1..];
    let Some(close) = rest.rfind(')') else {
        return BsdLine::NoMatch;
    };
    let name = &rest[..close];
    let Some(hex_part) = rest[close + 1..].trim_start().strip_prefix('=') else {
        return BsdLine::NoMatch;
    };
    let hex_part = hex_part.trim();
    let algo = match bsd_label_algo(label, hex_part.len()) {
        Some(algo) => algo,
        // Un'"etichetta" esadecimale è più probabilmente l'hash di una riga GNU.
        None if label.chars().all(|c| c.is_ascii_hexdigit()) => return BsdLine::NoMatch,
        None => return BsdLine::Invalid("unknown algorithm"),
    };
    if name.is_empty() {
        return BsdLine::Invalid("empty file name");
    }
    match normalize_digest(hex_part) {
        Some(hex) if hex.len() == algo.hex_len() => BsdLine::Entry(Parsed {
            algo: Some(algo),
            hex,
            name: name.to_string(),
        }),
        Some(_) => BsdLine::Invalid("digest length does not match the algorithm"),
        None => BsdLine::Invalid("invalid digest"),
    }
}

/// Etichetta BSD → algoritmo: nomi e id di `Algo`, più gli alias usati da coreutils, OpenSSL e xxhsum.
fn bsd_label_algo(label: &str, hex_len: usize) -> Option<Algo> {
    if let Some(algo) = Algo::from_label(label) {
        return Some(algo);
    }
    let l: String = label
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let algo = match l.as_str() {
        "blake2b" => Algo::Blake2b512,
        "blake2s" => Algo::Blake2s256,
        "sha2224" => Algo::Sha224,
        "sha2256" => Algo::Sha256,
        "sha2384" => Algo::Sha384,
        "sha2512" => Algo::Sha512,
        "rmd128" => Algo::Ripemd128,
        "rmd160" => Algo::Ripemd160,
        "rmd256" => Algo::Ripemd256,
        "rmd320" => Algo::Ripemd320,
        "xxh128" | "xxh3" => Algo::Xxh3_128,
        "crc" => Algo::Crc32,
        _ => return None,
    };
    // Gli alias senza lunghezza esplicita valgono solo se la lunghezza dell'hash è coerente.
    (algo.hex_len() == hex_len).then_some(algo)
}

/// `hex  nome` (testo), `hex *nome` (binario) oppure `hex nome` (spazio singolo).
fn parse_gnu(line: &str) -> Option<Parsed> {
    let split = line.find([' ', '\t'])?;
    let hex = normalize_digest(&line[..split])?;
    let rest = &line[split + 1..];
    let name = match rest.as_bytes().first() {
        Some(b' ' | b'*') => &rest[1..],
        _ => rest,
    };
    if name.is_empty() {
        return None;
    }
    Some(Parsed {
        algo: None,
        hex,
        name: name.to_string(),
    })
}

/// SFV: `nome hex`, con CRC32 di 8 cifre esadecimali in fondo alla riga.
fn parse_sfv(line: &str) -> Option<Parsed> {
    let split = line.rfind([' ', '\t'])?;
    let crc = &line[split + 1..];
    let name = line[..split].trim_end();
    if crc.len() != Algo::Crc32.hex_len() || name.is_empty() || !crc.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(Parsed {
        algo: None,
        hex: crc.to_ascii_lowercase(),
        name: name.to_string(),
    })
}

/// Hash esadecimale minuscolo, solo se ha la lunghezza di un digest di qualche algoritmo.
fn normalize_digest(s: &str) -> Option<String> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    if Algo::candidates_for_hex_len(s.len()).is_empty() {
        log::debug!("checksum file: {} hex digits is not a known digest length", s.len());
        return None;
    }
    Some(s.to_ascii_lowercase())
}

/// Escape di coreutils nei nomi: `\\`, `\n`, `\r`.
fn unescape_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut chars = name.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD5_EMPTY: &str = "d41d8cd98f00b204e9800998ecf8427e";

    #[test]
    fn gnu_variants() {
        let text = format!("{MD5_EMPTY}  a.txt\n{MD5_EMPTY} *b.bin\n{MD5_EMPTY} c d.txt\n");
        let e = parse_text(&text, Some(Algo::Md5));
        let names: Vec<_> = e.iter().map(|e| e.file_name.as_str()).collect();
        assert_eq!(names, ["a.txt", "b.bin", "c d.txt"]);
        assert!(e.iter().all(|e| e.algo == Some(Algo::Md5) && e.hex == MD5_EMPTY));
        assert_eq!(e.iter().map(|e| e.line).collect::<Vec<_>>(), [1, 2, 3]);
    }

    #[test]
    fn escaped_names() {
        let text = format!("\\{MD5_EMPTY}  a\\nb\\\\c\n");
        let e = parse_text(&text, None);
        assert_eq!(e[0].file_name, "a\nb\\c");
    }

    #[test]
    fn bsd_aliases_and_openssl() {
        let b2 = "0".repeat(128);
        let text = format!(
            "BLAKE2b (x) = {b2}\nMD5(y)= {MD5_EMPTY}\nSM3 (z) = {}\n",
            "0".repeat(64)
        );
        let e = parse_text(&text, None);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].algo, Some(Algo::Blake2b512));
        assert_eq!((e[1].algo, e[1].file_name.as_str()), (Some(Algo::Md5), "y"));
    }

    #[test]
    fn utf16_and_name_based_algo() {
        let mut bytes = vec![0xFF, 0xFE];
        for u in "abc".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode_text(&bytes), "abc");
        assert_eq!(algo_from_path(Path::new("/x/SHA256SUMS")), Some(Algo::Sha256));
        assert_eq!(algo_from_path(Path::new("MD5SUMS")), Some(Algo::Md5));
        assert_eq!(algo_from_path(Path::new("notes.txt")), None);
    }

    #[test]
    fn conventional_checksum_file_names() {
        assert!(is_checksum_file(Path::new("SHA256SUMS")));
        assert!(is_checksum_file(Path::new("/tmp/SHA256SUMS.txt")));
        assert!(is_checksum_file(Path::new("md5sums.txt")));
        assert!(is_checksum_file(Path::new("CHECKSUMS")));
        assert!(is_checksum_file(Path::new("checksums.txt")));
        assert!(is_checksum_file(Path::new("files.sha3-256")));
        assert!(!is_checksum_file(Path::new("README.txt")));
        assert!(!is_checksum_file(Path::new("my.sums.txt")));
        assert!(!is_checksum_file(Path::new("photo.jpg")));
    }
}
