//! Router axum e gestore delle richieste (contratto: docs/PIANO.md §5.4).
//!
//! Il server ha un solo gestore (`fallback`) che decide da sé cosa servire, invece di registrare
//! una rotta per file: così le regole (prefisso, percorsi sicuri, precompressi, 304) stanno in un
//! posto solo e "nessun'altra rotta" è vero per costruzione. Metodi ammessi: `GET` e `HEAD`.

use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri},
    response::Response,
    Router,
};
use tower_http::{set_header::SetResponseHeaderLayer, trace::TraceLayer};

use crate::assets::Assets;
use crate::config::normalize_prefix;

/// `Content-Security-Policy` di tutte le risposte. eframe non usa `eval`: basta `'wasm-unsafe-eval'`
/// per compilare il WebAssembly; `blob:` serve per i download (esportazioni) e le immagini generate.
pub const CONTENT_SECURITY_POLICY: &str =
    "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; \
     style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; manifest-src 'self'; \
     base-uri 'none'; frame-ancestors 'none'; object-src 'none'";
/// `Permissions-Policy`: l'app non usa nessuna di queste funzioni.
pub const PERMISSIONS_POLICY: &str = "camera=(), microphone=(), geolocation=()";
/// `Referrer-Policy` di tutte le risposte.
pub const REFERRER_POLICY: &str = "strict-origin-when-cross-origin";

/// File che il browser deve riconvalidare a ogni visita (il service worker ha una cache propria versionata).
const NO_CACHE_FILES: [&str; 3] = ["index.html", "sw.js", "manifest.webmanifest"];
const CACHE_STATIC: &str = "public, max-age=3600";

struct AppState {
    assets: Arc<dyn Assets>,
    /// Prefisso normalizzato: `""` (radice) oppure `/hasher`.
    prefix: String,
}

/// Costruisce il router. `prefix` è `HASHER_WEB_PREFIX` (`/`, `/hasher`, …): con un prefisso tutte le
/// rotte (anche `/healthz`) vivono sotto di esso e `/hasher` rimanda con 308 a `/hasher/`.
///
/// Su ogni risposta (anche 404, 304, 405) vengono aggiunti gli header di sicurezza.
pub fn router(assets: Arc<dyn Assets>, prefix: &str) -> Result<Router, String> {
    let state = Arc::new(AppState {
        assets,
        prefix: normalize_prefix(prefix)?,
    });
    Ok(Router::new()
        .fallback(handle)
        .with_state(state)
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CONTENT_SECURITY_POLICY),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static(PERMISSIONS_POLICY),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static(REFERRER_POLICY),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        // Log d'accesso a livello DEBUG (`RUST_LOG=debug`); con il livello predefinito `info` non scrive nulla.
        .layer(TraceLayer::new_for_http()))
}

async fn handle(State(state): State<Arc<AppState>>, method: Method, uri: Uri, headers: HeaderMap) -> Response {
    let head = if method == Method::GET {
        false
    } else if method == Method::HEAD {
        true
    } else {
        return method_not_allowed();
    };
    match resolve(&state.prefix, uri.path()) {
        Target::NotFound => not_found(),
        Target::Redirect => redirect(&state.prefix, uri.query()),
        Target::Health => health(head),
        Target::File(name) => serve_file(state.assets.as_ref(), name, &headers, head),
    }
}

/// Cosa chiede un percorso di richiesta.
#[derive(Debug, PartialEq, Eq)]
enum Target<'a> {
    /// `/healthz`
    Health,
    /// `/hasher` (senza barra finale) → `/hasher/`
    Redirect,
    /// Un file di `dist/` (`index.html`, `icons/icon.svg`, …)
    File(&'a str),
    NotFound,
}

fn resolve<'a>(prefix: &str, path: &'a str) -> Target<'a> {
    let rel = if prefix.is_empty() {
        match path.strip_prefix('/') {
            Some(rel) => rel,
            None => return Target::NotFound,
        }
    } else if path == prefix {
        // Servire index.html qui romperebbe gli URL relativi (`boot.js` si risolverebbe in `/boot.js`).
        return Target::Redirect;
    } else {
        match path.strip_prefix(prefix).and_then(|rest| rest.strip_prefix('/')) {
            Some(rel) => rel,
            None => return Target::NotFound,
        }
    };
    if rel.is_empty() {
        Target::File("index.html")
    } else if rel == "healthz" {
        Target::Health
    } else if is_servable_path(rel) {
        Target::File(rel)
    } else {
        Target::NotFound
    }
}

