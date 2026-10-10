# syntax=docker/dockerfile:1
# Hasher Web — immagine multi-stage: strumenti → build della PWA (wasm) e del server axum → runtime minimo.
#
#   podman build --format docker -t hasher-web .      (con Docker: docker build -t hasher-web .)
#   podman run --rm -p 8080:8080 hasher-web
#
# `--format docker` serve a podman per conservare l'istruzione HEALTHCHECK (il formato OCI la scarta;
# in alternativa si dichiara l'healthcheck al momento dell'avvio, vedi docs/DEPLOY.md).
# Opzioni di build: --build-arg CARGO_BUILD_JOBS=8 (parallelismo di cargo; predefinito 2 per le macchine con poca RAM).

# ---------- tools: toolchain wasm e strumenti di build (cambia di rado: layer in cache) ----------
FROM rust:1-bookworm AS tools

# Versione di binaryen (wasm-opt) usata per ottimizzare i .wasm; stessa della macchina di sviluppo.
ARG BINARYEN_VERSION=123
# Se vuoto, la versione della CLI wasm-bindgen viene letta da Cargo.lock (DEVE coincidere con il crate).
ARG WASM_BINDGEN_VERSION=

RUN rustup target add wasm32-unknown-unknown \
 && apt-get update \
 && apt-get install -y --no-install-recommends brotli \
 && rm -rf /var/lib/apt/lists/*

# wasm-opt: binario statico dalle release di binaryen.
RUN set -eux; \
    case "$(uname -m)" in \
      x86_64)  arch=x86_64 ;; \
      aarch64) arch=aarch64 ;; \
      *) echo "architettura non supportata: $(uname -m)" >&2; exit 1 ;; \
    esac; \
    mkdir /tmp/binaryen; \
    curl -fsSL --retry 3 "https://github.com/WebAssembly/binaryen/releases/download/version_${BINARYEN_VERSION}/binaryen-version_${BINARYEN_VERSION}-${arch}-linux.tar.gz" \
      | tar -xz -C /tmp/binaryen --strip-components=1; \
    install -m 0755 /tmp/binaryen/bin/wasm-opt /usr/local/bin/wasm-opt; \
    rm -rf /tmp/binaryen; \
    wasm-opt --version

# CLI wasm-bindgen con la STESSA versione del crate in Cargo.lock: tarball precompilato (secondi);
# se non disponibile per l'architettura o il download fallisce, ripiego su `cargo install` (minuti).
COPY Cargo.lock /tmp/Cargo.lock
RUN set -eux; \
    WBG="${WASM_BINDGEN_VERSION:-$(sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\(.*\)"/\1/p}' /tmp/Cargo.lock | head -n1)}"; \
    test -n "$WBG"; \
    case "$(uname -m)" in \
      x86_64)  triple=x86_64-unknown-linux-musl ;; \
      aarch64) triple=aarch64-unknown-linux-gnu ;; \
      *)       triple= ;; \
    esac; \
    mkdir /tmp/wbg; \
    if [ -n "$triple" ] && curl -fsSL --retry 3 \
         "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${WBG}/wasm-bindgen-${WBG}-${triple}.tar.gz" \
         | tar -xz -C /tmp/wbg --strip-components=1; then \
      install -m 0755 /tmp/wbg/wasm-bindgen /usr/local/bin/wasm-bindgen; \
    else \
      echo "tarball precompilato non disponibile: cargo install wasm-bindgen-cli ${WBG}" >&2; \
      cargo install --locked wasm-bindgen-cli --version "${WBG}" --root /usr/local; \
    fi; \
    rm -rf /tmp/wbg /tmp/Cargo.lock; \
    test "$(wasm-bindgen --version | awk '{print $2}')" = "$WBG"; \
    wasm-bindgen --version

# ---------- build: PWA (wasm) + server ----------
FROM tools AS build
ARG CARGO_BUILD_JOBS=2
ENV CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS}
WORKDIR /src
COPY Cargo.toml Cargo.lock rustfmt.toml ./
COPY .cargo ./.cargo
COPY crates ./crates
COPY web ./web
COPY scripts ./scripts

# I cache mount tengono registry e artefatti delle dipendenze fra una build e l'altra. I file del workspace
# vengono "toccati": COPY conserva le date del contesto e cargo, con un target in cache, potrebbe altrimenti
# considerare aggiornato un crate il cui sorgente è cambiato (le dipendenze restano in cache).

# 1) dist/: app egui + worker (wasm32), wasm-bindgen, wasm-opt, service worker versionato, .br/.gz
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target \
    find crates -type f \( -name '*.rs' -o -name 'Cargo.toml' \) -exec touch {} + \
 && scripts/build-web.sh

# 2) server: embedda dist/ nel binario (il target è in cache: il binario va copiato fuori in /out)
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target \
    find crates -type f \( -name '*.rs' -o -name 'Cargo.toml' \) -exec touch {} + \
 && cargo build --release --locked -p hasher-web-server \
 && install -D target/release/hasher-web-server /out/hasher-web-server

# ---------- runtime: distroless, senza shell, utente non privilegiato ----------
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime

LABEL org.opencontainers.image.title="Hasher Web" \
      org.opencontainers.image.description="Hasher Web: calcola e verifica gli hash dei file direttamente nel browser (PWA); server statico axum, nessun file viene inviato online." \
      org.opencontainers.image.source="https://github.com/inalto/hasher-web" \
      org.opencontainers.image.url="https://github.com/inalto/hasher-web" \
      org.opencontainers.image.documentation="https://github.com/inalto/hasher-web/blob/main/docs/DEPLOY.md" \
      org.opencontainers.image.licenses="NOASSERTION" \
      org.opencontainers.image.vendor="Martini Multimedia s.a.s."

COPY --from=build /out/hasher-web-server /hasher-web-server

ENV HASHER_WEB_BIND=0.0.0.0:8080
EXPOSE 8080
USER 65532:65532
# Distroless non ha shell né curl: il controllo è il binario stesso (GET /healthz su HASHER_WEB_BIND).
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 CMD ["/hasher-web-server", "--healthcheck"]
ENTRYPOINT ["/hasher-web-server"]
