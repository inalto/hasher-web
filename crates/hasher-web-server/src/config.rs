//! Configurazione da variabili d'ambiente.

/// Indirizzo di ascolto predefinito.
pub const DEFAULT_BIND: &str = "0.0.0.0:8080";

/// Configurazione del server, letta da `HASHER_WEB_BIND` e `HASHER_WEB_PREFIX`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Indirizzo `host:porta` su cui ascoltare (`HASHER_WEB_BIND`, predefinito `0.0.0.0:8080`).
    pub bind: String,
    /// Prefisso di percorso normalizzato: stringa vuota per la radice, altrimenti `/hasher`
    /// (inizia con `/`, nessuno `/` finale). Da `HASHER_WEB_PREFIX`.
    pub prefix: String,
}

impl Config {
    /// Legge la configurazione dall'ambiente del processo.
    pub fn from_env() -> Result<Self, String> {
        Self::from_values(
            std::env::var("HASHER_WEB_BIND").ok(),
            std::env::var("HASHER_WEB_PREFIX").ok(),
        )
    }

    /// Costruisce la configurazione da valori già letti (testabile senza toccare l'ambiente).
    pub fn from_values(bind: Option<String>, prefix: Option<String>) -> Result<Self, String> {
        let bind = bind.map(|b| b.trim().to_owned()).filter(|b| !b.is_empty());
        let prefix = normalize_prefix(prefix.as_deref().unwrap_or("/"))?;
        Ok(Self {
            bind: bind.unwrap_or_else(|| DEFAULT_BIND.to_owned()),
            prefix,
        })
    }
}

/// Normalizza il prefisso di percorso: `""`, `"/"` → `""` (radice); `"hasher"`, `"/hasher/"` → `"/hasher"`.
///
/// Sono ammessi solo segmenti con `A-Z a-z 0-9 . _ ~ -` che non inizino con un punto (niente `..`).
pub fn normalize_prefix(raw: &str) -> Result<String, String> {
    let mut out = String::new();
    for segment in raw.trim().split('/').filter(|s| !s.is_empty()) {
        let valid = !segment.starts_with('.')
            && segment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'-'));
        if !valid {
            return Err(format!(
                "HASHER_WEB_PREFIX non valido ({raw:?}): sono ammessi solo segmenti di lettere, cifre, '.', '_', '~' e '-'"
            ));
        }
        out.push('/');
        out.push_str(segment);
    }
    Ok(out)
}
