#!/usr/bin/env bash
# Ciclo di sviluppo: build web (debug) e server locale in debug (legge `dist/` dal disco a ogni richiesta).
#   scripts/dev.sh            → http://127.0.0.1:8080
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export PATH="$HOME/.cargo/bin:$PATH"
scripts/build-web.sh --debug
HASHER_WEB_BIND="${HASHER_WEB_BIND:-127.0.0.1:8080}" flock /tmp/hasher-web-cargo.lock cargo run -p hasher-web-server
