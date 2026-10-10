//! Catalogo degli algoritmi di hash supportati. Contratto condiviso fra motore, UI ed export.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Algoritmi supportati. L'ordine di dichiarazione è l'ordine di presentazione nell'interfaccia.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Algo {
    Crc32,
    Crc64,
    Xxh64,
    Xxh3_128,
    Md2,
    Md4,
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
    Sha3_256,
    Sha3_512,
    Ripemd128,
    Ripemd160,
    Ripemd256,
    Ripemd320,
    Blake2b512,
    Blake2s256,
    Blake3,
    Ed2k,
}

/// Raggruppamento logico usato dall'interfaccia (impostazioni, guida).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlgoGroup {
    Checksum,
    Md,
    Sha,
    Ripemd,
    Blake,
    P2p,
}

impl Algo {
    /// Tutti gli algoritmi, nell'ordine di presentazione.
    pub const ALL: [Algo; 22] = [
        Algo::Crc32,
        Algo::Crc64,
        Algo::Xxh64,
        Algo::Xxh3_128,
        Algo::Md2,
        Algo::Md4,
        Algo::Md5,
        Algo::Sha1,
        Algo::Sha224,
        Algo::Sha256,
        Algo::Sha384,
        Algo::Sha512,
        Algo::Sha3_256,
        Algo::Sha3_512,
        Algo::Ripemd128,
        Algo::Ripemd160,
        Algo::Ripemd256,
        Algo::Ripemd320,
        Algo::Blake2b512,
        Algo::Blake2s256,
        Algo::Blake3,
        Algo::Ed2k,
    ];

    /// Selezione attiva di default: quelli dell'applicazione di riferimento, tranne MD2.
    /// MD2 è escluso perché intrinsecamente seriale (~7-10 MB/s) e rallenterebbe tutto il calcolo;
    /// resta attivabile dalle impostazioni (vedi `is_slow`).
    pub const DEFAULT: [Algo; 9] = [
        Algo::Crc32,
        Algo::Md4,
        Algo::Md5,
        Algo::Sha1,
        Algo::Sha256,
        Algo::Sha512,
        Algo::Ripemd128,
        Algo::Ripemd160,
        Algo::Ed2k,
    ];

