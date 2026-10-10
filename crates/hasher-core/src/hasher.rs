//! Trait `StreamHasher` e costruzione degli hasher concreti.
//!
//! Il trait è nostro (non dipendiamo dal trait `Digest` esterno) così che CRC, xxHash, BLAKE3 ed ED2K
//! si integrino allo stesso modo degli algoritmi RustCrypto.
//!
//! Rappresentazione dei digest: i checksum numerici (CRC32, CRC64, xxHash64, xxHash3-128) sono
//! restituiti in big-endian, cioè nella forma canonica usata dai file SFV, da `xz`/7-Zip e da
//! `xxhsum`; tutti gli altri nella sequenza di byte definita dall'algoritmo.

use crate::algo::Algo;
use sha2::Digest;

/// Hasher incrementale: riceve i dati a blocchi e produce il digest finale.
pub trait StreamHasher: Send {
    fn update(&mut self, data: &[u8]);
    fn finalize(self: Box<Self>) -> Vec<u8>;
}

/// Dimensione del blocco ED2K (eMule).
pub const ED2K_CHUNK: usize = 9_728_000;

/// Costruisce l'hasher per l'algoritmo dato.
pub fn build(algo: Algo) -> Box<dyn StreamHasher> {
    match algo {
        Algo::Crc32 => Box::new(Crc32(crc32fast::Hasher::new())),
        Algo::Crc64 => Box::new(Crc64(CRC64_XZ.digest())),
        Algo::Xxh64 => Box::new(Xxh64(xxhash_rust::xxh64::Xxh64::new(0))),
        Algo::Xxh3_128 => Box::new(Xxh3_128(xxhash_rust::xxh3::Xxh3Default::new())),
        Algo::Md2 => rust_crypto::<md2::Md2>(),
        Algo::Md4 => rust_crypto::<md4::Md4>(),
        Algo::Md5 => rust_crypto::<md5::Md5>(),
        Algo::Sha1 => rust_crypto::<sha1::Sha1>(),
        Algo::Sha224 => rust_crypto::<sha2::Sha224>(),
        Algo::Sha256 => rust_crypto::<sha2::Sha256>(),
        Algo::Sha384 => rust_crypto::<sha2::Sha384>(),
        Algo::Sha512 => rust_crypto::<sha2::Sha512>(),
        Algo::Sha3_256 => rust_crypto::<sha3::Sha3_256>(),
        Algo::Sha3_512 => rust_crypto::<sha3::Sha3_512>(),
        Algo::Ripemd128 => rust_crypto::<ripemd::Ripemd128>(),
        Algo::Ripemd160 => rust_crypto::<ripemd::Ripemd160>(),
        Algo::Ripemd256 => rust_crypto::<ripemd::Ripemd256>(),
        Algo::Ripemd320 => rust_crypto::<ripemd::Ripemd320>(),
        Algo::Blake2b512 => rust_crypto::<blake2::Blake2b512>(),
        Algo::Blake2s256 => rust_crypto::<blake2::Blake2s256>(),
        Algo::Blake3 => Box::new(Blake3(Box::new(blake3::Hasher::new()))),
        Algo::Ed2k => Box::new(Ed2k::new()),
    }
}

/// Comodità per test e piccoli buffer: hash di un intero slice in memoria.
pub fn hash_bytes(algo: Algo, data: &[u8]) -> Vec<u8> {
    let mut h = build(algo);
    h.update(data);
    h.finalize()
}

// --- RustCrypto (digest 0.11) ---------------------------------------------------------------

/// Adattatore generico per qualsiasi tipo che implementa `digest::Digest` (tutti i crate RustCrypto
/// in uso condividono la stessa versione del crate `digest`, riesportata da ognuno).
struct RustCrypto<D>(D);

fn rust_crypto<D: Digest + Send + 'static>() -> Box<dyn StreamHasher> {
    Box::new(RustCrypto(D::new()))
}

impl<D: Digest + Send> StreamHasher for RustCrypto<D> {
    fn update(&mut self, data: &[u8]) {
        Digest::update(&mut self.0, data);
    }
    fn finalize(self: Box<Self>) -> Vec<u8> {
        self.0.finalize().to_vec()
    }
}

// --- Checksum ---------------------------------------------------------------------------------

struct Crc32(crc32fast::Hasher);

impl StreamHasher for Crc32 {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    fn finalize(self: Box<Self>) -> Vec<u8> {
        self.0.finalize().to_be_bytes().to_vec()
    }
}

/// CRC-64/XZ (ECMA-182 riflesso, init e xorout a 1): lo stesso di `xz` e 7-Zip. Tabella "slice-by-16"
/// calcolata a tempo di compilazione.
static CRC64_XZ: crc::Crc<u64, crc::Table<16>> = crc::Crc::<u64, crc::Table<16>>::new(&crc::CRC_64_XZ);

