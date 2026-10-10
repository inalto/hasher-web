//! Parti pure del protocollo del Web Worker (docs/PIANO.md §5.3), verificate in nativo.
//!
//! Il ciclo `async` del worker (`File.slice`, `arrayBuffer()`, `postMessage`) gira solo in un
//! browser; qui si verificano le regole che usa: interpretazione dei comandi, coda dei task con
//! annullamento, cadenza degli eventi `progress`, calcolo a blocchi e codifica dei digest.

use hasher_core::algo::Algo;
use hasher_core::hasher::hash_bytes;
use hasher_core::worker_protocol::{
    chunk_size, cmd, decode_digest, digest_entries, ev, parse_algo_ids, parse_cancel_target, parse_task_id,
    CancelTarget, ProgressThrottle, TaskHasher, TaskQueue, CANCEL_ALL, DEFAULT_CHUNK, MAX_CHUNK, MIN_CHUNK,
    PROGRESS_INTERVAL_MS, UPDATE_BLOCK,
};
use std::collections::BTreeMap;

#[test]
fn command_and_event_names_match_the_plan() {
    assert_eq!((cmd::HASH, cmd::CANCEL), ("hash", "cancel"));
    assert_eq!(
        [
            ev::READY,
            ev::STARTED,
            ev::PROGRESS,
            ev::DONE,
            ev::FAILED,
            ev::CANCELLED
        ],
        ["ready", "started", "progress", "done", "failed", "cancelled"]
    );
    assert_eq!(DEFAULT_CHUNK, 4 * 1024 * 1024);
    assert_eq!(CANCEL_ALL, -1);
    const { assert!(PROGRESS_INTERVAL_MS >= 50.0, "at most 20 progress events per second") };
}

// --- Identificativi degli algoritmi -------------------------------------------------------------

#[test]
fn every_algo_id_is_accepted() {
    let ids: Vec<&str> = Algo::ALL.iter().map(|a| a.id()).collect();
    assert_eq!(parse_algo_ids(&ids).unwrap(), Algo::ALL.to_vec());
    for a in Algo::ALL {
        assert_eq!(Algo::from_id(a.id()), Some(a));
        assert_eq!(parse_algo_ids([a.id()]).unwrap(), vec![a]);
    }
}

#[test]
fn algo_ids_keep_order_drop_duplicates_and_tolerate_labels() {
    assert_eq!(
        parse_algo_ids(["sha256", "md5", "sha256", "crc32", "md5"]).unwrap(),
        vec![Algo::Sha256, Algo::Md5, Algo::Crc32]
    );
    // Ripiego su `Algo::from_label`: nomi visualizzati e varianti di maiuscole/trattini.
    assert_eq!(
        parse_algo_ids(["SHA-256", "RIPEMD160", "xxHash3-128", "BLAKE2b-512"]).unwrap(),
        vec![Algo::Sha256, Algo::Ripemd160, Algo::Xxh3_128, Algo::Blake2b512]
    );
}

#[test]
fn unknown_or_missing_algorithms_are_errors() {
    let err = parse_algo_ids(["sha256", "whirlpool"]).unwrap_err();
    assert!(err.contains("whirlpool"), "{err}");
    assert!(parse_algo_ids(Vec::<String>::new()).is_err());
    assert!(parse_algo_ids([""]).is_err());
}

// --- task e chunk -------------------------------------------------------------------------------

#[test]
fn task_ids_are_non_negative_safe_integers() {
    assert_eq!(parse_task_id(0.0), Some(0));
    assert_eq!(parse_task_id(42.0), Some(42));
    assert_eq!(parse_task_id(9_007_199_254_740_991.0), Some(9_007_199_254_740_991));
    for bad in [-1.0, -0.5, 1.5, f64::NAN, f64::INFINITY, 9_007_199_254_740_992.0, 1e300] {
        assert_eq!(parse_task_id(bad), None, "{bad}");
    }
}

#[test]
fn cancel_targets() {
    assert_eq!(parse_cancel_target(-1.0), Some(CancelTarget::All));
    assert_eq!(parse_cancel_target(7.0), Some(CancelTarget::Task(7)));
    assert_eq!(parse_cancel_target(-2.0), None);
    assert_eq!(parse_cancel_target(f64::NAN), None);
}

#[test]
fn chunk_size_defaults_and_limits() {
    assert_eq!(chunk_size(None), DEFAULT_CHUNK);
    for bad in [0.0, -5.0, 0.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(chunk_size(Some(bad)), DEFAULT_CHUNK, "{bad}");
    }
    assert_eq!(chunk_size(Some(1.0)), MIN_CHUNK);
    assert_eq!(chunk_size(Some(1e12)), MAX_CHUNK);
    assert_eq!(chunk_size(Some(1_048_576.9)), 1_048_576);
    const { assert!(MIN_CHUNK <= DEFAULT_CHUNK && DEFAULT_CHUNK <= MAX_CHUNK) };
}

// --- Coda dei task ------------------------------------------------------------------------------

#[test]
fn queue_runs_one_task_at_a_time_in_fifo_order() {
    let mut q = TaskQueue::new();
    q.push(10, "a");
    q.push(11, "b");
    q.push(12, "c");
    assert_eq!(q.start_next(), Some((10, "a")));
    assert_eq!(q.running(), Some(10));
    // Finché il task 10 è in esecuzione non ne parte un altro.
    assert_eq!(q.start_next(), None);
    q.finish();
    assert_eq!(q.start_next(), Some((11, "b")));
    q.finish();
    assert_eq!(q.start_next(), Some((12, "c")));
    q.finish();
    assert_eq!(q.start_next(), None);
    assert_eq!(q.running(), None);
}

