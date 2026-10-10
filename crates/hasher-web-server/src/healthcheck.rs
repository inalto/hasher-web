//! Client HTTP minimale per `hasher-web-server --healthcheck`.
//!
//! L'immagine è distroless (nessuna shell, nessun `curl`): l'`HEALTHCHECK` esegue il binario stesso,
//! che apre un `TcpStream`, invia `GET <prefisso>/healthz` e verifica il `200`. Niente reqwest.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(3);

/// Interroga `/healthz` del server in ascolto su `bind` (con l'eventuale `prefix` già normalizzato).
///
/// Un indirizzo "qualunque" (`0.0.0.0`, `[::]`) viene sostituito dal loopback corrispondente.
pub fn healthcheck(bind: &str, prefix: &str) -> Result<(), String> {
    let addr = target_addr(bind)?;
    let mut stream =
        TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| format!("connessione a {addr} non riuscita: {e}"))?;
    stream.set_read_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;

    let request = format!(
        "GET {prefix}/healthz HTTP/1.1\r\nHost: {addr}\r\nUser-Agent: hasher-web-healthcheck\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("invio della richiesta non riuscito: {e}"))?;

    // La risposta è di pochi byte: basta la riga di stato (limite di sicurezza a 16 KiB).
    let mut response = Vec::new();
    stream
        .take(16 * 1024)
        .read_to_end(&mut response)
        .map_err(|e| format!("lettura della risposta non riuscita: {e}"))?;
    let text = String::from_utf8_lossy(&response);
    let status_line = text.lines().next().unwrap_or_default();
    match status_line.split_whitespace().nth(1) {
        Some("200") => Ok(()),
        _ => Err(format!("risposta inattesa da {addr}: {status_line:?}")),
    }
}

fn target_addr(bind: &str) -> Result<SocketAddr, String> {
    let mut addr = match bind.parse::<SocketAddr>() {
        Ok(addr) => addr,
        // Nome host (es. `localhost:8080`): si usa il primo indirizzo risolto.
        Err(_) => bind
            .to_socket_addrs()
            .map_err(|e| format!("indirizzo {bind:?} non valido: {e}"))?
            .next()
            .ok_or_else(|| format!("indirizzo {bind:?} non risolvibile"))?,
    };
    if addr.ip().is_unspecified() {
        addr.set_ip(match addr.ip() {
            IpAddr::V4(_) => IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            IpAddr::V6(_) => IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
        });
    }
    Ok(addr)
}

#[cfg(test)]
mod tests {
    use super::target_addr;

    #[test]
    fn unspecified_addresses_become_loopback() {
        assert_eq!(target_addr("0.0.0.0:8080").unwrap().to_string(), "127.0.0.1:8080");
        assert_eq!(target_addr("[::]:9000").unwrap().to_string(), "[::1]:9000");
        assert_eq!(target_addr("127.0.0.1:8081").unwrap().to_string(), "127.0.0.1:8081");
        assert_eq!(target_addr("192.168.1.5:80").unwrap().to_string(), "192.168.1.5:80");
        assert!(target_addr("nonsense").is_err());
    }
}
