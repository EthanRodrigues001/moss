//! A set of files held in memory: the local Project, the Base snapshot, or what Overleaf has.
//!
//! Text files are stored with LF line endings, so a Notepad edit that only changed line endings is
//! not a change, and what Moss uploads is always LF (what Overleaf itself stores).

use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::fsutil::list_files;
use crate::ignore::{is_ignored, normalize_to_lf};
use crate::names::check_path;
use crate::project::META_DIR;
use crate::workspace::{Skipped, clean_zip_name, common_wrapper};
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob {
    pub bytes: Vec<u8>,
    pub hash: String,
}

impl Blob {
    /// Build a blob for `path`, converting line endings for text files.
    pub fn new(path: &str, bytes: &[u8]) -> Blob {
        let bytes = normalize_to_lf(path, bytes).into_owned();
        let hash = hash_of(&bytes);
        Blob { bytes, hash }
    }

    /// The content as text, when it is valid UTF-8 without NUL bytes. Anything else is binary.
    pub fn text(&self) -> Option<&str> {
        if self.bytes.contains(&0) {
            return None;
        }
        std::str::from_utf8(&self.bytes).ok()
    }
}

pub fn hash_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone, Default)]
pub struct FileSet {
    pub files: BTreeMap<String, Blob>,
}

impl FileSet {
    pub fn get(&self, path: &str) -> Option<&Blob> {
        self.files.get(path)
    }

    pub fn insert(&mut self, path: &str, bytes: &[u8]) {
        self.files.insert(path.to_string(), Blob::new(path, bytes));
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Path to hash, for comparing sets and for the Review's staleness check.
    pub fn hashes(&self) -> BTreeMap<&str, &str> {
        self.files
            .iter()
            .map(|(p, b)| (p.as_str(), b.hash.as_str()))
            .collect()
    }

    /// One digest of every path and hash, so two sets can be compared with one string.
    pub fn digest(&self) -> String {
        let mut h = Sha256::new();
        for (path, blob) in &self.files {
            h.update(path.as_bytes());
            h.update(b"\t");
            h.update(blob.hash.as_bytes());
            h.update(b"\n");
        }
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The Project's files on disk (Ignore list applied, `.moss/` excluded).
    pub fn from_project_dir(dir: &Path) -> Result<FileSet> {
        let mut set = FileSet::default();
        for entry in list_files(dir)? {
            if entry.is_dir {
                continue;
            }
            let bytes = fs::read(dir.join(&entry.path))
                .map_err(|e| Error::io(format!("reading {}", entry.path), e))?;
            set.insert(&entry.path, &bytes);
        }
        Ok(set)
    }

    /// The Base snapshot (`.moss/base/`). A Project that was never synced has an empty one.
    pub fn from_base(project_dir: &Path) -> Result<FileSet> {
        let base = project_dir.join(META_DIR).join("base");
        let mut set = FileSet::default();
        if !base.is_dir() {
            return Ok(set);
        }
        for entry in WalkDir::new(&base).min_depth(1).into_iter().flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&base) else { continue };
            let rel = rel.to_string_lossy().replace('\\', "/");
            let bytes = fs::read(entry.path())
                .map_err(|e| Error::io(format!("reading base copy of {rel}"), e))?;
            set.insert(&rel, &bytes);
        }
        Ok(set)
    }

