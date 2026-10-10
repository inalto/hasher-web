# Hasher Web — Piano di sviluppo

Versione web (PWA) di **Hasher**, l'applicazione desktop di **Martini Multimedia s.a.s.** per
calcolare e verificare gli hash di uno o più file. Sorgente di riferimento: `../hasher`
(egui/eframe 0.36, Rust puro).

Data piano: 10 ottobre 2026.

---

## 1. Obiettivo

Un'applicazione raggiungibile da un indirizzo web, installabile come PWA (barra delle applicazioni
di Windows, dock di macOS), **il più simile possibile all'app desktop**, in cui gli hash vengono
calcolati **nel browser dell'utente** (WebAssembly) senza inviare nulla al server. Il server gira in
un contenitore Docker dietro a un reverse proxy e serve soltanto file statici.

## 2. Decisione architetturale: tutto Rust

| Livello | Scelta | Motivazione |
|---|---|---|
| Interfaccia | **eframe/egui 0.36 compilato in `wasm32-unknown-unknown`**, stesso codice di `hasher/src/ui` (font Inter e JetBrains Mono, icone Phosphor, tema, animazioni embeddati) | fedeltà totale all'app desktop; un solo linguaggio; eframe ha un backend web maturo (WebGL2 via `glow`, drag & drop, clipboard, tema di sistema) |
| Calcolo | crate **`hasher-core`** (algoritmi, parser checksum, export) compilato in wasm ed eseguito in **Web Worker** (uno per core, max 8) con lettura a blocchi `File.slice(…).arrayBuffer()` | i file non vengono mai caricati per intero in memoria: funziona anche con file da decine di GB; il thread dell'interfaccia resta fluido |
| Server | **`hasher-web-server`** (axum 0.8, tokio) che serve `dist/` embeddato nel binario (rust-embed), precompresso brotli/gzip, con header di cache e CSP; `GET /healthz`; prefisso configurabile | richiesta del committente ("in Rust con axum"); un solo binario statico in un'immagine distroless; nessuna API = privacy strutturale |
| PWA | `manifest.webmanifest` (standalone, icone maskable, `file_handlers` per i file di checksum), service worker con precache versionata (offline), `launchQueue` | installabile e utilizzabile offline dopo la prima visita |

Limiti noti e accettati dell'interfaccia su canvas (gli stessi del desktop egui): niente selezione
del testo nativa del browser, accessibilità ridotta. Sotto i 700 px di larghezza (telefoni) l'interfaccia usa un
layout compatto (`ui/layout.rs`: menu ☰, viste impilate, verifica a schermo intero, finestre entro lo schermo); su
touch i testi non parlano di trascinamento né di Ctrl/Shift.

Verificato nel sorgente di eframe 0.36.2 (`~/.cargo/registry/src/*/eframe-0.36.2/src/web/`):
- i file trascinati arrivano in `RawInput::dropped_files` come `Arc<dyn egui::DroppedFile>` con
  `web_file() -> Option<&web_sys::File>`: **lettura pigra**, nessun caricamento in memoria;
- il gestore `drop` di eframe usa solo `dataTransfer.files()` (niente cartelle): per le cartelle
  serve un listener proprio in fase di capture che usi `DataTransferItem.webkitGetAsEntry()`;
- eframe chiama `preventDefault` solo per Ctrl+O, Ctrl+P, Ctrl+S, Ctrl+, e Spazio: per Ctrl+E,
  Ctrl+L, F1 serve un listener `keydown` in capture che chiami `preventDefault`;
- `paste` sul canvas produce `egui::Event::Paste` (il canvas deve avere il focus); `copy_text`
  usa `navigator.clipboard`;
- `WebOptions::default()` segue il tema di sistema (`prefers-color-scheme`).

## 3. Differenze rispetto all'app desktop (vincoli del browser)

