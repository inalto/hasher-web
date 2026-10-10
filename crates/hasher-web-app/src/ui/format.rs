//! Formattazione localizzata di numeri, dimensioni, durate e date per l'interfaccia.

use chrono::{DateTime, Local};
use rust_i18n::t;
use std::time::Duration;

/// Separatore delle migliaia della lingua corrente.
fn thousands_sep() -> String {
    t!("app.thousands_separator").into_owned()
}

/// Separatore decimale della lingua corrente.
fn decimal_sep() -> String {
    t!("app.decimal_separator").into_owned()
}

/// Intero con separatore delle migliaia (`12.345.678`).
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let sep = thousands_sep();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 * sep.len());
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push_str(&sep);
        }
        out.push(c);
    }
    out
}

/// Numero decimale con il separatore della lingua corrente.
pub fn decimal(x: f64, decimals: usize) -> String {
    let s = format!("{x:.decimals$}");
    let sep = decimal_sep();
    if sep == "." {
        s
    } else {
        s.replace('.', &sep)
    }
}

/// Dimensione leggibile in base 10 (`12,3 MB`).
pub fn size(bytes: u64) -> String {
    let s = humansize::format_size(bytes, humansize::DECIMAL.decimal_places(1));
    let sep = decimal_sep();
    if sep == "." {
        s
    } else {
        s.replace('.', &sep)
    }
}

/// Dimensione esatta in byte (`12.345.678 byte`).
pub fn bytes_exact(bytes: u64) -> String {
    let key = plural_key("app.bytes", usize::try_from(bytes).unwrap_or(usize::MAX));
    t!(&key, n = thousands(bytes)).into_owned()
}

/// Velocità (`123,4 MB/s`).
pub fn speed(bytes_per_sec: f64) -> String {
    if !bytes_per_sec.is_finite() || bytes_per_sec <= 0.0 {
        return "—".to_owned();
    }
    format!("{}/s", size(bytes_per_sec as u64))
}

/// Durata breve (`350 ms`, `1,2 s`, `2 min 05 s`, `1 h 04 min`).
pub fn duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 1.0 {
        format!("{} ms", d.as_millis())
    } else if secs < 10.0 {
        format!("{} s", decimal(secs, 1))
    } else if secs < 60.0 {
        format!("{} s", secs.round() as u64)
    } else if secs < 3600.0 {
        let s = secs.round() as u64;
        format!("{} min {:02} s", s / 60, s % 60)
    } else {
        let m = (secs / 60.0).round() as u64;
        format!("{} h {:02} min", m / 60, m % 60)
    }
}

/// Tempo stimato rimanente (`~ 12 s`).
pub fn eta(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "—".to_owned();
    }
    let secs = secs.max(1.0);
    format!("~ {}", duration(Duration::from_secs_f64(secs.ceil())))
}

/// Data e ora nel formato della lingua corrente.
pub fn datetime(d: Option<DateTime<Local>>) -> String {
    match d {
        Some(d) => d.format(&t!("app.date_format")).to_string(),
        None => t!("app.not_available").into_owned(),
    }
}

/// Percentuale intera (`45%`).
pub fn percent(fraction: f32) -> String {
    format!("{}%", (fraction.clamp(0.0, 1.0) * 100.0).floor() as u32)
}

/// Chiave di traduzione al singolare (`<key>_one`) quando `n == 1`.
pub fn plural_key(key: &str, n: usize) -> String {
    if n == 1 {
        format!("{key}_one")
    } else {
        key.to_owned()
    }
}