    /// Files from a zip (what Overleaf's download gives). Entries on the Ignore list are dropped;
    /// names Windows cannot hold and names that differ only by case are returned as skipped.
    pub fn from_zip(bytes: &[u8]) -> Result<(FileSet, Vec<Skipped>)> {
        let mut archive =
            zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| Error::Zip(e.to_string()))?;
        let strip = common_wrapper(&mut archive);
        let mut set = FileSet::default();
        let mut skipped = Vec::new();
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| Error::Zip(e.to_string()))?;
            let raw = entry
                .name()
                .map(|n| n.into_owned())
                .unwrap_or_else(|_| "<unreadable name>".to_string());
            let Some(mut rel) = clean_zip_name(&raw) else {
                skipped.push(skip(&raw, "path escapes the project folder"));
                continue;
            };
            if let Some(prefix) = &strip {
                rel = rel.strip_prefix(prefix.as_str()).unwrap_or(&rel).to_string();
            }
            let rel = rel.trim_matches('/').to_string();
            if rel.is_empty() || entry.is_dir() || is_ignored(&rel) {
                continue;
            }
            if let Some((component, problem)) = check_path(&rel) {
                skipped.push(skip(&rel, &format!("`{component}` {problem}")));
                continue;
            }
            if let Some(first) = seen.get(&rel.to_lowercase()) {
                skipped.push(skip(&rel, &format!("differs only by case from `{first}`")));
                continue;
            }
            seen.insert(rel.to_lowercase(), rel.clone());
            let mut data = Vec::with_capacity(entry.size() as usize);
            entry
                .read_to_end(&mut data)
                .map_err(|e| Error::io(format!("extracting {rel}"), e))?;
            set.insert(&rel, &data);
        }
        Ok((set, skipped))
    }

    /// Pack the files into a zip (used to create an Overleaf project from a Local one).
    pub fn to_zip(&self) -> Result<Vec<u8>> {
        use std::io::Write;
        let mut out = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut out);
            let opts = zip::write::SimpleFileOptions::default();
            for (path, blob) in &self.files {
                zip.start_file(path.as_str(), opts)
                    .map_err(|e| Error::Zip(e.to_string()))?;
                zip.write_all(&blob.bytes)
                    .map_err(|e| Error::io(format!("zipping {path}"), e))?;
            }
            zip.finish().map_err(|e| Error::Zip(e.to_string()))?;
        }
        Ok(out.into_inner())
    }
}

fn skip(path: &str, reason: &str) -> Skipped {
    Skipped {
        path: path.to_string(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_text_hashes_like_lf_but_binary_is_untouched() {
        let a = Blob::new("main.tex", b"a\r\nb\r\n");
        let b = Blob::new("main.tex", b"a\nb\n");
        assert_eq!(a.hash, b.hash);
        let img = Blob::new("fig.png", b"\x89PNG\r\n\x1a\n");
        assert_eq!(img.bytes, b"\x89PNG\r\n\x1a\n");
        assert!(img.text().is_none() || !img.bytes.contains(&0));
    }

    #[test]
    fn text_means_valid_utf8_without_nul() {
        assert!(Blob::new("a.tex", "héllo".as_bytes()).text().is_some());
        assert!(Blob::new("a.tex", &[0xff, 0xfe, 0x41]).text().is_none());
        assert!(Blob::new("a.tex", b"a\0b").text().is_none());
    }

    #[test]
    fn digest_changes_when_any_file_changes() {
        let mut a = FileSet::default();
        a.insert("a.tex", b"1");
        let before = a.digest();
        a.insert("a.tex", b"2");
        assert_ne!(before, a.digest());
        a.insert("b.tex", b"1");
        assert_ne!(a.digest(), before);
    }

    #[test]
    fn zip_round_trips_and_drops_ignored_files() {
        let mut set = FileSet::default();
        set.insert("main.tex", b"hello\n");
        set.insert("chapters/one.tex", b"one\n");
        set.insert("main.aux", b"junk");
        let (back, skipped) = FileSet::from_zip(&set.to_zip().unwrap()).unwrap();
        assert!(skipped.is_empty());
        assert_eq!(back.files.keys().collect::<Vec<_>>(), ["chapters/one.tex", "main.tex"]);
    }

    #[test]
    fn zip_reports_windows_unsafe_and_case_clashing_names() {
        let mut set = FileSet::default();
        set.insert("ok.tex", b"x");
        set.insert("aux.tex", b"x");
        set.insert("Dup.tex", b"x");
        set.insert("dup.tex", b"x");
        let (back, skipped) = FileSet::from_zip(&set.to_zip().unwrap()).unwrap();
        let names: Vec<_> = skipped.iter().map(|s| s.path.as_str()).collect();
        assert!(names.contains(&"aux.tex"), "{names:?}");
        assert_eq!(skipped.len(), 2, "{names:?}");
        assert!(back.get("ok.tex").is_some());
        assert!(back.get("aux.tex").is_none());
    }

    #[test]
    fn a_wrapper_folder_in_the_zip_is_stripped() {
        let mut set = FileSet::default();
        set.insert("Report/main.tex", b"x");
        set.insert("Report/img/a.png", b"y");
        let (back, _) = FileSet::from_zip(&set.to_zip().unwrap()).unwrap();
        assert_eq!(back.files.keys().collect::<Vec<_>>(), ["img/a.png", "main.tex"]);
    }
}
