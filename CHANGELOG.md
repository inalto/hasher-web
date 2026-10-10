# Changelog

Tutte le modifiche rilevanti a Hasher Web sono documentate in questo file.
Il formato segue [Keep a Changelog](https://keepachangelog.com/it/1.1.0/) e il progetto adotta il
[Versionamento Semantico](https://semver.org/lang/it/).

## [0.1.1] - 2026-10-10

Prima versione pubblicata (immagine `ghcr.io/inalto/hasher-web:0.1.1`).

### Aggiunto

- Interfaccia dell'app desktop Hasher (egui) compilata in WebAssembly: stesse viste (file singolo,
  elenco, verifica, confronto, esportazione), stesso tema chiaro/scuro, stessi font e icone, 5 lingue.
- Calcolo dei 22 algoritmi in Web Worker (WebAssembly), con lettura dei file a blocchi: nessun file
  viene caricato per intero né inviato al server. Con un solo file gli algoritmi vengono ripartiti fra
  i worker.
- Trascinamento di file e cartelle (esplorate ricorsivamente), selettori di file e cartelle,
  verifica con hash incollato, file di checksum risolti fra i file rilasciati insieme, confronto fra
  due file, esportazione in TXT, CSV, JSON, XLSX, DOCX e file di checksum (con percorso relativo).
- Layout compatto per smartphone (sotto i 700 px): menu ☰, pulsanti a tutta larghezza, hash su due
  righe, verifica a schermo intero, finestre dentro lo schermo, comandi touch.
- PWA installabile (manifest con `file_handlers` per i file di checksum, service worker con cache
  versionata, uso offline, voce "Installa come app"); meta per iOS e Android, safe area.
- Impostazioni salvate nel `localStorage` del browser (stesso formato JSON del desktop).
- Server `hasher-web-server` (axum): file statici embeddati e precompressi (brotli/gzip), `ETag`,
  header di sicurezza e CSP, `/healthz`, prefisso di percorso configurabile, spegnimento ordinato,
  `--healthcheck`.
- Immagine Docker multi-stage (runtime distroless, utente non privilegiato, healthcheck), esempi di
  deploy per nginx, Caddy, Traefik e Apache, guida `docs/DEPLOY.md`, workflow CI e release.

### Corretto

- Le righe hash che vanno a capo su tre righe non si sovrappongono più alla riga successiva.

## [0.1.0] - 2026-10-10

Versione iniziale di sviluppo, non pubblicata.