#[test]
fn cancelling_the_running_task_raises_the_flag_only_for_it() {
    let mut q = TaskQueue::new();
    q.push(1, ());
    q.push(2, ());
    q.start_next();
    assert!(!q.is_running_cancelled());
    // Task sconosciuto o già concluso: ignorato.
    assert!(q.cancel(CancelTarget::Task(99)).is_empty());
    assert!(!q.is_running_cancelled());
    assert!(q.cancel(CancelTarget::Task(1)).is_empty());
    assert!(q.is_running_cancelled());
    // Il flag non sopravvive al task: il successivo parte pulito.
    q.finish();
    assert_eq!(q.start_next().map(|(t, _)| t), Some(2));
    assert!(!q.is_running_cancelled());
}

#[test]
fn cancelling_waiting_tasks_removes_them() {
    let mut q = TaskQueue::new();
    for t in 1..=5u64 {
        q.push(t, t * 100);
    }
    q.start_next();
    assert_eq!(q.cancel(CancelTarget::Task(3)), vec![3]);
    assert_eq!(q.waiting_len(), 3);
    assert!(!q.is_running_cancelled());
    assert_eq!(q.cancel(CancelTarget::All), vec![2, 4, 5]);
    assert_eq!(q.waiting_len(), 0);
    assert!(q.is_running_cancelled());
    q.finish();
    assert_eq!(q.start_next(), None);

    // `cancel(-1)` senza task in esecuzione non lascia flag pendenti.
    let mut idle: TaskQueue<()> = TaskQueue::new();
    assert!(idle.cancel(CancelTarget::All).is_empty());
    idle.push(8, ());
    assert_eq!(idle.start_next(), Some((8, ())));
    assert!(!idle.is_running_cancelled());
}

// --- Avanzamento --------------------------------------------------------------------------------

#[test]
fn progress_is_throttled_to_the_interval() {
    let mut t = ProgressThrottle::new(1000.0);
    assert!(!t.ready(1000.0));
    assert!(!t.ready(1049.9));
    assert!(t.ready(1050.0));
    assert!(!t.ready(1060.0));
    assert!(!t.ready(1099.0));
    assert!(t.ready(1100.0));
    // Su un secondo simulato a passi di 1 ms: al più 20 eventi.
    let mut t = ProgressThrottle::new(0.0);
    let emitted = (1..=1000).filter(|ms| t.ready(f64::from(*ms))).count();
    assert_eq!(emitted, 20);
}

// --- Calcolo e digest ---------------------------------------------------------------------------

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
fn task_hasher_in_chunks_matches_one_shot_for_every_algorithm() {
    let data = pseudo_random(600_001, 3);
    for chunk in [MIN_CHUNK, 100_000, UPDATE_BLOCK, DEFAULT_CHUNK] {
        let mut h = TaskHasher::new(&Algo::ALL);
        for piece in data.chunks(chunk) {
            h.update(piece);
        }
        assert_eq!(h.bytes(), data.len() as u64);
        let out = h.finalize();
        assert_eq!(out.iter().map(|(a, _)| *a).collect::<Vec<_>>(), Algo::ALL.to_vec());
        for (algo, digest) in out {
            assert_eq!(digest, hash_bytes(algo, &data), "{algo} chunk {chunk}");
        }
    }
}

#[test]
fn empty_file_yields_the_empty_digests() {
    let h = TaskHasher::new(&[Algo::Md5, Algo::Crc32, Algo::Ed2k]);
    assert_eq!(h.bytes(), 0);
    let hex = digest_entries(&h.finalize());
    assert_eq!(
        hex,
        vec![
            ("md5", "d41d8cd98f00b204e9800998ecf8427e".to_string()),
            ("crc32", "00000000".to_string()),
            ("ed2k", "31d6cfe0d16ae931b73c59d7e0c089c0".to_string()),
        ]
    );
}

#[test]
fn digest_entries_are_lowercase_and_keyed_by_id() {
    let mut h = TaskHasher::new(&[Algo::Sha256, Algo::Crc64, Algo::Xxh64, Algo::Xxh3_128]);
    h.update(b"ab");
    h.update(b"c");
    let entries = digest_entries(&h.finalize());
    assert_eq!(
        entries,
        vec![
            (
                "sha256",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_string()
            ),
            // Checksum numerici in big-endian (forma canonica di xz/7-Zip e xxhsum).
            ("crc64", "2cd8094a1a277627".to_string()),
            ("xxh64", "44bc2cf5ad770999".to_string()),
            ("xxh3-128", "06b05ab6733a618578af5f94892f3950".to_string()),
        ]
    );
    for (_, hex) in &entries {
        assert_eq!(*hex, hex.to_ascii_lowercase());
    }
}

#[test]
fn digests_roundtrip_through_decode() {
    let data = pseudo_random(10_000, 9);
    let mut h = TaskHasher::new(&Algo::ALL);
    h.update(&data);
    let raw = h.finalize();
    let decoded: BTreeMap<Algo, Vec<u8>> = digest_entries(&raw)
        .iter()
        .map(|(id, hex)| decode_digest(id, hex).unwrap_or_else(|| panic!("{id}")))
        .collect();
    assert_eq!(decoded, raw.into_iter().collect::<BTreeMap<_, _>>());

    // Maiuscolo accettato; lunghezza sbagliata, esadecimale non valido, id sconosciuto rifiutati.
    assert_eq!(
        decode_digest("crc32", "352441C2"),
        Some((Algo::Crc32, vec![0x35, 0x24, 0x41, 0xc2]))
    );
    assert_eq!(decode_digest("crc32", "352441"), None);
    assert_eq!(decode_digest("crc32", "zz2441c2"), None);
    assert_eq!(decode_digest("CRC32", "352441c2"), None);
    assert_eq!(decode_digest("whirlpool", "00"), None);
}
