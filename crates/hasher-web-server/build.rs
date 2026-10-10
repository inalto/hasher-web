// In release `rust-embed` incorpora `dist/` nel binario a tempo di compilazione, ma cargo non si
// accorge da solo di file aggiunti o rimossi nella cartella: questa direttiva fa ricompilare il
// crate quando `scripts/build-web.sh` rigenera `dist/` (anche con la cache di target del Dockerfile).
// In debug `dist/` viene letto dal disco a ogni richiesta, quindi non serve ricompilare.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        println!("cargo:rerun-if-changed=../../dist");
    }
}
