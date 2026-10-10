//! Test con socket reali: `--healthcheck`, HEAD su HTTP/1.1 e spegnimento ordinato.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use hasher_web_server::{healthcheck, router, serve, Config, EmbedAssets};
use rust_embed::Embed;
use tokio::sync::oneshot;

#[derive(Embed)]
#[folder = "testdata"]
struct TestData;

struct Running {
    addr: SocketAddr,
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

async fn start(prefix: &str, grace: Duration) -> Running {
    let app = router(Arc::new(EmbedAssets::<TestData>::new()), prefix).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(serve(
        listener,
        app,
        async move {
            let _ = stopped.await;
        },
        grace,
    ));
    Running { addr, stop, task }
}

/// Invia una richiesta HTTP/1.1 grezza e restituisce l'intera risposta (la connessione viene chiusa dal server).
fn raw_request(addr: SocketAddr, request: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut out = Vec::new();
    stream.read_to_end(&mut out).unwrap();
    out
}

fn split_response(raw: &[u8]) -> (String, Vec<u8>) {
    let pos = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("fine degli header");
    (
        String::from_utf8_lossy(&raw[..pos]).to_ascii_lowercase(),
        raw[pos + 4..].to_vec(),
    )
}

#[tokio::test]
async fn healthcheck_succeeds_against_a_running_server() {
    let server = start("/", SHUTDOWN).await;
    let bind = server.addr.to_string();
    assert_eq!(
        tokio::task::spawn_blocking(move || healthcheck(&bind, ""))
            .await
            .unwrap(),
        Ok(())
    );
    finish(server).await;
}

#[tokio::test]
async fn healthcheck_honours_the_prefix() {
    let server = start("/hasher", SHUTDOWN).await;
    let bind = server.addr.to_string();
    let b2 = bind.clone();
    assert_eq!(
        tokio::task::spawn_blocking(move || healthcheck(&bind, "/hasher"))
            .await
            .unwrap(),
        Ok(())
    );
    // Senza prefisso `/healthz` non esiste: il healthcheck deve fallire.
    let result = tokio::task::spawn_blocking(move || healthcheck(&b2, "")).await.unwrap();
    assert!(result.is_err(), "{result:?}");
    finish(server).await;
}

#[tokio::test]
async fn healthcheck_fails_when_nothing_listens() {
    // Si prende una porta libera e la si rilascia subito.
    let free = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    let result = tokio::task::spawn_blocking(move || healthcheck(&free, ""))
        .await
        .unwrap();
    assert!(result.is_err());
}

#[tokio::test]
async fn head_over_http_has_content_length_and_no_body() {
    let server = start("/", SHUTDOWN).await;
    let addr = server.addr;
    let (head, body) = tokio::task::spawn_blocking(move || {
        let raw = raw_request(addr, "HEAD /app.js HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n");
        split_response(&raw)
    })
    .await
    .unwrap();
    let len = std::fs::metadata(concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/app.js"))
        .unwrap()
        .len();
    assert!(head.starts_with("http/1.1 200"), "{head}");
    assert!(head.contains(&format!("content-length: {len}")), "{head}");
    assert!(body.is_empty());
    finish(server).await;
}

#[tokio::test]
async fn get_over_http_serves_the_precompressed_body() {
    let server = start("/", SHUTDOWN).await;
    let addr = server.addr;
    let (head, body) = tokio::task::spawn_blocking(move || {
        let raw = raw_request(
            addr,
            "GET /app.js HTTP/1.1\r\nHost: test\r\nAccept-Encoding: br\r\nConnection: close\r\n\r\n",
        );
        split_response(&raw)
    })
    .await
    .unwrap();
    assert!(head.contains("content-encoding: br"), "{head}");
    assert!(head.contains("vary: accept-encoding"), "{head}");
    assert_eq!(
        body,
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/app.js.br")).unwrap()
    );
    finish(server).await;
}

const SHUTDOWN: Duration = Duration::from_secs(5);

async fn finish(server: Running) {
    server.stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), server.task)
        .await
        .expect("arresto entro 5 s")
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn graceful_shutdown_completes_and_stops_accepting() {
    let server = start("/", SHUTDOWN).await;
    let addr = server.addr;
    finish(server).await;
    assert!(
        TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_err(),
        "il listener è stato chiuso"
    );
}

#[tokio::test]
async fn shutdown_is_bounded_even_with_a_stalled_client() {
    let server = start("/", Duration::from_millis(300)).await;
    // Un client che ha iniziato una richiesta e non la completa mai.
    let mut stalled = TcpStream::connect(server.addr).unwrap();
    stalled.write_all(b"GET / HTTP/1.1\r\nHost: test\r\n").unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    server.stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), server.task)
        .await
        .expect("lo spegnimento non deve restare appeso al client")
        .unwrap()
        .unwrap();
    drop(stalled);
}

#[test]
fn config_defaults_and_parsing() {
    let c = Config::from_values(None, None).unwrap();
    assert_eq!((c.bind.as_str(), c.prefix.as_str()), ("0.0.0.0:8080", ""));
    let c = Config::from_values(Some("127.0.0.1:8081".into()), Some("/hasher/".into())).unwrap();
    assert_eq!((c.bind.as_str(), c.prefix.as_str()), ("127.0.0.1:8081", "/hasher"));
    // Variabili presenti ma vuote → valori predefiniti.
    let c = Config::from_values(Some("  ".into()), Some(String::new())).unwrap();
    assert_eq!((c.bind.as_str(), c.prefix.as_str()), ("0.0.0.0:8080", ""));
    assert!(Config::from_values(None, Some("/../etc".into())).is_err());
}
