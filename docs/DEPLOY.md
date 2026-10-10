# Hasher Web — installazione e gestione

Questa guida spiega come mettere online **Hasher Web** con Docker o Podman dietro a un reverse proxy con HTTPS.
Il server (`hasher-web-server`, axum) serve soltanto file statici e `/healthz`: non ha database, non ha volumi, non
riceve file. Gli hash sono calcolati nel browser dell'utente.

- [Requisiti](#requisiti)
- [Avvio con docker e podman](#avvio-con-docker-e-podman)
- [Avvio con compose](#avvio-con-compose)
- [Variabili d'ambiente](#variabili-dambiente)
- [Aggiornamento](#aggiornamento)
- [Healthcheck e log](#healthcheck-e-log)
- [Reverse proxy](#reverse-proxy)
- [Prefisso di percorso](#prefisso-di-percorso)
- [Note sulla PWA](#note-sulla-pwa)
- [Costruire l'immagine dal sorgente](#costruire-limmagine-dal-sorgente)
- [Risoluzione dei problemi](#risoluzione-dei-problemi)

## Requisiti

- Docker (con `docker compose`) oppure Podman.
- Un reverse proxy con **HTTPS** davanti al contenitore: nginx, Caddy, Traefik o Apache (esempi in [`deploy/`](../deploy/)).
  **HTTPS è obbligatorio** per la PWA: senza HTTPS il browser non registra il service worker e non offre
  l'installazione come app (fa eccezione solo `http://localhost`). Hasher Web funziona comunque come normale pagina web.
- Un nome di dominio che punti al server (per ottenere un certificato Let's Encrypt).

L'immagine pesa pochi MB (distroless, solo il binario), gira come utente non privilegiato (`65532`) e non scrive su
disco: può essere eseguita con file system in sola lettura e senza capability.

## Avvio con docker e podman

### docker

```sh
docker run -d --name hasher-web --restart unless-stopped \
  -p 127.0.0.1:8080:8080 \
  -e RUST_LOG=info \
  --read-only --cap-drop ALL --security-opt no-new-privileges \
  ghcr.io/inalto/hasher-web:latest
```

### podman

```sh
podman run -d --name hasher-web --restart unless-stopped \
  -p 127.0.0.1:8080:8080 \
  -e RUST_LOG=info \
  --read-only --cap-drop ALL --security-opt no-new-privileges \
  --health-cmd 'CMD /hasher-web-server --healthcheck' --health-interval 30s \
  ghcr.io/inalto/hasher-web:latest
```

Con podman l'`HEALTHCHECK` scritto nell'immagine può essere ignorato (dipende dal formato dell'immagine): per questo
`--health-cmd` lo dichiara esplicitamente. L'immagine non ha shell, quindi il comando va nella forma `CMD ...` e non
`CMD-SHELL`.

La porta è pubblicata solo su `127.0.0.1`: il contenitore è raggiungibile dal reverse proxy sullo stesso server ma non
direttamente da Internet. Per provarlo apri <http://localhost:8080>. Se il proxy gira su un altro host o in un altro
contenitore, pubblica la porta sull'interfaccia giusta (`-p 8080:8080`) e limitala con il firewall.

Controllo rapido:

```sh
curl -i http://127.0.0.1:8080/healthz       # 200 ok
docker ps --format '{{.Names}}\t{{.Status}}' # "healthy" dopo qualche secondo
```

### Avvio automatico con systemd (podman, Quadlet)

Con podman ≥ 4.4 (AlmaLinux/RHEL/Rocky 9.2+) si può gestire il contenitore come servizio systemd. Per un servizio di
sistema crea `/etc/containers/systemd/hasher-web.container` (per l'utente corrente: `~/.config/containers/systemd/`):

```ini
[Unit]
Description=Hasher Web
After=network-online.target

[Container]
Image=ghcr.io/inalto/hasher-web:latest
PublishPort=127.0.0.1:8080:8080
Environment=RUST_LOG=info
ReadOnly=true
DropCapability=ALL
NoNewPrivileges=true
HealthCmd=CMD /hasher-web-server --healthcheck
HealthInterval=30s
# Aggiornamento automatico con `podman auto-update` (timer podman-auto-update.timer)
AutoUpdate=registry

[Service]
Restart=always

[Install]
WantedBy=multi-user.target default.target
```

Poi: `systemctl daemon-reload && systemctl start hasher-web` (per l'utente: `systemctl --user daemon-reload`).

## Avvio con compose

Il file [`compose.yaml`](../compose.yaml) nella radice del progetto avvia il servizio `hasher-web`
(`ghcr.io/inalto/hasher-web:latest`, porta `127.0.0.1:8080`, riavvio automatico, healthcheck, file system in sola lettura):

```sh
docker compose up -d                 # scarica l'immagine se manca e avvia
docker compose up -d --build         # oppure la costruisce dal sorgente (vedi sotto)
docker compose logs -f hasher-web
docker compose down
```

Funziona anche con `podman compose` / `podman-compose`. Le variabili `HASHER_WEB_PREFIX` e `RUST_LOG` si possono
impostare nell'ambiente o in un file `.env` accanto a `compose.yaml`.

Compose completi con il proxy incluso, che ottengono da soli il certificato HTTPS:

| File | Proxy | Avvio |
|---|---|---|
| [`deploy/compose.caddy.yaml`](../deploy/compose.caddy.yaml) | Caddy | `HASHER_DOMAIN=hasher.example.com docker compose -f deploy/compose.caddy.yaml up -d` |
| [`deploy/compose.traefik.yaml`](../deploy/compose.traefik.yaml) | Traefik v3 | `HASHER_DOMAIN=hasher.example.com ACME_EMAIL=admin@example.com docker compose -f deploy/compose.traefik.yaml up -d` |

## Variabili d'ambiente

| Variabile | Predefinito | Significato |
|---|---|---|
| `HASHER_WEB_BIND` | `0.0.0.0:8080` | Indirizzo e porta su cui il server ascolta *dentro* il contenitore. Usata anche dall'healthcheck. |
| `HASHER_WEB_PREFIX` | `/` | Prefisso di percorso, per esempio `/hasher` (vedi [Prefisso di percorso](#prefisso-di-percorso)). Sono ammessi solo segmenti di lettere, cifre, `.`, `_`, `~` e `-`. |
| `RUST_LOG` | `info` | Livello dei log: `error`, `warn`, `info`, `debug`. Con `debug` viene scritto anche il log d'accesso (una riga per richiesta). |

Valori non validi (prefisso con `..`, porta occupata, indirizzo errato) fanno terminare il server con un messaggio
chiaro nei log e codice d'uscita 1.

Il binario ha anche tre opzioni: `--healthcheck`, `--version` e `--help`.

## Aggiornamento

I tag dell'immagine su GHCR: `latest` (ultima versione stabile), `X.Y.Z` e `vX.Y.Z` (esatta), `X.Y` (ultima patch) e
`sha-<commit>` (ogni push su `main`). In produzione conviene fissare una versione (`ghcr.io/inalto/hasher-web:0.1.0`).

```sh
# docker
docker pull ghcr.io/inalto/hasher-web:latest
docker rm -f hasher-web
docker run -d --name hasher-web ...                  # lo stesso comando di prima

# compose
docker compose pull && docker compose up -d          # ricrea solo il servizio se l'immagine è cambiata

# podman
podman pull ghcr.io/inalto/hasher-web:latest && podman rm -f hasher-web && podman run -d --name hasher-web ...
# oppure, con Quadlet: `podman auto-update` (o il timer podman-auto-update.timer)
```

L'interruzione è di pochi istanti e non si perde nulla: il server non ha stato. Dopo un aggiornamento i browser che
avevano già aperto Hasher scaricano la nuova versione in background e la usano alla visita successiva (vedi
[Note sulla PWA](#note-sulla-pwa)). Alla chiusura del contenitore il server si spegne in modo ordinato (SIGTERM):
le richieste in corso terminano, al massimo entro 8 secondi.

## Healthcheck e log

- `GET /healthz` risponde `200 ok` (con un prefisso: `GET <prefisso>/healthz`).
- Nel contenitore non ci sono shell né `curl`: l'`HEALTHCHECK` esegue il binario stesso,
  `hasher-web-server --healthcheck`, che fa un `GET /healthz` su `HASHER_WEB_BIND` (sostituendo `0.0.0.0` con
  `127.0.0.1`) e termina con 0 se riceve `200`, con 1 altrimenti. Lo stesso comando funziona a mano:
  `docker exec hasher-web /hasher-web-server --healthcheck; echo $?`.
- Stato: `docker inspect --format '{{.State.Health.Status}}' hasher-web` (podman: `podman healthcheck run hasher-web`).
- Log: `docker logs -f hasher-web` (stdout, senza colori). All'avvio compare
  `Hasher Web X.Y.Z in ascolto su http://0.0.0.0:8080/`; con `RUST_LOG=debug` anche una riga per richiesta
  (`tower_http::trace`). I log non contengono mai nomi o contenuti dei file dell'utente: il server non li riceve.

## Reverse proxy

Esempi commentati in [`deploy/`](../deploy/):

| File | Contenuto |
|---|---|
| [`nginx.conf.example`](../deploy/nginx.conf.example) | nginx con Let's Encrypt, HSTS, `gzip off`, variante sotto-percorso |
| [`Caddyfile.example`](../deploy/Caddyfile.example) | Caddy (HTTPS automatico), variante sotto-percorso |
| [`compose.caddy.yaml`](../deploy/compose.caddy.yaml) | Caddy + Hasher Web in un unico compose |
| [`compose.traefik.yaml`](../deploy/compose.traefik.yaml) | Traefik v3 + Hasher Web con le label e Let's Encrypt |
| [`apache.conf.example`](../deploy/apache.conf.example) | Apache httpd con `ProxyPass`, sotto-percorso |

Requisiti comuni, già rispettati dagli esempi:

- **HTTPS** sul proxy (certificato valido: Let's Encrypt va bene) e reindirizzamento da `http://` a `https://`.
- Passare `Accept-Encoding` al server **senza ricomprimere** e senza riscrivere i corpi: i file sono già precompressi
  (brotli e gzip) e il server sceglie la variante giusta, con `Vary: Accept-Encoding` e `ETag` per variante.
  In nginx `gzip off;`, in Apache `SetEnv no-gzip 1`, in Caddy niente `encode`.
- Non servono header `X-Forwarded-*`: tutti gli URL dell'applicazione sono relativi. Si possono inviare comunque.
- Non rimuovere gli header di sicurezza del server (CSP, `X-Content-Type-Options`, `Referrer-Policy`,
  `Permissions-Policy`): il proxy aggiunge solo HSTS.
- Nessuna cache sul proxy: ci pensano `ETag` e `Cache-Control` (`no-cache` per `index.html`, `sw.js` e
  `manifest.webmanifest`, un'ora per il resto).

### Header di sicurezza inviati dal server

`Content-Security-Policy: default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; style-src 'self'
'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; manifest-src 'self'; base-uri 'none'; frame-ancestors
'none'; object-src 'none'` — l'app non può contattare altri siti (`connect-src 'self'`); `blob:` serve ai download delle
esportazioni. Inoltre `X-Content-Type-Options: nosniff`, `Referrer-Policy: strict-origin-when-cross-origin` e
`Permissions-Policy: camera=(), microphone=(), geolocation=()`. Compaiono su *tutte* le risposte, errori compresi.

## Prefisso di percorso

Per servire Hasher Web a un indirizzo come `https://example.com/hasher/` ci sono due modi (esempi per ogni proxy in
[`deploy/`](../deploy/)):

1. **Il proxy toglie il prefisso** (`/hasher/boot.js` → `/boot.js`): il server resta alla radice e non serve alcuna
   configurazione. Funziona perché tutti gli URL dell'app sono relativi. nginx: `proxy_pass http://127.0.0.1:8080/;`
   (con la barra finale) in `location /hasher/`.
2. **Il proxy passa il percorso invariato** e il server lo conosce: avvia il contenitore con
   `-e HASHER_WEB_PREFIX=/hasher`. Tutte le rotte (anche `/healthz`) stanno allora sotto il prefisso:
   `/hasher/` → `index.html`, `/hasher/app.js`, `/hasher/healthz`; fuori dal prefisso c'è solo `404`.
   `/hasher` (senza barra finale) riceve un redirect `308` verso `/hasher/`: servire `index.html` a quell'indirizzo
   romperebbe gli URL relativi (`boot.js` verrebbe cercato in `/boot.js`).

Il service worker e il manifest usano lo scope `./`, cioè la cartella dell'applicazione: due installazioni su percorsi
diversi dello stesso dominio non si disturbano.

## Note sulla PWA

- **HTTPS è obbligatorio** (tranne `http://localhost`): senza, niente service worker, niente installazione, niente uso offline.
- **Installazione**: in Chrome e Edge compare l'icona *Installa* nella barra degli indirizzi (in alternativa dal menu del
  browser: Chrome *Installa Hasher…* oppure *Salva e condividi → Installa pagina come app*; Edge *App → Installa questo sito
  come app*); nell'app c'è anche **Aiuto → Installa come app**. Firefox su desktop non installa le PWA; Safari su macOS
  offre *Aggiungi al Dock*.
- **Smartphone e tablet**: su Android, Chrome propone *Installa app* (o *Aggiungi a schermata Home* dal menu ⋮);
  su iPhone e iPad, in Safari, *Condividi → Aggiungi alla schermata Home*. Sotto i 700 px di larghezza l'interfaccia
  passa al layout compatto (menu ☰, pulsanti a tutta larghezza, verifica a schermo intero); su iOS il selettore di
  cartelle non esiste e *Apri cartella* permette di scegliere singoli file.
- **Barra delle applicazioni e dock**: su Windows, con l'app installata e aperta, clic destro sull'icona nella barra →
  *Aggancia alla barra delle applicazioni*; su macOS l'app compare in
  *Applicazioni* e si trascina nel Dock.
- **Apri con**: dopo l'installazione, clic destro su un file `.sha256`, `.md5`, `.sha1`, `.sha512`, `.sfv`, … →
  *Apri con* → *Hasher* (alla prima volta Chrome/Edge chiede di confermare e si può impostare come app predefinita).
  Il file di checksum viene aperto direttamente in verifica; per controllare anche i file elencati, trascinali
  nella finestra insieme al file di checksum (il browser non permette di leggere i vicini senza che li scelga l'utente).
- **Uso offline**: alla prima visita il service worker scarica e memorizza tutti i file dell'applicazione
  (alcuni MB, soprattutto i `.wasm`); da quel momento Hasher si apre e calcola anche senza rete.
- **Aggiornamenti**: `sw.js` e `index.html` non vengono mai tenuti in cache dal browser (`no-cache`), quindi a ogni visita
  il browser controlla se esiste una versione nuova; se sì, la scarica in background e la usa alla visita seguente
  (o dopo un ricaricamento). Per forzare subito: `Ctrl+F5`, oppure strumenti per sviluppatori → *Application* →
  *Service Workers* → *Update*.
- **Privacy**: nessun file viene inviato al server. Il browser può mostrare «Caricare N file?» quando si sceglie una
  cartella: è il testo standard del selettore di cartelle, non avviene alcun upload. Le impostazioni sono nel
  `localStorage` del browser, non sul server.

## Costruire l'immagine dal sorgente

```sh
git clone https://github.com/inalto/hasher-web.git && cd hasher-web
podman build --format docker -t hasher-web .         # con Docker: docker build -t hasher-web .
podman run --rm -p 127.0.0.1:8080:8080 hasher-web
```

`--format docker` serve a podman per conservare l'`HEALTHCHECK` dell'immagine (il formato OCI lo scarta). Il
`Dockerfile` ha tre stadi: **tools** (Rust, target `wasm32`, `wasm-opt` e CLI `wasm-bindgen` alla versione di
`Cargo.lock`, scaricate come binari precompilati), **build** (`scripts/build-web.sh`, poi
`cargo build --release -p hasher-web-server` che embedda `dist/`) e **runtime** (`gcr.io/distroless/cc-debian12:nonroot`
con il solo binario). Opzioni utili:

- `--build-arg CARGO_BUILD_JOBS=8`: parallelismo di cargo (predefinito 2, pensato per macchine con poca RAM; la
  compilazione dell'app con LTO è onerosa in memoria).
- `--build-arg BINARYEN_VERSION=123`, `--build-arg WASM_BINDGEN_VERSION=...`: versioni degli strumenti. La CLI
  `wasm-bindgen` deve avere la stessa versione del crate in `Cargo.lock`: se lasci vuoto il secondo argomento viene letta da lì.
- Le build ripetute riusano registry e artefatti delle dipendenze tramite cache mount (`RUN --mount=type=cache`);
  supportati da Docker BuildKit e da podman.

Per compilare senza contenitori vedi «Compilare dal sorgente» nel [README](../README.md).

## Risoluzione dei problemi

**Il proxy risponde 502 / 503 (Bad Gateway / Service Unavailable)**
Il proxy non raggiunge il contenitore. Verifica che sia in esecuzione e `healthy` (`docker ps`), che risponda in locale
(`curl -i http://127.0.0.1:8080/healthz` → `200 ok`) e che l'indirizzo nel proxy coincida con la porta pubblicata
(`-p 127.0.0.1:8080:8080` ↔ `proxy_pass http://127.0.0.1:8080`). Se proxy e contenitore sono in due reti compose
diverse, il nome `hasher-web` non si risolve. Con `HASHER_WEB_PREFIX` impostato, `/healthz` esiste solo come
`<prefisso>/healthz`. Guarda `docker logs hasher-web` per errori di avvio (porta occupata, prefisso non valido).

**502 solo su AlmaLinux/RHEL/Rocky con nginx o Apache, mentre `curl` dal server funziona**
SELinux impedisce al proxy di collegarsi a una porta di rete. Abilita: `setsebool -P httpd_can_network_connect 1`.
Se anche il firewall blocca, `firewall-cmd --permanent --add-service=http --add-service=https && firewall-cmd --reload`.

**Pagina bianca, o l'errore «Impossibile avviare Hasher»**
Apri la console del browser (`F12`). Cause tipiche: `Content-Type` sbagliato per `.wasm` (vedi sotto); il proxy ha
rimosso o sostituito l'header `Content-Security-Policy` con uno più restrittivo; il proxy ricomprime o modifica i file
(`Content-Encoding` doppio, `ERR_CONTENT_DECODING_FAILED`: disattiva `gzip`/`encode` sul proxy); il browser non supporta WebGL2
o ha l'accelerazione hardware disattivata.

**Errore «Incorrect response MIME type. Expected 'application/wasm'»**
Il file `.wasm` non arriva con `Content-Type: application/wasm`. Il server lo imposta correttamente: la causa è un proxy
(o una CDN) che lo cambia. Controlla con `curl -sI https://tuo-dominio/app_bg.wasm | grep -i content-type` e verifica che
il proxy non applichi regole di `types` / `AddType` ai file proxati.

**Dopo un aggiornamento vedo ancora la versione vecchia (service worker)**
Il service worker serve la versione memorizzata finché non ne installa una nuova, cosa che avviene alla visita
successiva. Ricarica la pagina due volte oppure forza con `Ctrl+F5`. Se non basta: strumenti per sviluppatori →
*Application* → *Service Workers* → *Unregister*, poi *Storage* → *Clear site data*, e ricarica. Verifica anche che il proxy
non metta in cache `sw.js` o `index.html` (devono arrivare con `Cache-Control: no-cache`).

**Il browser non offre «Installa app»**
Servono HTTPS (o `localhost`), `manifest.webmanifest` raggiungibile con `Content-Type: application/manifest+json`, il
service worker registrato e un browser Chrome/Edge. Controlla in *Application* → *Manifest* quali requisiti mancano. Se hai
già installato l'app, il browser non la propone di nuovo.

**`/hasher` mostra una pagina che non carica, oppure dà 404**
Con un sotto-percorso scegli una sola modalità (vedi [Prefisso di percorso](#prefisso-di-percorso)): se il proxy
toglie il prefisso, il contenitore **non** deve avere `HASHER_WEB_PREFIX`; se il proxy lo passa, deve averlo. L'indirizzo
da usare ha la barra finale (`/hasher/`).

**Il contenitore risulta `unhealthy` con podman, ma funziona**
L'immagine non ha shell: l'healthcheck deve usare la forma `CMD` (`--health-cmd 'CMD /hasher-web-server --healthcheck'`) e,
se il comando è scritto nell'immagine, l'immagine va costruita con `--format docker`.

**Il file di checksum segnala MANCANTE per file che esistono**
Il browser non può cercare file sul disco: i file elencati vanno trascinati insieme al file di checksum (o la cartella che
li contiene). Il confronto avviene sul nome relativo e, in subordine, sul solo nome.