/// Un percorso è servibile solo se ogni segmento è un nome "normale": niente segmenti vuoti (`//`,
/// barra finale), niente file nascosti né `.`/`..`, niente `\` o caratteri di controllo. I file
/// `.br`/`.gz` non si servono mai come risorse a sé: sono varianti dell'originale.
/// Il percorso non viene decodificato (`%2e%2e` resta un nome qualunque che non esiste).
fn is_servable_path(rel: &str) -> bool {
    let ends_with = |ext: &str| {
        rel.len() > ext.len() && rel.as_bytes()[rel.len() - ext.len()..].eq_ignore_ascii_case(ext.as_bytes())
    };
    !(ends_with(".br") || ends_with(".gz"))
        && rel.split('/').all(|seg| {
            !seg.is_empty() && !seg.starts_with('.') && !seg.contains('\\') && !seg.chars().any(char::is_control)
        })
}

fn serve_file(assets: &dyn Assets, name: &str, headers: &HeaderMap, head: bool) -> Response {
    let Some(plain) = assets.get(name) else {
        return not_found();
    };
    let br = assets.get(&format!("{name}.br"));
    let gz = assets.get(&format!("{name}.gz"));
    // `Vary` va indicato ogni volta che esistono varianti, anche quando si serve quella non compressa.
    let vary = br.is_some() || gz.is_some();

    let accepted = AcceptEncoding::from_headers(headers);
    let (encoding, asset) = match (br, gz) {
        (Some(b), g) if accepted.br > 0 && (g.is_none() || accepted.br >= accepted.gzip) => (Some("br"), b),
        (_, Some(g)) if accepted.gzip > 0 => (Some("gzip"), g),
        _ => (None, plain),
    };

    // L'ETag è l'hash della rappresentazione servita: le varianti br/gzip hanno quindi ETag diversi.
    let etag = format!("\"{}\"", hex(&asset.sha256));
    let cache_control = if NO_CACHE_FILES.contains(&name) {
        "no-cache"
    } else {
        CACHE_STATIC
    };

    let mut res = Response::new(Body::empty());
    let h = res.headers_mut();
    set(h, header::ETAG, &etag);
    set(h, header::CACHE_CONTROL, cache_control);
    if vary {
        h.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    }
    if if_none_match(headers, &etag) {
        *res.status_mut() = StatusCode::NOT_MODIFIED;
        return res;
    }
    set(h, header::CONTENT_TYPE, &content_type(name));
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(asset.data.len()));
    if let Some(encoding) = encoding {
        h.insert(header::CONTENT_ENCODING, HeaderValue::from_static(encoding));
    }
    if !head {
        *res.body_mut() = Body::from(match asset.data {
            std::borrow::Cow::Borrowed(bytes) => Bytes::from_static(bytes),
            std::borrow::Cow::Owned(vec) => Bytes::from(vec),
        });
    }
    res
}

/// `Content-Type` dall'estensione. I formati testuali hanno `charset=utf-8`; `.wasm` e `.webmanifest`
/// hanno il tipo richiesto da `WebAssembly.instantiateStreaming` e dai browser per i manifest.
fn content_type(name: &str) -> String {
    let file = name.rsplit('/').next().unwrap_or(name);
    let Some((_, ext)) = file.rsplit_once('.') else {
        return "application/octet-stream".to_owned();
    };
    let ext = ext.to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "webmanifest" => "application/manifest+json",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "png" => "image/png",
        "ico" => "image/x-icon",
        other => mime_guess::from_ext(other)
            .first_raw()
            .unwrap_or("application/octet-stream"),
    }
    .to_owned()
}

/// Valutazione di `If-None-Match` (confronto debole, elenco separato da virgole, `*`).
fn if_none_match(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .any(|candidate| candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == etag)
}

/// Qualità (millesimi, 0..=1000) con cui il client accetta `br` e `gzip`. Senza `Accept-Encoding` valgono 0:
/// la risposta è non compressa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AcceptEncoding {
    br: u16,
    gzip: u16,
}

impl AcceptEncoding {
    fn from_headers(headers: &HeaderMap) -> Self {
        let (mut br, mut gzip, mut star) = (None, None, None);
        for value in headers
            .get_all(header::ACCEPT_ENCODING)
            .iter()
            .filter_map(|v| v.to_str().ok())
        {
            for item in value.split(',') {
                let mut parts = item.split(';');
                let coding = parts.next().unwrap_or("").trim().to_ascii_lowercase();
                let q = parts
                    .find_map(|p| {
                        let p = p.trim();
                        p.strip_prefix("q=").or_else(|| p.strip_prefix("Q=")).map(parse_q)
                    })
                    .unwrap_or(1000);
                match coding.as_str() {
                    "br" => br = Some(q),
                    "gzip" | "x-gzip" => gzip = Some(q),
                    "*" => star = Some(q),
                    _ => {}
                }
            }
        }
        // Le voci esplicite vincono sul carattere jolly.
        Self {
            br: br.or(star).unwrap_or(0),
            gzip: gzip.or(star).unwrap_or(0),
        }
    }
}

