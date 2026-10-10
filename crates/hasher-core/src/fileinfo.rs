//! Metadati di un file: nome, dimensione, date, attributi.
//!
//! Stessa forma del desktop, ma senza alcuna lettura dal file system: sul web i valori arrivano
//! dall'oggetto `File` del browser (nome, percorso relativo nella cartella trascinata, dimensione,
//! data di modifica); data di creazione, attributi e permessi non sono disponibili e restano
//! `None` / vuoti, così export e interfaccia continuano a funzionare senza casi speciali.

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Attributi mostrati all'utente. `id()` è la chiave di traduzione (`attr.<id>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FileAttr {
    ReadOnly,
    Hidden,
    System,
    Archive,
    Compressed,
    Encrypted,
    Temporary,
    Sparse,
    Offline,
    NotIndexed,
    ReparsePoint,
    Symlink,
    Executable,
    SetUid,
    SetGid,
    Sticky,
}

impl FileAttr {
    pub fn id(self) -> &'static str {
        match self {
            FileAttr::ReadOnly => "read_only",
            FileAttr::Hidden => "hidden",
            FileAttr::System => "system",
            FileAttr::Archive => "archive",
            FileAttr::Compressed => "compressed",
            FileAttr::Encrypted => "encrypted",
            FileAttr::Temporary => "temporary",
            FileAttr::Sparse => "sparse",
            FileAttr::Offline => "offline",
            FileAttr::NotIndexed => "not_indexed",
            FileAttr::ReparsePoint => "reparse_point",
            FileAttr::Symlink => "symlink",
            FileAttr::Executable => "executable",
            FileAttr::SetUid => "setuid",
            FileAttr::SetGid => "setgid",
            FileAttr::Sticky => "sticky",
        }
    }

    /// Lettera breve in stile Windows (R, H, S, A, …) per la vista compatta.
    pub fn short(self) -> &'static str {
        match self {
            FileAttr::ReadOnly => "R",
            FileAttr::Hidden => "H",
            FileAttr::System => "S",
            FileAttr::Archive => "A",
            FileAttr::Compressed => "C",
            FileAttr::Encrypted => "E",
            FileAttr::Temporary => "T",
            FileAttr::Sparse => "P",
            FileAttr::Offline => "O",
            FileAttr::NotIndexed => "I",
            FileAttr::ReparsePoint => "L",
            FileAttr::Symlink => "L",
            FileAttr::Executable => "X",
            FileAttr::SetUid => "U",
            FileAttr::SetGid => "G",
            FileAttr::Sticky => "K",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    /// Sul web: nome del file oppure percorso relativo alla cartella trascinata (`cartella/sub/file`).
    pub path: PathBuf,
    pub name: String,
    /// Estensione in minuscolo, senza punto; `None` se assente.
    pub extension: Option<String>,
    pub size: u64,
    pub created: Option<DateTime<Local>>,
    pub modified: Option<DateTime<Local>>,
    pub accessed: Option<DateTime<Local>>,
    pub attributes: Vec<FileAttr>,
    /// Bit dei permessi POSIX (es. `0o644`); mai disponibili sul web.
    pub unix_mode: Option<u32>,
    pub is_symlink: bool,
}

impl FileInfo {
    /// Metadati essenziali da nome/percorso, dimensione e data di modifica (tutto il resto assente).
    pub fn basic(path: impl Into<PathBuf>, size: u64, modified: Option<DateTime<Local>>) -> Self {
        let path: PathBuf = path.into();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .filter(|e| !e.is_empty());
        FileInfo {
            path,
            name,
            extension,
            size,
            created: None,
            modified,
            accessed: None,
            attributes: Vec::new(),
            unix_mode: None,
            is_symlink: false,
        }
    }

    /// Dimensione leggibile (es. `12,3 MB`) in base 10 come i file manager.
    pub fn size_human(&self) -> String {
        humansize::format_size(self.size, humansize::DECIMAL)
    }

    /// Permessi in stile `ls -l` (`rwxr-xr-x`), se disponibili.
    pub fn unix_mode_string(&self) -> Option<String> {
        let m = self.unix_mode?;
        let mut s = String::with_capacity(9);
        for (shift, special) in [(6u32, 0o4000u32), (3, 0o2000), (0, 0o1000)] {
            let r = (m >> shift) & 0o4 != 0;
            let w = (m >> shift) & 0o2 != 0;
            let x = (m >> shift) & 0o1 != 0;
            let sp = m & special != 0;
            s.push(if r { 'r' } else { '-' });
            s.push(if w { 'w' } else { '-' });
            s.push(match (x, sp, shift) {
                (true, true, 0) => 't',
                (false, true, 0) => 'T',
                (true, true, _) => 's',
                (false, true, _) => 'S',
                (true, false, _) => 'x',
                (false, false, _) => '-',
            });
        }
        Some(s)
    }

    /// Attributi come stringa compatta in stile Windows (es. `RHA`), vuota se nessuno.
    pub fn attributes_short(&self) -> String {
        self.attributes.iter().map(|a| a.short()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_fills_name_and_extension() {
        let i = FileInfo::basic("foto/Vacanze.JPG", 10, None);
        assert_eq!(i.name, "Vacanze.JPG");
        assert_eq!(i.extension.as_deref(), Some("jpg"));
        assert_eq!(i.size, 10);
        assert!(i.attributes.is_empty() && i.unix_mode.is_none() && !i.is_symlink);
        assert_eq!(FileInfo::basic("Makefile", 0, None).extension, None);
        assert_eq!(FileInfo::basic(".bashrc", 0, None).extension, None);
    }

    #[test]
    fn unix_mode_string_renders_ls_style() {
        let mut i = FileInfo::basic("x", 0, None);
        for (mode, expected) in [
            (0o644, "rw-r--r--"),
            (0o755, "rwxr-xr-x"),
            (0o4755, "rwsr-xr-x"),
            (0o4644, "rwSr--r--"),
            (0o2755, "rwxr-sr-x"),
            (0o1777, "rwxrwxrwt"),
            (0o1776, "rwxrwxrwT"),
        ] {
            i.unix_mode = Some(mode);
            assert_eq!(i.unix_mode_string().as_deref(), Some(expected), "{mode:o}");
        }
        i.unix_mode = None;
        assert_eq!(i.unix_mode_string(), None);
    }

    #[test]
    fn attributes_short_concatenates_letters() {
        let mut i = FileInfo::basic("x", 0, None);
        assert_eq!(i.attributes_short(), "");
        i.attributes = vec![FileAttr::ReadOnly, FileAttr::Hidden, FileAttr::Archive];
        assert_eq!(i.attributes_short(), "RHA");
    }
}
