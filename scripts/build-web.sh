#!/usr/bin/env bash
# Costruisce `dist/`: app egui (wasm) + worker core (wasm) + file statici di `web/`, service worker
# versionato e copie precompresse (.br/.gz). Poi `cargo build --release -p hasher-web-server`
# embedda `dist/` nel binario.
#
# Uso: scripts/build-web.sh [--debug] [--no-opt]
#   --debug   profilo dev (compilazione rapida, wasm più grande, senza wasm-opt)
#   --no-opt  salta wasm-opt anche in release
#
# Requisiti: rustup target wasm32-unknown-unknown; `wasm-bindgen` CLI con la STESSA versione del
# crate wasm-bindgen in Cargo.lock; opzionali `wasm-opt` (binaryen), `brotli`, `gzip`.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export PATH="$HOME/.cargo/bin:$PATH"

PROFILE=release
CARGO_FLAGS=(--release)
OPT=1
for arg in "$@"; do
  case "$arg" in
    --debug) PROFILE=debug; CARGO_FLAGS=(); OPT=0 ;;
    --no-opt) OPT=0 ;;
    *) echo "argomento sconosciuto: $arg" >&2; exit 2 ;;
  esac
done

# Le compilazioni sono serializzate (poca memoria sulla macchina di sviluppo, vedi docs/PIANO.md §6).
LOCK=/tmp/hasher-web-cargo.lock
run_cargo() { flock "$LOCK" cargo "$@"; }

# --- Verifica versione di wasm-bindgen (crate ↔ CLI) --------------------------------------------
WBG_CRATE="$(sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\(.*\)"/\1/p}' Cargo.lock | head -n1)"
WBG_CLI="$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)"
if [[ -z "$WBG_CLI" ]]; then
  echo "errore: wasm-bindgen CLI non trovata (serve la versione $WBG_CRATE)" >&2; exit 1
fi
if [[ "$WBG_CRATE" != "$WBG_CLI" ]]; then
  echo "errore: wasm-bindgen CLI $WBG_CLI ≠ crate $WBG_CRATE (cargo install wasm-bindgen-cli --version $WBG_CRATE)" >&2; exit 1
fi

# --- Compilazione wasm --------------------------------------------------------------------------
echo "==> cargo build ($PROFILE) hasher-web-app"
run_cargo build --target wasm32-unknown-unknown "${CARGO_FLAGS[@]}" -p hasher-web-app
echo "==> cargo build ($PROFILE) hasher-core --features worker"
run_cargo build --target wasm32-unknown-unknown "${CARGO_FLAGS[@]}" -p hasher-core --features worker

OUT="target/wasm32-unknown-unknown/$PROFILE"
rm -rf dist && mkdir -p dist

echo "==> wasm-bindgen"
wasm-bindgen --target web --no-typescript --out-dir dist --out-name app "$OUT/hasher-web-app.wasm"
wasm-bindgen --target web --no-typescript --out-dir dist --out-name core "$OUT/hasher_core.wasm"

if [[ "$OPT" == 1 ]] && command -v wasm-opt >/dev/null; then
  echo "==> wasm-opt"
  wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-reference-types \
    -o dist/app_bg.wasm dist/app_bg.wasm
  wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-reference-types \
    -o dist/core_bg.wasm dist/core_bg.wasm
fi

# --- File statici -------------------------------------------------------------------------------
echo "==> file statici"
cp -r web/. dist/
rm -f dist/sw.template.js

# --- Service worker versionato ------------------------------------------------------------------
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n1)"
GIT="$(git -C "$ROOT" describe --always --dirty 2>/dev/null || echo nogit)"
STAMP="$(date -u +%Y%m%d%H%M%S)"
ASSETS="$(cd dist && find . -type f ! -name 'sw.js' ! -name '*.br' ! -name '*.gz' ! -name '.gitkeep' \
  | sed 's|^\./||' | sort | awk 'BEGIN{printf "["} {printf "%s\"%s\"", (NR>1?",":""), $0} END{printf "]"}')"
ASSETS="${ASSETS/\"index.html\"/\"./\",\"index.html\"}"
sed -e "s|__VERSION__|$VERSION-$GIT-$STAMP|" -e "s|__ASSETS__|$ASSETS|" web/sw.template.js > dist/sw.js
touch dist/.gitkeep

# --- Precompressione ----------------------------------------------------------------------------
if [[ "$PROFILE" == release ]]; then
  echo "==> precompressione"
  while IFS= read -r f; do
    command -v brotli >/dev/null && brotli -kf -q 11 "$f"
    command -v gzip >/dev/null && gzip -kf -9 "$f"
  done < <(find dist -type f \( -name '*.wasm' -o -name '*.js' -o -name '*.html' -o -name '*.css' \
           -o -name '*.webmanifest' -o -name '*.svg' -o -name '*.json' \))
fi

echo "==> dist/ pronto:"
du -h dist/app_bg.wasm dist/core_bg.wasm
