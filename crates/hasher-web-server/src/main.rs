//! Hasher Web — eseguibile del server. Vedi la documentazione della libreria (`lib.rs`).

use std::process::ExitCode;
use std::sync::Arc;

use hasher_web_server::{
    healthcheck, router, serve, shutdown_signal, Assets, Config, Dist, EmbedAssets, SHUTDOWN_GRACE,
};

const HELP: &str = "\
Hasher Web — server HTTP della PWA (Martini Multimedia s.a.s.)

USO:
    hasher-web-server               avvia il server
    hasher-web-server --healthcheck GET /healthz sul server locale; exit 0 se risponde 200, 1 altrimenti
    hasher-web-server --version     mostra la versione
    hasher-web-server --help        mostra questo aiuto

VARIABILI D'AMBIENTE:
    HASHER_WEB_BIND     indirizzo di ascolto (predefinito 0.0.0.0:8080)
    HASHER_WEB_PREFIX   prefisso di percorso, es. /hasher (predefinito /)
    RUST_LOG            livello dei log (predefinito info; debug per il log d'accesso)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => run(),
        ["--healthcheck"] => check(),
        ["--version" | "-V"] => {
            println!("hasher-web-server {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        ["--help" | "-h"] => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("argomenti non riconosciuti: {}\n\n{HELP}", args.join(" "));
            ExitCode::from(2)
        }
    }
}

/// `--healthcheck`: usato dall'`HEALTHCHECK` dell'immagine.
fn check() -> ExitCode {
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    match healthcheck(&config.bind, &config.prefix) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("healthcheck fallito: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> ExitCode {
    tracing_subscriber::fmt()
        // Colori solo su un terminale: `docker logs` / journald non devono ricevere sequenze ANSI.
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(e) => {
            tracing::error!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => {
            tracing::error!("impossibile avviare il runtime tokio: {e}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run_server(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

async fn run_server(config: Config) -> Result<(), String> {
    let assets: Arc<dyn Assets> = Arc::new(EmbedAssets::<Dist>::new());
    if assets.get("index.html").is_none() {
        tracing::warn!("l'app non è inclusa: manca index.html in dist/ (eseguire scripts/build-web.sh prima di compilare il server)");
    }
    let app = router(assets, &config.prefix)?;
    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await
        .map_err(|e| format!("impossibile ascoltare su {}: {e}", config.bind))?;
    let local = listener.local_addr().map_err(|e| e.to_string())?;
    tracing::info!(
        "Hasher Web {} in ascolto su http://{local}{}/",
        env!("CARGO_PKG_VERSION"),
        config.prefix
    );
    serve(listener, app, shutdown_signal(), SHUTDOWN_GRACE)
        .await
        .map_err(|e| format!("errore del server: {e}"))?;
    tracing::info!("server arrestato");
    Ok(())
}