| Desktop | Web |
|---|---|
| Percorso completo | Nome del file, oppure percorso relativo alla cartella trascinata (`webkitRelativePath` / entry) |
| Data di creazione, attributi, permessi | Non disponibili: `created = None`, `attributes` vuoto, `unix_mode = None`; la scheda mostra solo dimensione e data di modifica (griglia a 2 voci) |
| "Apri cartella" nella scheda del file | Rimosso |
| Dialoghi nativi `rfd` | `<input type="file" multiple>` e `<input type="file" webkitdirectory>` creati da Rust e `click()`-ati nell'azione egui (la *transient activation* del clic dura alcuni secondi: sufficiente). Il browser mostra "Caricare N file?": nulla viene caricato, lo spiega la guida |
| Cartelle trascinate (walkdir, ricorsione, nascosti, symlink) | `webkitGetAsEntry()` + `FileSystemDirectoryReader.readEntries()` ricorsivo (loop finché restituisce 0 voci); opzione "ricorsione" e "file nascosti" (nomi con `.`) rispettate; "segui i link simbolici" rimossa |
| File di checksum: i file elencati si cercano su disco | Si risolvono fra i file **già in elenco o rilasciati insieme** (stesso drop / stessa cartella), confrontando il nome relativo (e in subordine il solo nome); altrimenti MANCANTE |
| Impostazioni in `hasher.settings.json` portable | `localStorage`, chiave `hasher.settings` (stesso JSON); `window` ignorato |
| Esportazione con dialogo "Salva" | Download di un `Blob` (`<a download>`), nome `hasher-AAAAMMGG-HHMMSS.<ext>` |
| Menu File → Esci | Rimosso |
| Scorciatoie | Identiche; `keydown` in capture con `preventDefault` per Ctrl+O, Ctrl+Shift+O, Ctrl+E, Ctrl+L, Ctrl+,, F1 |
| Parallelismo: rayon per algoritmo + N file in parallelo | Pool di Web Worker (`navigator.hardwareConcurrency`, 1..=8, impostazione "file in parallelo" = numero di worker, 0 = auto). Con **più file**: un file per worker, tutti gli algoritmi. Con **un solo file** (o meno file che worker): gli algoritmi vengono **ripartiti fra i worker**, ognuno rilegge il file (cache del SO); avanzamento del file = minimo fra i task |
| Lingua automatica da `sys-locale` | `sys-locale` con feature `js` (`navigator.languages`) |
| Riga di comando, screenshot Xvfb, `HASHER_UI` | Rimossi |
| Aiuto → "Impostazioni portabili" | Testo adattato: impostazioni nel browser, funzionamento offline, installazione come app |
| — | Voce **"Installa come app"** (menu Aiuto e pulsante nello stato vuoto) visibile solo quando il browser espone `beforeinstallprompt` (`window.hasherCanInstall()`), che chiama `window.hasherInstall()` |
| — | File aperti dal sistema con la PWA (`file_handlers`): `window.hasherLaunchFiles` + evento `hasher:launch-files` |

Niente viene mai inviato al server: il server non ha endpoint oltre ai file statici e `/healthz`.

## 4. Architettura del codice

```
hasher-web/
├── Cargo.toml                   workspace (default-members: core + server; l'app è wasm32)
├── .cargo/config.toml           jobs = 2
├── crates/
│   ├── hasher-core/             Rust puro, nessun file system; nativo (test) e wasm32
│   │   └── src/ algo.rs hasher.rs checksum_file.rs fileinfo.rs result.rs export/ worker.rs
│   ├── hasher-web-app/          bin wasm32: eframe + port di hasher/src/ui + glue DOM
│   │   ├── assets/ brand/ fonts/ icons/        (copiati dal desktop, include_bytes!)
│   │   ├── locales/ it en fr de es .yml        (rust-i18n)
│   │   └── src/ main.rs settings.rs engine/ web/ ui/
│   └── hasher-web-server/       bin nativo: axum, rust-embed di ../../dist
├── web/                         index.html style.css boot.js worker.js manifest.webmanifest
│                                sw.template.js favicon.ico icons/
├── scripts/build-web.sh         cargo (wasm32) → wasm-bindgen → wasm-opt → dist/ + sw.js + .br/.gz
├── scripts/dev.sh               build debug + server locale
├── dist/                        output della build (ignorato da git, tranne .gitkeep)
├── Dockerfile, compose.yaml     immagine multi-stage, esempio compose
├── deploy/                      esempi di reverse proxy (nginx, Caddy, Traefik)
├── docs/PIANO.md                questo file
└── .github/workflows/           ci.yml (fmt, clippy nativo+wasm, test, build), release.yml (tag → GHCR + tar.gz)
```

### 4.1 Pipeline di build (`scripts/build-web.sh`)

1. `cargo build --target wasm32-unknown-unknown --release -p hasher-web-app`
2. `cargo build --target wasm32-unknown-unknown --release -p hasher-core --features worker`
3. `wasm-bindgen --target web --out-name app` / `--out-name core` → `dist/app.js`, `dist/app_bg.wasm`,
   `dist/core.js`, `dist/core_bg.wasm` (la versione della CLI deve coincidere con il crate: lo script la verifica)
