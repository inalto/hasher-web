// Web Worker che calcola gli hash: carica il core wasm e cede il controllo a Rust (`worker_main`).
import init, { worker_main } from './core.js';
await init();
worker_main();