    /// Nome da mostrare all'utente (non tradotto: sono nomi propri).
    pub fn name(self) -> &'static str {
        match self {
            Algo::Crc32 => "CRC32",
            Algo::Crc64 => "CRC64",
            Algo::Xxh64 => "xxHash64",
            Algo::Xxh3_128 => "xxHash3-128",
            Algo::Md2 => "MD2",
            Algo::Md4 => "MD4",
            Algo::Md5 => "MD5",
            Algo::Sha1 => "SHA-1",
            Algo::Sha224 => "SHA-224",
            Algo::Sha256 => "SHA-256",
            Algo::Sha384 => "SHA-384",
            Algo::Sha512 => "SHA-512",
            Algo::Sha3_256 => "SHA3-256",
            Algo::Sha3_512 => "SHA3-512",
            Algo::Ripemd128 => "RIPEMD-128",
            Algo::Ripemd160 => "RIPEMD-160",
            Algo::Ripemd256 => "RIPEMD-256",
            Algo::Ripemd320 => "RIPEMD-320",
            Algo::Blake2b512 => "BLAKE2b-512",
            Algo::Blake2s256 => "BLAKE2s-256",
            Algo::Blake3 => "BLAKE3",
            Algo::Ed2k => "ED2K",
        }
    }

    /// Identificatore stabile, minuscolo, senza spazi (file di configurazione, CLI, export).
    pub fn id(self) -> &'static str {
        match self {
            Algo::Crc32 => "crc32",
            Algo::Crc64 => "crc64",
            Algo::Xxh64 => "xxh64",
            Algo::Xxh3_128 => "xxh3-128",
            Algo::Md2 => "md2",
            Algo::Md4 => "md4",
            Algo::Md5 => "md5",
            Algo::Sha1 => "sha1",
            Algo::Sha224 => "sha224",
            Algo::Sha256 => "sha256",
            Algo::Sha384 => "sha384",
            Algo::Sha512 => "sha512",
            Algo::Sha3_256 => "sha3-256",
            Algo::Sha3_512 => "sha3-512",
            Algo::Ripemd128 => "ripemd128",
            Algo::Ripemd160 => "ripemd160",
            Algo::Ripemd256 => "ripemd256",
            Algo::Ripemd320 => "ripemd320",
            Algo::Blake2b512 => "blake2b-512",
            Algo::Blake2s256 => "blake2s-256",
            Algo::Blake3 => "blake3",
            Algo::Ed2k => "ed2k",
        }
    }

    /// `true` per gli algoritmi molto lenti (da segnalare nell'interfaccia).
    pub fn is_slow(self) -> bool {
        matches!(self, Algo::Md2)
    }

    /// Lunghezza del digest in byte.
    pub fn digest_len(self) -> usize {
        match self {
            Algo::Crc32 => 4,
            Algo::Crc64 | Algo::Xxh64 => 8,
            Algo::Xxh3_128 | Algo::Md2 | Algo::Md4 | Algo::Md5 | Algo::Ripemd128 | Algo::Ed2k => 16,
            Algo::Sha1 | Algo::Ripemd160 => 20,
            Algo::Sha224 => 28,
            Algo::Sha256 | Algo::Sha3_256 | Algo::Ripemd256 | Algo::Blake2s256 | Algo::Blake3 => 32,
            Algo::Ripemd320 => 40,
            Algo::Sha384 => 48,
            Algo::Sha512 | Algo::Sha3_512 | Algo::Blake2b512 => 64,
        }
    }

    /// Lunghezza della rappresentazione esadecimale.
    pub fn hex_len(self) -> usize {
        self.digest_len() * 2
    }

    pub fn group(self) -> AlgoGroup {
        match self {
            Algo::Crc32 | Algo::Crc64 | Algo::Xxh64 | Algo::Xxh3_128 => AlgoGroup::Checksum,
            Algo::Md2 | Algo::Md4 | Algo::Md5 => AlgoGroup::Md,
            Algo::Sha1
            | Algo::Sha224
            | Algo::Sha256
            | Algo::Sha384
            | Algo::Sha512
            | Algo::Sha3_256
            | Algo::Sha3_512 => AlgoGroup::Sha,
            Algo::Ripemd128 | Algo::Ripemd160 | Algo::Ripemd256 | Algo::Ripemd320 => AlgoGroup::Ripemd,
            Algo::Blake2b512 | Algo::Blake2s256 | Algo::Blake3 => AlgoGroup::Blake,
            Algo::Ed2k => AlgoGroup::P2p,
        }
    }

    /// Algoritmi il cui digest esadecimale ha la lunghezza data (riconoscimento di un hash incollato).
    pub fn candidates_for_hex_len(len: usize) -> Vec<Algo> {
        Algo::ALL.iter().copied().filter(|a| a.hex_len() == len).collect()
    }

    /// Algoritmo associato all'estensione di un file di checksum (`.md5`, `.sha256`, `.sfv`…).
    pub fn from_checksum_extension(ext: &str) -> Option<Algo> {
        match ext.to_ascii_lowercase().as_str() {
            "sfv" | "crc32" | "crc" => Some(Algo::Crc32),
            "md5" | "md5sum" => Some(Algo::Md5),
            "md4" => Some(Algo::Md4),
            "sha" | "sha1" | "sha1sum" => Some(Algo::Sha1),
            "sha224" => Some(Algo::Sha224),
            "sha256" | "sha256sum" => Some(Algo::Sha256),
            "sha384" => Some(Algo::Sha384),
            "sha512" | "sha512sum" => Some(Algo::Sha512),
            "sha3" | "sha3-256" => Some(Algo::Sha3_256),
            "sha3-512" => Some(Algo::Sha3_512),
            "b2" | "b2sum" | "blake2" | "blake2b" => Some(Algo::Blake2b512),
            "blake3" | "b3" | "b3sum" => Some(Algo::Blake3),
            "ed2k" => Some(Algo::Ed2k),
            _ => None,
        }
    }

    /// Nome come appare nei file checksum in formato BSD (`SHA256 (file) = …`) e come `id` alternativo.
    pub fn from_label(label: &str) -> Option<Algo> {
        let l: String = label
            .to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        Algo::ALL.iter().copied().find(|a| {
            let id: String = a.id().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
            let name: String = a
                .name()
                .to_ascii_lowercase()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect();
            l == id || l == name
        })
    }

    /// Estensione consigliata per l'esportazione in formato checksum.
    pub fn checksum_extension(self) -> &'static str {
        match self {
            Algo::Crc32 => "sfv",
            Algo::Md5 => "md5",
            Algo::Sha1 => "sha1",
            Algo::Sha256 => "sha256",
            Algo::Sha512 => "sha512",
            Algo::Blake2b512 => "b2",
            Algo::Blake3 => "blake3",
            _ => "txt",
        }
    }
}

impl fmt::Display for Algo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Converte un digest in esadecimale, minuscolo o maiuscolo.
pub fn to_hex(bytes: &[u8], uppercase: bool) -> String {
    if uppercase {
        hex::encode_upper(bytes)
    } else {
        hex::encode(bytes)
    }
}

/// Normalizza un hash incollato dall'utente: rimuove spazi, due punti, trattini e prefissi `0x`,
/// e lo porta in minuscolo. Restituisce `None` se il risultato non è esadecimale.
pub fn normalize_hex(input: &str) -> Option<String> {
    let mut s: String = input.trim().to_ascii_lowercase();
    if let Some(rest) = s.strip_prefix("0x") {
        s = rest.to_string();
    }
    let cleaned: String = s
        .chars()
        .filter(|c| !matches!(c, ' ' | ':' | '-' | '\t' | '\n' | '\r'))
        .collect();
    if cleaned.is_empty() || !cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_contains_defaults() {
        for a in Algo::DEFAULT {
            assert!(Algo::ALL.contains(&a));
        }
    }

    #[test]
    fn hex_len_candidates() {
        assert!(Algo::candidates_for_hex_len(64).contains(&Algo::Sha256));
        assert!(Algo::candidates_for_hex_len(8) == vec![Algo::Crc32]);
    }

    #[test]
    fn normalize() {
        assert_eq!(normalize_hex(" 0xAB:cd-12 ").as_deref(), Some("abcd12"));
        assert!(normalize_hex("xyz").is_none());
    }

    #[test]
    fn labels() {
        assert_eq!(Algo::from_label("SHA256"), Some(Algo::Sha256));
        assert_eq!(Algo::from_label("sha-256"), Some(Algo::Sha256));
        assert_eq!(Algo::from_label("RIPEMD160"), Some(Algo::Ripemd160));
    }

    #[test]
    fn serde_roundtrip() {
        let s = serde_json::to_string(&Algo::Sha3_256).unwrap();
        assert_eq!(s, "\"sha3-256\"");
        assert_eq!(serde_json::from_str::<Algo>(&s).unwrap(), Algo::Sha3_256);
    }
}
