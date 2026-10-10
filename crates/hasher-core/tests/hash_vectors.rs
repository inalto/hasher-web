//! Vettori noti per tutti gli algoritmi, ED2K su più blocchi e confronto con gli strumenti di sistema.
//!
//! Portati dai test d'integrazione dell'app desktop (`hasher/tests/hash_vectors.rs`) con gli stessi
//! valori attesi; la lettura da disco (`engine::hash_file`) è sostituita dal calcolo a blocchi di
//! [`TaskHasher`], lo stesso usato dal Web Worker, e gli strumenti di sistema ricevono i dati da
//! standard input (nessun file temporaneo).
//!
//! Provenienza dei valori attesi:
//! - MD5, SHA-1/2/3, RIPEMD-160, BLAKE2b/2s, CRC32: Python `hashlib` / `zlib`;
//! - MD2, MD4, RIPEMD-128/256/320: RFC 1319/1320 e vettori ufficiali di RIPEMD (gli stessi dei file
//!   KAT dei crate RustCrypto); MD4 verificato anche con `openssl dgst -md4`;
//! - CRC-64/XZ: valore "check" del catalogo CRC e implementazione bit a bit indipendente in Python;
//! - BLAKE3, xxHash64, XXH3-128: implementazioni di riferimento indipendenti in Python (scritte dalla
//!   specifica), coincidenti con i valori pubblicati per l'input vuoto.
//!
//! I checksum numerici (CRC32, CRC64, xxHash64, XXH3-128) sono in big-endian, la forma canonica di
//! SFV, `xz`/7-Zip e `xxhsum`.

use hasher_core::algo::Algo;
use hasher_core::hasher::{build, hash_bytes, ED2K_CHUNK};
use hasher_core::worker_protocol::{TaskHasher, DEFAULT_CHUNK};
use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

fn hex_of(algo: Algo, data: &[u8]) -> String {
    hex::encode(hash_bytes(algo, data))
}

/// (algoritmo, hash di "", hash di "abc")
const VECTORS: &[(Algo, &str, &str)] = &[
    (Algo::Crc32, "00000000", "352441c2"),
    (Algo::Crc64, "0000000000000000", "2cd8094a1a277627"),
    (Algo::Xxh64, "ef46db3751d8e999", "44bc2cf5ad770999"),
    (Algo::Xxh3_128, "99aa06d3014798d86001c324468d497f", "06b05ab6733a618578af5f94892f3950"),
    (Algo::Md2, "8350e5a3e24c153df2275c9f80692773", "da853b0d3f88d99b30283a69e6ded6bb"),
    (Algo::Md4, "31d6cfe0d16ae931b73c59d7e0c089c0", "a448017aaf21d8525fc10ae87aa6729d"),
    (Algo::Md5, "d41d8cd98f00b204e9800998ecf8427e", "900150983cd24fb0d6963f7d28e17f72"),
    (Algo::Sha1, "da39a3ee5e6b4b0d3255bfef95601890afd80709", "a9993e364706816aba3e25717850c26c9cd0d89d"),
    (
        Algo::Sha224,
        "d14a028c2a3a2bc9476102bb288234c415a2b01f828ea62ac5b3e42f",
        "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7",
    ),
    (
        Algo::Sha256,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    ),
    (
        Algo::Sha384,
        "38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da274edebfe76f65fbd51ad2f14898b95b",
        "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7",
    ),
    (
        Algo::Sha512,
        "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e",
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
    ),
    (
        Algo::Sha3_256,
        "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a",
        "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532",
    ),
    (
        Algo::Sha3_512,
        "a69f73cca23a9ac5c8b567dc185a756e97c982164fe25859e0d1dcc1475c80a615b2123af1f5f94c11e3e9402c3ac558f500199d95b6d3e301758586281dcd26",
        "b751850b1a57168a5693cd924b6b096e08f621827444f70d884f5d0240d2712e10e116e9192af3c91a7ec57647e3934057340b4cf408d5a56592f8274eec53f0",
    ),
    (Algo::Ripemd128, "cdf26213a150dc3ecb610f18f6b38b46", "c14a12199c66e4ba84636b0f69144c77"),
    (Algo::Ripemd160, "9c1185a5c5e9fc54612808977ee8f548b2258d31", "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc"),
    (
        Algo::Ripemd256,
        "02ba4c4e5f8ecd1877fc52d64d30e37a2d9774fb1e5d026380ae0168e3c5522d",
        "afbd6e228b9d8cbbcef5ca2d03e6dba10ac0bc7dcbe4680e1e42d2e975459b65",
    ),
    (
        Algo::Ripemd320,
        "22d65d5661536cdc75c1fdf5c6de7b41b9f27325ebc61e8557177d705a0ec880151c3a32a00899b8",
        "de4c01b3054f8930a79d09ae738e92301e5a17085beffdc1b8d116713e74f82fa942d64cdbc4682d",
    ),
    (
        Algo::Blake2b512,
        "786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce",
        "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923",
    ),
    (
        Algo::Blake2s256,
        "69217a3079908094e11121d042354a7c1f55b6482ca1a51e1b250dfd1ed0eef9",
        "508c5e8c327c14e2e1a72ba34eeb452f37458b209ed63a294d999b4c86675982",
    ),
    (
        Algo::Blake3,
        "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262",
        "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85",
    ),
    (Algo::Ed2k, "31d6cfe0d16ae931b73c59d7e0c089c0", "a448017aaf21d8525fc10ae87aa6729d"),
];

