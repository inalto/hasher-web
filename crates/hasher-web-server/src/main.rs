//! Hasher Web — server HTTP. Martini Multimedia s.a.s.
//!
//! Serve i file statici della PWA (`dist/`, prodotti da `scripts/build-web.sh`) e nient'altro:
//! nessuna API, nessun upload. Gli hash vengono calcolati nel browser dell'utente.
//!
//! Contratto (docs/PIANO.md §5.4):
//! - `GET /healthz` → `200 ok`;
//! - `GET /<file>` → file di `dist/` con `Content-Type` corretto; `/` → `index.html`;
//! - variabili d'ambiente: `HASHER_WEB_BIND` (default `0.0.0.0:8080`), `HASHER_WEB_PREFIX`
//!   (default `/`, es. `/hasher` se il proxy non toglie il prefisso), `RUST_LOG`;
//! - header di cache: `index.html`, `sw.js`, `manifest.webmanifest` → `no-cache`; il resto
//!   `public, max-age=3600`; `.br`/`.gz` precompressi serviti in base ad `Accept-Encoding`.
//!
//! STATO: versione minima funzionante. Completamento (precompressi, header di sicurezza/CSP,
//! prefisso, spegnimento ordinato, `--healthcheck`) a carico dell'agente "server" (fase 1C).

use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../dist"]
#[exclude = ".gitkeep"]
struct Dist;

async fn healthz() -> &'static str {
    "ok"
}

async fn static_file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Dist::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let cache = if matches!(path, "index.html" | "sw.js" | "manifest.webmanifest") {
                "no-cache"
            } else {
                "public, max-age=3600"
            };
            (
                [
                    (header::CONTENT_TYPE, HeaderValue::from_str(mime.as_ref()).unwrap()),
                    (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
                ],
                Body::from(file.data.into_owned()),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let bind = std::env::var("HASHER_WEB_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_owned());
    let app = Router::new()
        .route("/healthz", get(healthz))
        .fallback(get(static_file))
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("cannot bind {bind}: {e}"));
    tracing::info!("Hasher Web in ascolto su http://{bind}");
    axum::serve(listener, app).await.expect("server error");
}