fn parse_q(value: &str) -> u16 {
    value
        .trim()
        .parse::<f32>()
        .map_or(1000, |q| (q.clamp(0.0, 1.0) * 1000.0).round() as u16)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn set(headers: &mut HeaderMap, name: HeaderName, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        headers.insert(name, value);
    }
}

fn text_response(status: StatusCode, body: &'static str, head: bool) -> Response {
    let mut res = Response::new(if head { Body::empty() } else { Body::from(body) });
    *res.status_mut() = status;
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(body.len()));
    res
}

fn health(head: bool) -> Response {
    text_response(StatusCode::OK, "ok", head)
}

fn not_found() -> Response {
    text_response(StatusCode::NOT_FOUND, "not found\n", false)
}

fn method_not_allowed() -> Response {
    let mut res = text_response(StatusCode::METHOD_NOT_ALLOWED, "method not allowed\n", false);
    res.headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
    res
}

fn redirect(prefix: &str, query: Option<&str>) -> Response {
    let mut location = format!("{prefix}/");
    if let Some(query) = query {
        location.push('?');
        location.push_str(query);
    }
    let mut res = Response::new(Body::empty());
    *res.status_mut() = StatusCode::PERMANENT_REDIRECT;
    set(res.headers_mut(), header::LOCATION, &location);
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_without_prefix() {
        assert_eq!(resolve("", "/"), Target::File("index.html"));
        assert_eq!(resolve("", "/healthz"), Target::Health);
        assert_eq!(resolve("", "/app.js"), Target::File("app.js"));
        assert_eq!(resolve("", "/icons/icon.svg"), Target::File("icons/icon.svg"));
        for bad in [
            "/..", "/../x", "/a/../x", "/.hidden", "//x", "/icons/", "/a\\b", "/x.br", "/x.GZ",
        ] {
            assert_eq!(resolve("", bad), Target::NotFound, "{bad}");
        }
    }

    #[test]
    fn resolve_with_prefix() {
        assert_eq!(resolve("/hasher", "/hasher"), Target::Redirect);
        assert_eq!(resolve("/hasher", "/hasher/"), Target::File("index.html"));
        assert_eq!(resolve("/hasher", "/hasher/healthz"), Target::Health);
        assert_eq!(resolve("/hasher", "/hasher/app.js"), Target::File("app.js"));
        for bad in ["/", "/app.js", "/healthz", "/hasherx", "/hasherx/app.js", "/hash"] {
            assert_eq!(resolve("/hasher", bad), Target::NotFound, "{bad}");
        }
    }

    #[test]
    fn content_types() {
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(content_type("a/b/app.JS"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("app_bg.wasm"), "application/wasm");
        assert_eq!(content_type("manifest.webmanifest"), "application/manifest+json");
        assert_eq!(content_type("README"), "application/octet-stream");
        assert_eq!(content_type("x.unknownext"), "application/octet-stream");
    }

    #[test]
    fn accept_encoding_parsing() {
        let parse = |v: &str| {
            let mut h = HeaderMap::new();
            h.insert(header::ACCEPT_ENCODING, HeaderValue::from_str(v).unwrap());
            AcceptEncoding::from_headers(&h)
        };
        assert_eq!(
            AcceptEncoding::from_headers(&HeaderMap::new()),
            AcceptEncoding { br: 0, gzip: 0 }
        );
        assert_eq!(parse("gzip, deflate, br"), AcceptEncoding { br: 1000, gzip: 1000 });
        assert_eq!(parse("br;q=0, gzip"), AcceptEncoding { br: 0, gzip: 1000 });
        assert_eq!(parse("gzip;q=0.5, br;q=0.8"), AcceptEncoding { br: 800, gzip: 500 });
        assert_eq!(parse("*;q=0.1, gzip"), AcceptEncoding { br: 100, gzip: 1000 });
        assert_eq!(parse("BR"), AcceptEncoding { br: 1000, gzip: 0 });
        assert_eq!(parse("identity"), AcceptEncoding { br: 0, gzip: 0 });
        assert_eq!(parse("x-gzip"), AcceptEncoding { br: 0, gzip: 1000 });
    }
}