#[test]
fn vectors_cover_every_algorithm() {
    for algo in Algo::ALL {
        assert!(VECTORS.iter().any(|(a, _, _)| *a == algo), "missing vectors for {algo}");
    }
}

#[test]
fn known_answers_empty_and_abc() {
    for &(algo, empty, abc) in VECTORS {
        assert_eq!(hex_of(algo, b""), empty, "{algo}(\"\")");
        assert_eq!(hex_of(algo, b"abc"), abc, "{algo}(\"abc\")");
        assert_eq!(empty.len(), algo.hex_len(), "{algo}: vector length");
    }
}

#[test]
fn known_answers_123456789() {
    // Valori "check" dei cataloghi CRC e Python hashlib / riferimento indipendente.
    let expected = [
        (Algo::Crc32, "cbf43926"),
        (Algo::Crc64, "995dc9bbdf1939fa"),
        (Algo::Xxh64, "8cb841db40e6ae83"),
        (Algo::Md5, "25f9e794323b453885f5181f1b624d0b"),
        (Algo::Sha1, "f7c3bc1d808e04732adf679965ccc34ca7ae3441"),
        (
            Algo::Sha256,
            "15e2b0d3c33891ebb0f1ef609ec419420c20e320ce94c65fbc8c3312448eb225",
        ),
        (Algo::Ripemd160, "d3d0379126c1e5e0ba70ad6e5e53ff6aeab9f4fa"),
        (
            Algo::Blake2s256,
            "7acc2dd21a2909140507f37396acce906864b5f118dfa766b107962b7a82a0d4",
        ),
        (
            Algo::Blake3,
            "b7d65b48420d1033cb2595293263b6f72eabee20d55e699d0df1973b3c9deed1",
        ),
    ];
    for (algo, hex) in expected {
        assert_eq!(hex_of(algo, b"123456789"), hex, "{algo}(\"123456789\")");
    }
}

#[test]
fn more_published_vectors() {
    let fox = b"The quick brown fox jumps over the lazy dog";
    assert_eq!(hex_of(Algo::Md2, fox), "03d85a0d629d2c442e987525319fc471");
    assert_eq!(hex_of(Algo::Md4, b"message digest"), "d9130a8164549fe818874806e1c7014b");
    assert_eq!(
        hex_of(Algo::Ripemd128, b"message digest"),
        "9e327b3d6e523062afc1132d7df9d1b8"
    );
    assert_eq!(
        hex_of(Algo::Ripemd320, b"message digest"),
        "3a8e28502ed45d422f68844f9dd316e7b98533fa3f2a91d29f84d425c88d6b4eff727df66a7c0197"
    );
    assert_eq!(hex_of(Algo::Ripemd160, fox), "37f332f68db77bd9d7edd4969571ad671cf9dd3b");
}

/// Dati pseudo-casuali deterministici (xorshift64*).
fn pseudo_random(len: usize, mut seed: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 8);
    while out.len() < len {
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        out.extend_from_slice(&seed.wrapping_mul(0x2545_f491_4f6c_dd1d).to_le_bytes());
    }
    out.truncate(len);
    out
}

