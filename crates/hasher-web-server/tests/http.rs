//! Test del router senza socket (`tower::ServiceExt::oneshot`).
//!
//! `dist/` è condiviso e prodotto dalla build dell'app, quindi i test non lo usano: puntano la stessa
//! `EmbedAssets` di produzione a `testdata/` (file piccoli e versionati, con varianti `.br`/`.gz`
//! già generate). Il percorso di codice provato è quindi quello reale: stesso hash SHA-256 per
//! l'ETag, stessa ricerca per nome. In debug `rust-embed` legge `testdata/` dal disco a ogni richiesta.

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::Router;
use hasher_web_server::{router, EmbedAssets, CONTENT_SECURITY_POLICY, PERMISSIONS_POLICY, REFERRER_POLICY};
use rust_embed::Embed;
use tower::ServiceExt;

#[derive(Embed)]
#[folder = "testdata"]
struct TestData;

fn app(prefix: &str) -> Router {
    router(Arc::new(EmbedAssets::<TestData>::new()), prefix).expect("prefisso valido")
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .unwrap_or_else(|| panic!("header {name} assente"))
            .to_str()
            .unwrap()
    }
    fn has(&self, name: &str) -> bool {
        self.headers.contains_key(name)
    }
}

async fn send(app: &Router, method: &str, path: &str, headers: &[(&str, &str)]) -> Reply {
    let mut req = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    let res = app.clone().oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
    let (parts, body) = res.into_parts();
    let body = axum::body::to_bytes(body, usize::MAX).await.unwrap().to_vec();
    Reply {
        status: parts.status,
        headers: parts.headers,
        body,
    }
}

async fn get(app: &Router, path: &str) -> Reply {
    send(app, "GET", path, &[]).await
}

fn testdata(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata").join(name)).unwrap()
}

fn assert_security_headers(r: &Reply) {
    assert_eq!(r.header("x-content-type-options"), "nosniff");
    assert_eq!(r.header("referrer-policy"), "strict-origin-when-cross-origin");
    assert_eq!(
        r.header("permissions-policy"),
        "camera=(), microphone=(), geolocation=()"
    );
    assert_eq!(r.header("content-security-policy"), CONTENT_SECURITY_POLICY);
}

#[tokio::test]
async fn index_is_served_at_root_and_by_name() {
    let app = app("/");
    for path in ["/", "/index.html"] {
        let r = get(&app, path).await;
        assert_eq!(r.status, StatusCode::OK, "{path}");
        assert_eq!(r.body, testdata("index.html"), "{path}");
        assert_eq!(r.header("content-type"), "text/html; charset=utf-8");
        assert_eq!(r.header("cache-control"), "no-cache");
        assert_eq!(r.header("content-length"), testdata("index.html").len().to_string());
        assert!(!r.has("content-encoding"));
    }
}

#[tokio::test]
async fn healthz_returns_ok() {
    let app = app("/");
    let r = get(&app, "/healthz").await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body, b"ok");
    assert_eq!(r.header("content-type"), "text/plain; charset=utf-8");

    let head = send(&app, "HEAD", "/healthz", &[]).await;
    assert_eq!(head.status, StatusCode::OK);
    assert!(head.body.is_empty());
}

#[tokio::test]
async fn unknown_paths_are_404() {
    let app = app("/");
    for path in [
        "/missing.js",
        "/icons",
        "/icons/",
        "/nope/nope",
        "/ghost.js",
        "/healthz/",
        "/api/hash",
    ] {
        let r = get(&app, path).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{path}");
        assert_security_headers(&r);
    }
}

