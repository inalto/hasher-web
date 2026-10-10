<p align="center">
  <img src="crates/hasher-web-app/assets/brand/logo.svg" alt="Martini Multimedia" width="220">
</p>

<h1 align="center">Hasher Web</h1>

<p align="center">
  La versione web (PWA) di <a href="https://github.com/inalto/hasher">Hasher</a>: calcola e verifica gli hash
  dei tuoi file <b>direttamente nel browser</b>. Nessun file viene inviato online.
</p>

> Stato: in costruzione. Vedi [docs/PIANO.md](docs/PIANO.md).

Tutto in Rust: l'interfaccia è la stessa dell'app desktop (egui) compilata in WebAssembly, gli hash
vengono calcolati in Web Worker (WebAssembly) leggendo i file a blocchi, e un piccolo server
[axum](https://github.com/tokio-rs/axum) serve i file statici in un contenitore Docker dietro a un
reverse proxy. L'app è installabile come PWA (barra delle applicazioni di Windows, dock di macOS).

© Martini Multimedia s.a.s.
