//! The Ignore list, plus text/binary handling shared by Import, Sync and writes.

use std::borrow::Cow;

use serde::Serialize;

/// File extensions that LaTeX tools generate. They never take part in Sync or zip export.
const GENERATED_EXTENSIONS: &[&str] = &[
    "aux", "log", "bbl", "blg", "out", "toc", "lof", "lot", "fls", "fdb_latexmk", "bcf", "nav",
    "snm", "vrb", "xdv", "idx", "ind", "ilg", "glo", "gls", "glg",
];

/// Whole-name matches and suffixes that are generated or junk.
const GENERATED_SUFFIXES: &[&str] = &[".synctex.gz", ".synctex", ".run.xml", ".moss-tmp"];
const JUNK_NAMES: &[&str] = &[".DS_Store", "Thumbs.db", "desktop.ini"];

/// True when `rel` (a `/`-separated path relative to the Project) must be left out of Sync,
/// zip export and Import.
pub fn is_ignored(rel: &str) -> bool {
    let mut components = rel.split('/').peekable();
    while let Some(c) = components.next() {
        let is_last = components.peek().is_none();
        if c == ".moss" || c == ".git" || c == "__MACOSX" || c.starts_with("_minted") {
            return true;
        }
        if is_last && is_generated_file(c) {
            return true;
        }
    }
    false
}

fn is_generated_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if JUNK_NAMES.iter().any(|j| j.eq_ignore_ascii_case(name)) {
        return true;
    }
    if GENERATED_SUFFIXES.iter().any(|s| lower.ends_with(s)) {
        return true;
    }
    match lower.rsplit_once('.') {
        Some((stem, ext)) => !stem.is_empty() && GENERATED_EXTENSIONS.contains(&ext),
        None => false,
    }
}

const TEXT_EXTENSIONS: &[&str] = &[
    "tex", "bib", "sty", "cls", "bst", "bbx", "cbx", "lbx", "def", "cfg", "dtx", "ins", "ltx",
    "tikz", "pgf", "txt", "md", "csv", "tsv", "json", "xml", "yml", "yaml", "svg", "gitignore",
    "latexmkrc",
];

/// Text files are stored with LF line endings; everything else is left byte-for-byte alone.
pub fn is_text_path(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel).to_ascii_lowercase();
    if name == "latexmkrc" || name == ".latexmkrc" {
        return true;
    }
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| TEXT_EXTENSIONS.contains(&ext))
}

/// Convert CRLF (and lone CR) to LF for text files. Anything containing NUL bytes (UTF-16, or
/// a binary file with a text extension) is returned unchanged.
pub fn normalize_to_lf<'a>(rel: &str, bytes: &'a [u8]) -> Cow<'a, [u8]> {
    if !is_text_path(rel) || bytes.contains(&0) || !bytes.contains(&b'\r') {
        return Cow::Borrowed(bytes);
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' {
            out.push(b'\n');
            if bytes.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    Cow::Owned(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    Tex,
    Bib,
    Style,
    Class,
    Bst,
    Image,
    Pdf,
    Asset,
    Folder,
}

pub fn kind_of(rel: &str, is_dir: bool) -> FileKind {
    if is_dir {
        return FileKind::Folder;
    }
    let ext = rel
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "tex" | "ltx" => FileKind::Tex,
        "bib" => FileKind::Bib,
        "sty" => FileKind::Style,
        "cls" => FileKind::Class,
        "bst" => FileKind::Bst,
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "bmp" | "webp" | "eps" | "tif" | "tiff" => {
            FileKind::Image
        }
        "pdf" => FileKind::Pdf,
        _ => FileKind::Asset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_files_are_ignored() {
        for p in [
            "main.aux",
            "main.log",
            "chapters/intro.aux",
            "main.synctex.gz",
            "main.fdb_latexmk",
            "main.bbl",
            ".moss/project.json",
            "sub/.git/config",
            "_minted-main/cache.pygtex",
            "Thumbs.db",
            "main.tex.moss-tmp",
        ] {
            assert!(is_ignored(p), "{p} should be ignored");
        }
    }

    #[test]
    fn real_sources_are_kept() {
        for p in [
            "main.tex",
            "refs.bib",
            "figures/plot.pdf",
            "fig.png",
            "chapters/auxiliary.tex",
            "log.tex",
            "out/figure.png",
        ] {
            assert!(!is_ignored(p), "{p} should be kept");
        }
    }

    #[test]
    fn crlf_becomes_lf_in_text_files_only() {
        let n = normalize_to_lf("a.tex", b"one\r\ntwo\r\nthree\rfour");
        assert_eq!(&*n, b"one\ntwo\nthree\nfour");
        let bin = normalize_to_lf("a.png", b"\r\n\r\n");
        assert_eq!(&*bin, b"\r\n\r\n");
        let utf16 = normalize_to_lf("a.tex", b"a\0\r\0\n\0");
        assert_eq!(&*utf16, b"a\0\r\0\n\0");
    }

    #[test]
    fn already_lf_is_not_copied() {
        assert!(matches!(normalize_to_lf("a.tex", b"x\ny\n"), Cow::Borrowed(_)));
    }

    #[test]
    fn kinds() {
        assert_eq!(kind_of("a.TEX", false), FileKind::Tex);
        assert_eq!(kind_of("figs", true), FileKind::Folder);
        assert_eq!(kind_of("x.jpg", false), FileKind::Image);
        assert_eq!(kind_of("x.xyz", false), FileKind::Asset);
    }
}