#[test]
fn checksums_match_one_shot_functions_of_their_crates() {
    let data = pseudo_random(3_000_017, 7);
    assert_eq!(hash_bytes(Algo::Crc32, &data), crc32fast::hash(&data).to_be_bytes());
    assert_eq!(
        hash_bytes(Algo::Xxh64, &data),
        xxhash_rust::xxh64::xxh64(&data, 0).to_be_bytes()
    );
    assert_eq!(
        hash_bytes(Algo::Xxh3_128, &data),
        xxhash_rust::xxh3::xxh3_128(&data).to_be_bytes()
    );
    assert_eq!(hash_bytes(Algo::Blake3, &data), blake3::hash(&data).as_bytes());
    let crc64 = crc::Crc::<u64>::new(&crc::CRC_64_XZ).checksum(&data);
    assert_eq!(hash_bytes(Algo::Crc64, &data), crc64.to_be_bytes());
}

#[test]
fn streaming_with_odd_splits_matches_one_shot() {
    let data = pseudo_random(1_000_000, 11);
    for algo in Algo::ALL {
        let expected = hash_bytes(algo, &data);
        for step in [63usize, 64, 65, 4093, 65_537] {
            let mut h = build(algo);
            for piece in data.chunks(step) {
                h.update(piece);
            }
            assert_eq!(h.finalize(), expected, "{algo} step {step}");
        }
        // Un byte alla volta (su un prefisso, MD2 è lento).
        let mut h = build(algo);
        for b in &data[..5000] {
            h.update(std::slice::from_ref(b));
        }
        assert_eq!(h.finalize(), hash_bytes(algo, &data[..5000]), "{algo} byte by byte");
    }
}

// --- ED2K -------------------------------------------------------------------------------------

/// Riferimento ED2K non in streaming (convenzione eMule "rossa"), costruito direttamente su `md4`.
fn ed2k_reference(data: &[u8]) -> Vec<u8> {
    use md4::{Digest, Md4};
    if data.len() < ED2K_CHUNK {
        return Md4::digest(data).to_vec();
    }
    let mut cat = Vec::new();
    for chunk in data.chunks(ED2K_CHUNK) {
        cat.extend_from_slice(&Md4::digest(chunk));
    }
    if data.len().is_multiple_of(ED2K_CHUNK) {
        cat.extend_from_slice(&Md4::digest(b""));
    }
    Md4::digest(&cat).to_vec()
}

#[test]
fn ed2k_matches_reference_at_chunk_boundaries() {
    let data = pseudo_random(2 * ED2K_CHUNK + 5, 42);
    let sizes = [
        0,
        1,
        100,
        ED2K_CHUNK - 1,
        ED2K_CHUNK,
        ED2K_CHUNK + 1,
        2 * ED2K_CHUNK,
        2 * ED2K_CHUNK + 5,
    ];
    for size in sizes {
        let slice = &data[..size];
        let expected = ed2k_reference(slice);
        // Tutto in una volta.
        assert_eq!(hash_bytes(Algo::Ed2k, slice), expected, "size {size}, one shot");
        // A pezzi dispari che scavalcano i confini dei blocchi.
        let mut h = build(Algo::Ed2k);
        for piece in slice.chunks(1_000_003) {
            h.update(piece);
        }
        assert_eq!(h.finalize(), expected, "size {size}, 1000003-byte pieces");
    }
    // Per dimensioni inferiori a un blocco ED2K coincide con MD4.
    assert_eq!(
        hash_bytes(Algo::Ed2k, &data[..100]),
        hash_bytes(Algo::Md4, &data[..100])
    );
    // Un blocco esatto non è l'MD4 dei dati (si accoda il blocco vuoto).
    assert_ne!(
        hash_bytes(Algo::Ed2k, &data[..ED2K_CHUNK]),
        hash_bytes(Algo::Md4, &data[..ED2K_CHUNK])
    );
}

/// Come il Web Worker: [`TaskHasher`] alimentato a blocchi di `chunk` byte.
fn hash_in_chunks(algos: &[Algo], data: &[u8], chunk: usize) -> BTreeMap<Algo, Vec<u8>> {
    let mut hasher = TaskHasher::new(algos);
    for piece in data.chunks(chunk) {
        hasher.update(piece);
    }
    assert_eq!(hasher.bytes(), data.len() as u64);
    hasher.finalize().into_iter().collect()
}

/// Sostituisce `ed2k_via_hash_file_with_1mib_reads` del desktop: letture da 1 MiB e da 4 MiB (il
/// blocco di default del worker, che non divide `ED2K_CHUNK`).
#[test]
fn ed2k_via_task_hasher_with_1mib_and_default_reads() {
    let data = pseudo_random(ED2K_CHUNK + 1, 5);
    for chunk in [1024 * 1024, DEFAULT_CHUNK] {
        let out = hash_in_chunks(&[Algo::Ed2k, Algo::Md4], &data, chunk);
        assert_eq!(out[&Algo::Ed2k], ed2k_reference(&data), "chunk {chunk}");
        assert_eq!(out[&Algo::Md4], hash_bytes(Algo::Md4, &data), "chunk {chunk}");
    }
}