4. `wasm-opt -O3` (se presente)
5. copia di `web/` in `dist/`, generazione di `dist/sw.js` dal template con versione e lista dei file
6. precompressione `.br` e `.gz`

Poi `cargo build --release -p hasher-web-server` embedda `dist/`. In debug il server legge `dist/`
dal disco a ogni richiesta (`rust-embed`): `scripts/dev.sh` per il ciclo di sviluppo.

Strumenti sulla macchina di sviluppo: `~/.cargo/bin/{wasm-bindgen 0.2.129, wasm-opt 123}`,
`rustup target wasm32-unknown-unknown`, `brotli`, `gzip`, `podman` (non `docker`).

## 5. Contratti condivisi

### 5.1 `hasher-core` (già compilante, nativo e wasm32)

- `algo::{Algo, AlgoGroup, to_hex, normalize_hex}` — identici al desktop (22 algoritmi, `DEFAULT` 9).
- `hasher::{StreamHasher, build, hash_bytes, ED2K_CHUNK}` — identici al desktop.
- `fileinfo::{FileInfo, FileAttr}` — stessa forma del desktop più `FileInfo::basic(path, size, modified)`.
- `result::{FileId, FileResult}` — identici al desktop (`hex()`, `matching_algos()`).
- `checksum_file::{ChecksumEntry, ChecksumFile, parse_text, parse_bytes(path, bytes), is_checksum_file(path), algo_from_path, CHECKSUM_EXTENSIONS}` — come il desktop ma senza file system.
- `export::{ExportFormat (+ mime(), suggested_file_name()), ExportLabels, ExportOptions, export_to_vec(format, results, opts) -> anyhow::Result<Vec<u8>>}`.
- `APP_NAME`, `APP_VERSION`, `COMPANY`, `COMPANY_URL`.
- feature `worker` (solo wasm32): `worker::worker_main()` — §5.3.

**Nessun altro crate modifica le firme pubbliche sopra.** Aggiunte compatibili (nuove funzioni) sono ammesse e vanno annotate nel riepilogo finale dell'agente.

### 5.2 `hasher-web-app`

- `settings::{Settings, Language, ThemeMode}` — stessi campi del desktop (`window` ignorato), con
  `Settings::load()` / `save()` su `localStorage["hasher.settings"]` (JSON, lettura tollerante come il desktop).
- `engine::{EngineConfig, EngineEvent, EngineHandle, WebEngine}` — stesso contratto di eventi del
  desktop (`Discovered { id, path, size }`, `Started`, `Progress { id, bytes_done }`, `Finished(Box<FileResult>)`,
  `Failed`, `AllDone { files_ok, files_failed, cancelled, elapsed }`) con `path: PathBuf` = percorso
  relativo; input `Vec<WebInput>` con `WebInput { file: web_sys::File, rel_path: String }`; gli eventi
  finiscono in una coda `Rc<RefCell<VecDeque<EngineEvent>>>` riempita dai callback `onmessage` dei
  worker (ogni callback chiama `ctx.request_repaint()`) e svuotata in `App::logic`.
- `web::dnd` — listener `dragover`/`drop` in **capture** sul canvas: a `drop` legge sincronamente
  `dataTransfer.items[i].webkitGetAsEntry()`, `stopImmediatePropagation()` + `preventDefault()`,
  legge i modificatori (`shiftKey`, `ctrlKey`/`metaKey`), poi esplora le cartelle in modo asincrono e
  consegna `Vec<WebInput>` all'app (coda + repaint); infine invia al canvas un `dragleave` sintetico
  così eframe azzera `hovered_files` (l'overlay di trascinamento continua a funzionare tramite
  `hovered_files` del `dragover` di eframe, che resta attivo).
- `web::picker` — `open_files()`, `open_folder()`, `open_checksum()` via `<input type="file">`.
- `web::download(name, mime, bytes)` — `Blob` + `<a download>` + `URL.revokeObjectURL`.
- `web::storage` — get/set su `localStorage`.
- `web::install` — `hasherCanInstall()` / `hasherInstall()` (vedi `web/boot.js`).
- `web::keys` — `keydown` in capture per `preventDefault` delle scorciatoie dell'app.
- `web::launch` — coda `hasherLaunchFiles` + evento `hasher:launch-files`.