#[tokio::test]
async fn unsafe_paths_are_404() {
    let app = app("/");
    for path in [
        "/..",
        "/../Cargo.toml",
        "/icons/../index.html",
        "/%2e%2e/Cargo.toml",
        "/%2E%2E%2FCargo.toml",
        "//index.html",
        "/.hidden",
        "/icons/.hidden",
        "/a%5Cb",
    ] {
        let r = get(&app, path).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn etag_and_304() {
    let app = app("/");
    let first = get(&app, "/app_bg.wasm").await;
    assert_eq!(first.status, StatusCode::OK);
    let etag = first.header("etag").to_owned();
    assert!(
        etag.starts_with('"') && etag.ends_with('"') && etag.len() == 66,
        "ETag SHA-256 atteso: {etag}"
    );
    assert_eq!(get(&app, "/app_bg.wasm").await.header("etag"), etag, "l'ETag è stabile");

    // Corrispondenza esatta, debole, in un elenco e carattere jolly → 304 senza corpo.
    for inm in [
        etag.clone(),
        format!("W/{etag}"),
        format!("\"altro\", {etag}"),
        "*".to_owned(),
    ] {
        let r = send(&app, "GET", "/app_bg.wasm", &[("if-none-match", &inm)]).await;
        assert_eq!(r.status, StatusCode::NOT_MODIFIED, "{inm}");
        assert!(r.body.is_empty());
        assert_eq!(r.header("etag"), etag);
        assert_eq!(r.header("cache-control"), "public, max-age=3600");
        assert_security_headers(&r);
    }
    // ETag diverso → risposta completa.
    let r = send(&app, "GET", "/app_bg.wasm", &[("if-none-match", "\"altro\"")]).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body, testdata("app_bg.wasm"));
    // Anche HEAD risponde 304.
    let r = send(&app, "HEAD", "/app_bg.wasm", &[("if-none-match", &etag)]).await;
    assert_eq!(r.status, StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn different_files_have_different_etags() {
    let app = app("/");
    let a = get(&app, "/app.js").await;
    let b = get(&app, "/sw.js").await;
    assert_ne!(a.header("etag"), b.header("etag"));
}

#[tokio::test]
async fn security_headers_on_every_kind_of_response() {
    let app = app("/hasher");
    let replies = [
        get(&app, "/hasher/").await,                                             // 200
        get(&app, "/hasher/healthz").await,                                      // 200 (testo)
        get(&app, "/hasher/missing").await,                                      // 404
        get(&app, "/").await,                                                    // 404 (fuori prefisso)
        get(&app, "/hasher").await,                                              // 308
        send(&app, "POST", "/hasher/", &[]).await,                               // 405
        send(&app, "GET", "/hasher/app.js", &[("accept-encoding", "br")]).await, // 200 compresso
    ];
    for r in &replies {
        assert_security_headers(r);
    }
    let etag = replies[0].header("etag").to_owned();
    let not_modified = send(&app, "GET", "/hasher/", &[("if-none-match", &etag)]).await;
    assert_eq!(not_modified.status, StatusCode::NOT_MODIFIED);
    assert_security_headers(&not_modified);

    // Il testo dei valori coincide con il contratto del PIANO §5.4.
    assert_eq!(REFERRER_POLICY, "strict-origin-when-cross-origin");
    assert_eq!(PERMISSIONS_POLICY, "camera=(), microphone=(), geolocation=()");
    assert_eq!(
        CONTENT_SECURITY_POLICY,
        "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; connect-src 'self'; manifest-src 'self'; base-uri 'none'; frame-ancestors 'none'; \
         object-src 'none'"
    );
}

#[tokio::test]
async fn content_types_by_extension() {
    let app = app("/");
    let cases = [
        ("/index.html", "text/html; charset=utf-8"),
        ("/style.css", "text/css; charset=utf-8"),
        ("/app.js", "text/javascript; charset=utf-8"),
        ("/sw.js", "text/javascript; charset=utf-8"),
        ("/app_bg.wasm", "application/wasm"),
        ("/manifest.webmanifest", "application/manifest+json"),
        ("/icons/icon.svg", "image/svg+xml; charset=utf-8"),
        ("/icons/icon-16.png", "image/png"),
    ];
    for (path, content_type) in cases {
        let r = get(&app, path).await;
        assert_eq!(r.status, StatusCode::OK, "{path}");
        assert_eq!(r.header("content-type"), content_type, "{path}");
    }
}

#[tokio::test]
async fn cache_control_policy() {
    let app = app("/");
    for path in ["/index.html", "/sw.js", "/manifest.webmanifest"] {
        assert_eq!(get(&app, path).await.header("cache-control"), "no-cache", "{path}");
    }
    for path in [
        "/app.js",
        "/app_bg.wasm",
        "/style.css",
        "/icons/icon.svg",
        "/icons/icon-16.png",
    ] {
        assert_eq!(
            get(&app, path).await.header("cache-control"),
            "public, max-age=3600",
            "{path}"
        );
    }
}

#[tokio::test]
async fn brotli_is_served_when_accepted() {
    let app = app("/");
    let r = send(&app, "GET", "/app.js", &[("accept-encoding", "gzip, deflate, br")]).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body, testdata("app.js.br"), "br ha la precedenza su gzip");
    assert_eq!(r.header("content-encoding"), "br");
    assert_eq!(r.header("vary"), "Accept-Encoding");
    assert_eq!(
        r.header("content-type"),
        "text/javascript; charset=utf-8",
        "il tipo resta quello dell'originale"
    );
    assert_eq!(r.header("content-length"), testdata("app.js.br").len().to_string());

    // L'ETag della variante è diverso da quello dell'originale, e il 304 vale per la variante.
    let plain = get(&app, "/app.js").await;
    assert_eq!(plain.body, testdata("app.js"));
    assert_ne!(plain.header("etag"), r.header("etag"));
    let again = send(
        &app,
        "GET",
        "/app.js",
        &[("accept-encoding", "br"), ("if-none-match", r.header("etag"))],
    )
    .await;
    assert_eq!(again.status, StatusCode::NOT_MODIFIED);
    assert_eq!(again.header("vary"), "Accept-Encoding");
    let wrong = send(
        &app,
        "GET",
        "/app.js",
        &[("accept-encoding", "br"), ("if-none-match", plain.header("etag"))],
    )
    .await;
    assert_eq!(
        wrong.status,
        StatusCode::OK,
        "l'ETag dell'originale non vale per la variante br"
    );

    let wasm = send(&app, "GET", "/app_bg.wasm", &[("accept-encoding", "br")]).await;
    assert_eq!(wasm.body, testdata("app_bg.wasm.br"));
    assert_eq!(wasm.header("content-type"), "application/wasm");
}

#[tokio::test]
async fn gzip_is_served_when_brotli_is_missing_or_refused() {
    let app = app("/");
    // `style.css` ha solo la variante .gz.
    let r = send(&app, "GET", "/style.css", &[("accept-encoding", "br, gzip")]).await;
    assert_eq!(r.body, testdata("style.css.gz"));
    assert_eq!(r.header("content-encoding"), "gzip");
    assert_eq!(r.header("content-type"), "text/css; charset=utf-8");
    // `app.js` ha entrambe: br rifiutato (q=0) → gzip.
    let r = send(&app, "GET", "/app.js", &[("accept-encoding", "br;q=0, gzip")]).await;
    assert_eq!(r.body, testdata("app.js.gz"));
    assert_eq!(r.header("content-encoding"), "gzip");
    // Il client preferisce gzip con una qualità più alta.
    let r = send(&app, "GET", "/app.js", &[("accept-encoding", "gzip;q=1.0, br;q=0.4")]).await;
    assert_eq!(r.header("content-encoding"), "gzip");
    // Carattere jolly.
    let r = send(&app, "GET", "/app.js", &[("accept-encoding", "*")]).await;
    assert_eq!(r.header("content-encoding"), "br");
}

#[tokio::test]
async fn identity_when_compression_is_not_accepted_or_not_available() {
    let app = app("/");
    for accept in [None, Some("identity"), Some("deflate"), Some("br;q=0, gzip;q=0")] {
        let headers: Vec<(&str, &str)> = accept.map(|a| ("accept-encoding", a)).into_iter().collect();
        let r = send(&app, "GET", "/app.js", &headers).await;
        assert_eq!(r.body, testdata("app.js"), "{accept:?}");
        assert!(!r.has("content-encoding"), "{accept:?}");
        assert_eq!(
            r.header("vary"),
            "Accept-Encoding",
            "Vary anche quando si serve l'originale: {accept:?}"
        );
    }
    // `style.css` non ha .br: un client che accetta solo br riceve l'originale.
    let r = send(&app, "GET", "/style.css", &[("accept-encoding", "br")]).await;
    assert_eq!(r.body, testdata("style.css"));
    assert!(!r.has("content-encoding"));
    // Un file senza varianti non ha né Content-Encoding né Vary.
    let r = send(&app, "GET", "/icons/icon-16.png", &[("accept-encoding", "br, gzip")]).await;
    assert!(!r.has("content-encoding") && !r.has("vary"));
}

#[tokio::test]
async fn precompressed_files_are_never_served_as_resources() {
    let app = app("/");
    for path in [
        "/app.js.br",
        "/app.js.gz",
        "/app_bg.wasm.br",
        "/style.css.gz",
        "/ghost.js.br",
        "/APP.JS.BR",
    ] {
        let r = send(&app, "GET", path, &[("accept-encoding", "br, gzip")]).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{path}");
    }
    // Una variante senza originale non rende servibile l'originale.
    assert_eq!(
        send(&app, "GET", "/ghost.js", &[("accept-encoding", "br")])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn only_get_and_head_are_allowed() {
    let app = app("/");
    for method in ["POST", "PUT", "DELETE", "PATCH", "OPTIONS"] {
        for path in ["/", "/healthz", "/app.js"] {
            let r = send(&app, method, path, &[]).await;
            assert_eq!(r.status, StatusCode::METHOD_NOT_ALLOWED, "{method} {path}");
            assert_eq!(r.header("allow"), "GET, HEAD");
        }
    }
}

#[tokio::test]
async fn head_has_the_headers_of_get_but_no_body() {
    let app = app("/");
    for path in ["/index.html", "/app.js"] {
        let get_reply = send(&app, "GET", path, &[("accept-encoding", "br")]).await;
        let head = send(&app, "HEAD", path, &[("accept-encoding", "br")]).await;
        assert_eq!(head.status, StatusCode::OK);
        assert!(head.body.is_empty());
        for name in ["content-type", "content-length", "etag", "cache-control"] {
            assert_eq!(head.header(name), get_reply.header(name), "{path} {name}");
        }
        assert_eq!(head.has("content-encoding"), get_reply.has("content-encoding"));
    }
}

#[tokio::test]
async fn path_prefix_nests_all_routes() {
    let app = app("/hasher");
    // Le rotte stanno tutte sotto il prefisso.
    let index = get(&app, "/hasher/").await;
    assert_eq!(index.status, StatusCode::OK);
    assert_eq!(index.body, testdata("index.html"));
    assert_eq!(get(&app, "/hasher/index.html").await.body, testdata("index.html"));
    assert_eq!(
        get(&app, "/hasher/app.js").await.header("content-type"),
        "text/javascript; charset=utf-8"
    );
    assert_eq!(get(&app, "/hasher/icons/icon.svg").await.status, StatusCode::OK);
    assert_eq!(get(&app, "/hasher/healthz").await.body, b"ok");
    let br = send(&app, "GET", "/hasher/app.js", &[("accept-encoding", "br")]).await;
    assert_eq!(br.body, testdata("app.js.br"));

    // `/hasher` senza barra rimanda a `/hasher/`: servire index.html lì romperebbe gli URL relativi.
    let redirect = get(&app, "/hasher").await;
    assert_eq!(redirect.status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(redirect.header("location"), "/hasher/");
    let redirect = get(&app, "/hasher?x=1").await;
    assert_eq!(redirect.header("location"), "/hasher/?x=1");

    // Fuori dal prefisso non c'è nulla.
    for path in [
        "/",
        "/index.html",
        "/app.js",
        "/healthz",
        "/hasherx",
        "/hasherx/app.js",
        "/other/app.js",
        "/hash",
    ] {
        assert_eq!(get(&app, path).await.status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn prefix_is_normalised() {
    for prefix in ["hasher", "/hasher", "/hasher/", " /hasher// "] {
        let app = app(prefix);
        assert_eq!(get(&app, "/hasher/").await.status, StatusCode::OK, "{prefix:?}");
        assert_eq!(get(&app, "/").await.status, StatusCode::NOT_FOUND, "{prefix:?}");
    }
    for prefix in ["", "/", "//"] {
        let app = app(prefix);
        assert_eq!(get(&app, "/").await.status, StatusCode::OK, "{prefix:?}");
    }
    let nested = app("/tools/hasher");
    assert_eq!(get(&nested, "/tools/hasher/").await.status, StatusCode::OK);
    assert_eq!(get(&nested, "/tools/hasher").await.header("location"), "/tools/hasher/");
}

#[tokio::test]
async fn invalid_prefixes_are_rejected() {
    for prefix in [
        "/..",
        "/a/../b",
        "/.hidden",
        "/with space",
        "/a?b",
        "/a#b",
        "/%2e",
        "/città",
    ] {
        assert!(
            router(Arc::new(EmbedAssets::<TestData>::new()), prefix).is_err(),
            "{prefix:?}"
        );
    }
}