struct Crc64(crc::Digest<'static, u64, crc::Table<16>>);

impl StreamHasher for Crc64 {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    fn finalize(self: Box<Self>) -> Vec<u8> {
        self.0.finalize().to_be_bytes().to_vec()
    }
}

/// xxHash64 con seed 0.
struct Xxh64(xxhash_rust::xxh64::Xxh64);

impl StreamHasher for Xxh64 {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    fn finalize(self: Box<Self>) -> Vec<u8> {
        self.0.digest().to_be_bytes().to_vec()
    }
}

/// XXH3 a 128 bit con seed 0 e segreto di default (forma canonica big-endian, come `xxh128sum`).
struct Xxh3_128(xxhash_rust::xxh3::Xxh3Default);

impl StreamHasher for Xxh3_128 {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    fn finalize(self: Box<Self>) -> Vec<u8> {
        self.0.digest128().to_be_bytes().to_vec()
    }
}

// --- BLAKE3 -----------------------------------------------------------------------------------

/// Lo stato di `blake3::Hasher` è grande (~2 KiB): resta in un box a parte.
struct Blake3(Box<blake3::Hasher>);

impl StreamHasher for Blake3 {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    fn finalize(self: Box<Self>) -> Vec<u8> {
        self.0.finalize().as_bytes().to_vec()
    }
}

// --- ED2K -------------------------------------------------------------------------------------

/// ED2K (eDonkey/eMule), convenzione eMule "rossa":
/// - dimensione < `ED2K_CHUNK`: digest = MD4(dati);
/// - altrimenti: digest = MD4(MD4(blocco₁) ‖ MD4(blocco₂) ‖ …), e se la dimensione è un multiplo
///   esatto di `ED2K_CHUNK` si accoda anche MD4 di un blocco vuoto prima dell'MD4 finale.
///
/// Elaborazione in streaming: nessun blocco viene bufferizzato, i digest dei blocchi completati
/// alimentano direttamente l'MD4 "radice".
struct Ed2k {
    chunk_size: usize,
    /// MD4 del blocco corrente.
    chunk: md4::Md4,
    /// Byte già immessi nel blocco corrente (sempre < `chunk_size` fra una chiamata e l'altra).
    filled: usize,
    /// MD4 della concatenazione dei digest dei blocchi completati.
    root: md4::Md4,
    /// Almeno un blocco completo è stato chiuso (dimensione totale ≥ `chunk_size`).
    multi: bool,
}

impl Ed2k {
    fn new() -> Self {
        Self::with_chunk_size(ED2K_CHUNK)
    }

    fn with_chunk_size(chunk_size: usize) -> Self {
        debug_assert!(chunk_size > 0);
        Self {
            chunk_size,
            chunk: md4::Md4::new(),
            filled: 0,
            root: md4::Md4::new(),
            multi: false,
        }
    }
}

impl StreamHasher for Ed2k {
    fn update(&mut self, mut data: &[u8]) {
        while !data.is_empty() {
            let take = (self.chunk_size - self.filled).min(data.len());
            let (head, tail) = data.split_at(take);
            Digest::update(&mut self.chunk, head);
            self.filled += take;
            data = tail;
            if self.filled == self.chunk_size {
                let digest = std::mem::take(&mut self.chunk).finalize();
                Digest::update(&mut self.root, digest);
                self.filled = 0;
                self.multi = true;
            }
        }
    }

    fn finalize(self: Box<Self>) -> Vec<u8> {
        let Ed2k { chunk, root, multi, .. } = *self;
        if !multi {
            // Meno di un blocco (anche file vuoto): semplice MD4.
            return chunk.finalize().to_vec();
        }
        // Ultimo blocco parziale oppure, se la dimensione è un multiplo esatto, il blocco vuoto
        // (l'hasher appena azzerato produce proprio MD4("")).
        let mut root = root;
        Digest::update(&mut root, chunk.finalize());
        root.finalize().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Riferimento non in streaming, con dimensione di blocco arbitraria.
    fn ed2k_reference(data: &[u8], chunk: usize) -> Vec<u8> {
        if data.len() < chunk {
            return md4::Md4::digest(data).to_vec();
        }
        let mut cat = Vec::new();
        for c in data.chunks(chunk) {
            cat.extend_from_slice(&md4::Md4::digest(c));
        }
        if data.len().is_multiple_of(chunk) {
            cat.extend_from_slice(&md4::Md4::digest(b""));
        }
        md4::Md4::digest(&cat).to_vec()
    }

    #[test]
    fn ed2k_small_chunks_match_reference() {
        const CHUNK: usize = 7;
        let data: Vec<u8> = (0..200u32).map(|i| (i * 31 + 7) as u8).collect();
        for len in [0, 1, 6, 7, 8, 13, 14, 15, 21, 22, 49, 50, 199] {
            let expected = ed2k_reference(&data[..len], CHUNK);
            for step in [1, 2, 3, 5, 7, 8, 11, 64, 1000] {
                let mut h: Box<dyn StreamHasher> = Box::new(Ed2k::with_chunk_size(CHUNK));
                for piece in data[..len].chunks(step) {
                    h.update(piece);
                }
                h.update(&[]);
                assert_eq!(h.finalize(), expected, "len={len} step={step}");
            }
        }
    }

    #[test]
    fn ed2k_empty_is_md4_of_nothing() {
        assert_eq!(
            hex::encode(hash_bytes(Algo::Ed2k, b"")),
            "31d6cfe0d16ae931b73c59d7e0c089c0"
        );
    }

    #[test]
    fn every_algo_has_declared_length() {
        for algo in Algo::ALL {
            assert_eq!(hash_bytes(algo, b"abc").len(), algo.digest_len(), "{algo}");
        }
    }

    #[test]
    fn streaming_equals_one_shot() {
        let data: Vec<u8> = (0..10_000u32).map(|i| (i ^ (i >> 3)) as u8).collect();
        for algo in Algo::ALL {
            let one = hash_bytes(algo, &data);
            let mut h = build(algo);
            for piece in data.chunks(97) {
                h.update(piece);
            }
            assert_eq!(h.finalize(), one, "{algo}");
        }
    }
}