### 5.3 Protocollo del Web Worker (oggetti JS via `postMessage`)

Ingresso (thread principale → worker):
- `{ cmd: "hash", task: number, file: File, algos: string[] /* Algo::id */, chunk?: number /* default 4 MiB */ }`
- `{ cmd: "cancel", task: number }` (`task = -1`: tutti)

Uscita (worker → thread principale):
- `{ ev: "ready" }`
- `{ ev: "started", task }`
- `{ ev: "progress", task, bytes }` (≤ 20/s per task)
- `{ ev: "done", task, bytes, elapsed_ms, digests: { "<algo id>": "<hex minuscolo>" } }`
- `{ ev: "failed", task, error }`
- `{ ev: "cancelled", task }`

Il worker legge `file.slice(off, off+chunk).arrayBuffer()` in un ciclo `async`, aggiorna gli hasher
(`hasher::build` per ciascun algoritmo), controlla l'annullamento fra un blocco e l'altro (i messaggi
`cancel` vengono consegnati durante gli `await`). Un `File` modificato durante la lettura produce
`failed` (`NotReadableError`). Il worker è avviato da `web/worker.js` (`import init, { worker_main } from './core.js'`).

### 5.4 `dist/` e server

```
dist/ index.html style.css boot.js app.js app_bg.wasm worker.js core.js core_bg.wasm
      manifest.webmanifest sw.js favicon.ico icons/*.png icons/icon.svg  (+ *.br, *.gz)
```

Server (`hasher-web-server`):
- `GET /healthz` → `200 text/plain "ok"`; `GET /` → `index.html`; `GET /<file>` → file di `dist/`
  (`Content-Type` da estensione; `.wasm` = `application/wasm`, `.webmanifest` = `application/manifest+json`);
  404 per il resto; nessun'altra rotta.
- `Cache-Control`: `index.html`, `sw.js`, `manifest.webmanifest` → `no-cache`; altri → `public, max-age=3600`
  (il service worker ha una cache propria versionata). `ETag` dall'hash del contenuto.
- Precompressi: se esiste `<file>.br`/`<file>.gz` e `Accept-Encoding` lo consente → `Content-Encoding` + `Vary: Accept-Encoding`.
- Sicurezza: `X-Content-Type-Options: nosniff`, `Referrer-Policy: strict-origin-when-cross-origin`,
  `Permissions-Policy` minimale, `Content-Security-Policy: default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; manifest-src 'self'; base-uri 'none'; frame-ancestors 'none'; object-src 'none'`.
- Configurazione via ambiente: `HASHER_WEB_BIND` (default `0.0.0.0:8080`), `HASHER_WEB_PREFIX`
  (default `/`; con `/hasher` tutte le rotte vengono annidate sotto il prefisso), `RUST_LOG`.
- Spegnimento ordinato su SIGTERM/SIGINT; `hasher-web-server --healthcheck` (GET `/healthz` su
  `HASHER_WEB_BIND`, exit 0/1) per l'`HEALTHCHECK` dell'immagine distroless.
- Dietro al proxy: HTTPS obbligatorio per PWA/service worker; il proxy deve passare `Accept-Encoding`
  (o comprimere da sé) e non riscrivere i corpi; nessun header `X-Forwarded-*` necessario (tutti gli URL sono relativi).

## 6. Ripartizione del lavoro

| Fase | Modulo | Agente | Motivo |
|---|---|---|---|
| 0 | Scheletro: workspace, `hasher-core` compilante, asset, `web/`, script di build, server minimo, piano | coordinatore (Fable) | definisce i contratti |
| 1A | `hasher-core`: `worker.rs` (protocollo §5.3), test con vettori noti e `wasm-bindgen-test`/verifica wasm, porting dei test d'integrazione del desktop (`tests/hash_vectors.rs`, `checksum_file.rs`, `export.rs`) | **Opus** | correttezza crittografica, I/O asincrono nel worker |
| 1B | `hasher-web-app`: port di `src/ui/*` e `settings.rs` al web, `engine/` (pool di worker), `web/` (dnd, picker, download, storage, keys, install, launch), traduzioni adattate, voce "Installa", build end-to-end con `scripts/build-web.sh` | **Opus** | la parte più ampia e delicata |
| 1C | `hasher-web-server` completo (§5.4), Dockerfile e `compose.yaml` verificati con podman, `deploy/` (nginx, Caddy, Traefik), `.github/workflows/{ci,release}.yml`, README completo | **Sonnet** | configurazione standard ben specificata |
| 2 | Integrazione, build release, test nel browser (Playwright), confronto con gli screenshot del desktop | coordinatore | |
| 3 | Rifiniture, documentazione, release | coordinatore + Sonnet | |

