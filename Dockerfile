# syntax=docker/dockerfile:1
# Hasher Web — immagine multi-stage: build wasm + server axum, runtime minimo senza shell.
# BOZZA (fase 1C): da completare e verificare con `podman build -t hasher-web .`

# ---------- build ----------
FROM rust:1-bookworm AS build
RUN rustup target add wasm32-unknown-unknown \
 && apt-get update && apt-get install -y --no-install-recommends binaryen brotli gzip git \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY web ./web
COPY scripts ./scripts
COPY .cargo ./.cargo
# La CLI wasm-bindgen deve avere la stessa versione del crate in Cargo.lock.
RUN WBG="$(sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\(.*\)"/\1/p}' Cargo.lock | head -n1)" \
 && cargo install --locked wasm-bindgen-cli --version "$WBG"
RUN scripts/build-web.sh
RUN cargo build --release -p hasher-web-server

# ---------- runtime ----------
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/hasher-web-server /hasher-web-server
ENV HASHER_WEB_BIND=0.0.0.0:8080
EXPOSE 8080
USER nonroot
ENTRYPOINT ["/hasher-web-server"]
