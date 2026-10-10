//! Hasher Web — server HTTP. Martini Multimedia s.a.s.
//!
//! Serve i file statici della PWA (`dist/`, prodotti da `scripts/build-web.sh`) e nient'altro:
//! nessuna API, nessun upload. Gli hash vengono calcolati nel browser dell'utente.
//!
//! Contratto (docs/PIANO.md §5.4):
//! - `GET /healthz` → `200 ok`; `GET /` → `index.html`; `GET /<file>` → file di `dist/`; 404 altrimenti;
//! - `ETag` + `304`, `Cache-Control` (`no-cache` per `index.html`, `sw.js`, `manifest.webmanifest`),
//!   varianti precompresse `.br`/`.gz` negoziate con `Accept-Encoding`, header di sicurezza e CSP;
//! - variabili d'ambiente: `HASHER_WEB_BIND` (default `0.0.0.0:8080`), `HASHER_WEB_PREFIX`
//!   (default `/`, es. `/hasher` se il proxy non toglie il prefisso), `RUST_LOG`.
//!
//! La libreria espone il router (`router`) e le parti accessorie (`serve` con spegnimento ordinato,
//! `healthcheck`, `Config`) in modo che i test possano provarli senza avviare il binario.

mod assets;
mod config;
mod handler;
mod healthcheck;

use std::future::Future;
use std::time::Duration;

pub use assets::{Asset, Assets, Dist, EmbedAssets};
pub use config::{normalize_prefix, Config, DEFAULT_BIND};
pub use handler::{router, CONTENT_SECURITY_POLICY, PERMISSIONS_POLICY, REFERRER_POLICY};
pub use healthcheck::healthcheck;

/// Dopo il segnale di arresto le connessioni ancora attive hanno questo tempo per terminare.
/// `docker stop` / `podman stop` attendono 10 s prima di inviare SIGKILL.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(8);

/// Serve `app` su `listener` finché `shutdown` non si completa; poi smette di accettare connessioni,
/// lascia terminare le richieste in corso (al massimo `grace`) e ritorna.
pub async fn serve(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
    grace: Duration,
) -> std::io::Result<()> {
    let (signalled_tx, signalled_rx) = tokio::sync::oneshot::channel::<()>();
    let signal = async move {
        shutdown.await;
        // Se `serve` è già terminato il destinatario non c'è più: nessun problema.
        let _ = signalled_tx.send(());
    };
    let server = axum::serve(listener, app).with_graceful_shutdown(signal);
    tokio::select! {
        result = server => result,
        () = async {
            if signalled_rx.await.is_ok() {
                tokio::time::sleep(grace).await;
            } else {
                std::future::pending::<()>().await;
            }
        } => {
            tracing::warn!("spegnimento forzato: alcune connessioni non sono terminate entro {} s", grace.as_secs());
            Ok(())
        }
    }
}

/// Si completa alla ricezione di SIGTERM (docker/podman stop) o SIGINT (Ctrl+C).
pub async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => tracing::info!("ricevuto SIGINT: spegnimento ordinato"),
        () = terminate => tracing::info!("ricevuto SIGTERM: spegnimento ordinato"),
    }
}