Regole operative per gli agenti:
- `export PATH="$HOME/.cargo/bin:$PATH"` (rustup non è nel PATH della shell).
- **Ogni** invocazione di cargo va serializzata: `flock /tmp/hasher-web-cargo.lock cargo …`
  (`jobs = 2` è già in `.cargo/config.toml`; la macchina ha poca memoria libera).
- Non eseguire `cargo build --release` dell'app senza motivo: per verificare usare
  `cargo check --target wasm32-unknown-unknown -p hasher-web-app` e `cargo clippy --target wasm32-unknown-unknown -p hasher-web-app -- -D warnings`.
- Consultare le API reali in `~/.cargo/registry/src/index.crates.io-*/` (eframe-0.36.2, egui-0.36.2,
  web-sys-0.3.106, wasm-bindgen-0.2.129, axum-0.8.x) invece di andare a memoria.
- Non toccare i crate degli altri agenti; se serve una modifica a `hasher-core`, segnalarla nel riepilogo.
- Codice e commenti in italiano come nel desktop; messaggi di commit in inglese (li fa il coordinatore).
- `getrandom` 0.3 su wasm32: se compare nel grafo, abilitare in `.cargo/config.toml` il rustflag
  `--cfg getrandom_backend="wasm_js"` e la feature `wasm_js` del crate.

## 7. Criteri di accettazione

1. Trascinando un file compaiono nome, dimensione, data di modifica e tutti gli hash di default; i
   valori coincidono con `sha256sum`, `md5sum`, `rhash` (CRC32, ED2K, RIPEMD) e con l'app desktop.
2. Trascinando una cartella con 100 file la tabella si popola progressivamente, con barra globale,
   velocità, tempo stimato e annullamento funzionante.
3. Un file da ≥ 4 GB viene hashato senza aumento della memoria del browser (streaming).
4. Incollando un hash corretto appare la conferma verde con il nome dell'algoritmo; con uno sbagliato l'errore rosso.
5. Trascinando una cartella che contiene `SHA256SUMS` (o un `.sha256`) i file elencati vengono verificati con esito per riga.
6. Esportazione TXT/CSV/JSON/XLSX/DOCX/checksum scaricabile e apribile (editor, LibreOffice, Excel, Word).
7. Cambio lingua immediato fra le 5 lingue; tema che segue il sistema e override manuale.
8. Impostazioni persistenti fra una sessione e l'altra del browser.
9. Guida, scorciatoie e Informazioni accessibili da menu e con `F1`.
10. PWA installabile da Chrome/Edge (manifest valido, service worker, funziona offline alla seconda visita);
    "Apri con Hasher" su un file `.sha256` apre la verifica.
11. `cargo fmt --check`, `cargo clippy -- -D warnings` (nativo e wasm32) e `cargo test` puliti;
    `podman build` riuscito; il contenitore risponde su `:8080` e funziona dietro a nginx con TLS.
12. Aspetto sovrapponibile agli screenshot di `../hasher/docs/screenshots/`.

## 8. Rischi e mitigazioni

| Rischio | Mitigazione |
|---|---|
| File enormi in memoria | mai `DroppedFile::bytes_async`; solo `web_file()` + `slice()` nel worker |
| `std::time::Instant` panica su wasm | `web_time::Instant` nell'app; nel worker `performance.now()` |
| `SystemTime::now()` in `rust_xlsxwriter` | feature `wasm` del crate (già nel manifest per wasm32) |
| `docx-rs` trascina `image` (wasm più grande) | accettato; brotli riduce molto; `opt-level = 3` + `wasm-opt` |
| CSP e `wasm-unsafe-eval` | eframe non usa `eval`: basta `'wasm-unsafe-eval'` |
| Transient activation per `input.click()` | l'azione egui scatta nel frame successivo al clic: entro il limite dei browser |
| Memoria in compilazione | `jobs = 2`, `flock`, LTO fat solo in release, un solo `cargo` alla volta |
| Versione CLI wasm-bindgen ≠ crate | pin `=0.2.129` nel workspace, verifica nello script e nel Dockerfile |