// --- Strumenti di sistema --------------------------------------------------------------------

/// Esegue un comando passando `data` su standard input e restituisce il primo campo della prima
/// riga di output, se lo strumento è disponibile.
fn tool_digest(program: &str, args: &[&str], data: &[u8]) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // L'output è minuscolo: si può scrivere tutto l'input prima di leggerlo. Se lo strumento
    // termina in anticipo (opzione non supportata) la scrittura fallisce e il confronto si salta.
    let wrote = child
        .stdin
        .take()
        .map(|mut s| s.write_all(data).is_ok())
        .unwrap_or(false);
    let output = child.wait_with_output().ok()?;
    if !wrote || !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let line = text.lines().next()?;
    // `openssl dgst -r` stampa "hash *stdin"; coreutils "hash  -".
    Some(line.split_whitespace().next()?.to_ascii_lowercase())
}

#[test]
fn matches_system_tools_on_random_data() {
    let data = pseudo_random(3 * 1024 * 1024 + 123, 1234);
    let ours = hash_in_chunks(&Algo::ALL, &data, 1024 * 1024);
    let ours_hex = |a: Algo| hex::encode(&ours[&a]);

    let mut checked = Vec::new();
    let coreutils = [
        ("md5sum", Algo::Md5),
        ("sha1sum", Algo::Sha1),
        ("sha224sum", Algo::Sha224),
        ("sha256sum", Algo::Sha256),
        ("sha384sum", Algo::Sha384),
        ("sha512sum", Algo::Sha512),
        ("b2sum", Algo::Blake2b512),
    ];
    for (tool, algo) in coreutils {
        if let Some(theirs) = tool_digest(tool, &[], &data) {
            assert_eq!(ours_hex(algo), theirs, "{algo} vs {tool}");
            checked.push(tool.to_string());
        }
    }

    // Python: zlib.crc32 e gli algoritmi di hashlib.
    let py = "import sys, zlib, hashlib\n\
              d = sys.stdin.buffer.read()\n\
              print('crc32', '%08x' % zlib.crc32(d))\n\
              for n in ['sha3_256', 'sha3_512', 'blake2s', 'blake2b', 'ripemd160', 'md5']:\n\
              \x20   try:\n\
              \x20       print(n, hashlib.new(n, d).hexdigest())\n\
              \x20   except Exception:\n\
              \x20       pass\n";
    if let Ok(mut child) = Command::new("python3")
        .arg("-I")
        .arg("-c")
        .arg(py)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        let wrote = child
            .stdin
            .take()
            .map(|mut s| s.write_all(&data).is_ok())
            .unwrap_or(false);
        if let Ok(out) = child.wait_with_output() {
            if wrote && out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let mut it = line.split_whitespace();
                    let (Some(name), Some(value)) = (it.next(), it.next()) else {
                        continue;
                    };
                    let algo = match name {
                        "crc32" => Algo::Crc32,
                        "sha3_256" => Algo::Sha3_256,
                        "sha3_512" => Algo::Sha3_512,
                        "blake2s" => Algo::Blake2s256,
                        "blake2b" => Algo::Blake2b512,
                        "ripemd160" => Algo::Ripemd160,
                        "md5" => Algo::Md5,
                        _ => continue,
                    };
                    assert_eq!(ours_hex(algo), value, "{algo} vs python {name}");
                    checked.push(format!("python:{name}"));
                }
            }
        }
    }

    // OpenSSL (MD4 richiede il provider "legacy", può mancare).
    let openssl = [
        (
            &["dgst", "-r", "-md4", "-provider", "legacy", "-provider", "default"][..],
            Algo::Md4,
        ),
        (
            &[
                "dgst",
                "-r",
                "-ripemd160",
                "-provider",
                "legacy",
                "-provider",
                "default",
            ][..],
            Algo::Ripemd160,
        ),
        (&["dgst", "-r", "-sha3-512"][..], Algo::Sha3_512),
    ];
    for (args, algo) in openssl {
        if let Some(theirs) = tool_digest("openssl", args, &data) {
            if theirs.len() == algo.hex_len() {
                assert_eq!(ours_hex(algo), theirs, "{algo} vs openssl");
                checked.push(format!("openssl:{algo}"));
            }
        }
    }

    eprintln!("cross-checked against: {}", checked.join(", "));
}
