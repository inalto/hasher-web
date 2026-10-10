#!/usr/bin/env bash
# Ciclo di sviluppo: build web (debug) e server locale in debug (legge `dist/` dal disco a ogni richiesta,
# quindi dopo un nuovo `scripts/build-web.sh --debug` basta ricaricare la pagina).
#   scripts/dev.sh            → http://127.0.0.1:8080
#
# Le compilazioni passano dal lock condiviso; il server viene avviato FUORI dal lock, altrimenti
# terrebbe bloccate le altre compilazioni per tutta la sua durata.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export PATH="$HOME/.cargo/bin:$PATH"
LOCK=/tmp/hasher-web-cargo.lock
scripts/build-web.sh --debug
flock "$LOCK" cargo build -p hasher-web-server
HASHER_WEB_BIND="${HASHER_WEB_BIND:-127.0.0.1:8080}" exec target/debug/hasher-web-server
