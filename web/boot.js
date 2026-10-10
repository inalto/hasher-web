// Avvio della PWA: service worker, prompt di installazione, file aperti dal sistema, app wasm.
// Tutto resta nel browser: nessuna richiesta di rete oltre ai file statici dell'app.

// --- Installazione (Chrome/Edge): il prompt viene conservato e mostrato su richiesta dell'app.
let installPrompt = null;
window.addEventListener('beforeinstallprompt', (e) => {
  e.preventDefault();
  installPrompt = e;
  window.dispatchEvent(new CustomEvent('hasher:installable'));
});
window.addEventListener('appinstalled', () => { installPrompt = null; });
/** Chiamata da Rust: mostra il prompt di installazione se disponibile. Restituisce true se mostrato. */
window.hasherInstall = async () => {
  if (!installPrompt) return false;
  const p = installPrompt;
  installPrompt = null;
  p.prompt();
  await p.userChoice;
  return true;
};
window.hasherCanInstall = () => installPrompt !== null;

// --- File aperti con la PWA (file_handlers del manifest / "Apri con"): coda letta da Rust.
window.hasherLaunchFiles = [];
if ('launchQueue' in window) {
  window.launchQueue.setConsumer(async (launchParams) => {
    for (const handle of launchParams.files || []) {
      try { window.hasherLaunchFiles.push(await handle.getFile()); } catch (e) { console.warn('launchQueue', e); }
    }
    window.dispatchEvent(new CustomEvent('hasher:launch-files'));
  });
}

// --- Service worker (precache versionata, funzionamento offline).
if ('serviceWorker' in navigator) {
  window.addEventListener('load', () => {
    navigator.serviceWorker.register('sw.js').catch((e) => console.warn('service worker', e));
  });
}

// --- App egui (wasm-bindgen --target web): `init()` esegue `main` del crate hasher-web-app.
import init from './app.js';
try {
  await init();
} catch (e) {
  const splash = document.getElementById('splash');
  if (splash) {
    splash.innerHTML = '<p class="error">Impossibile avviare Hasher: ' +
      String(e).replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c])) + '</p>';
  }
  console.error(e);
}
