//! The Workspace: the one folder that holds every Project, plus creating and importing them.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

use crate::fsutil;
use crate::ignore::is_ignored;
use crate::names::{check_path, sanitize_component};
use crate::project::{META_DIR, Origin, OverleafLink, Project, detect_root_file};
use crate::{Error, Result};

const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct ProjectSummary {
    /// Folder name inside `projects/`; the stable handle for a Project on this PC.
    pub dir_name: String,
    pub id: String,
    pub name: String,
    pub root_file: String,
    pub origin: Origin,
    pub overleaf: Option<OverleafLink>,
    pub archived: bool,
    /// When a source file last changed, in milliseconds since the Unix epoch.
    pub last_edited_ms: Option<u64>,
}

/// A file Import could not bring in, shown on the Needs attention list.
#[derive(Debug, Clone, Serialize)]
pub struct Skipped {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct ImportReport {
    pub project: ProjectSummary,
    pub files_imported: usize,
    pub ignored: usize,
    pub skipped: Vec<Skipped>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// `Documents\Moss` for the current user.
    pub fn default_root() -> PathBuf {
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        home.join("Documents").join("Moss")
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Workspace> {
        let root = root.into();
        for dir in [root.join("projects"), root.join(META_DIR)] {
            fs::create_dir_all(&dir)
                .map_err(|e| Error::io(format!("creating {}", dir.display()), e))?;
        }
        Ok(Workspace { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.root.join("projects")
    }

    pub fn open_project(&self, dir_name: &str) -> Result<Project> {
        if dir_name.contains(['/', '\\']) || dir_name.starts_with('.') {
            return Err(Error::UnsafePath(dir_name.to_string()));
        }
        let dir = self.projects_dir().join(dir_name);
        if !dir.is_dir() {
            return Err(Error::ProjectNotFound(dir_name.to_string()));
        }
        Project::open(&dir)
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>> {
        let entries = fs::read_dir(self.projects_dir())
            .map_err(|e| Error::io("listing projects", e))?;
        let mut out: Vec<ProjectSummary> = entries
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter_map(|e| {
                let dir_name = e.file_name().to_string_lossy().into_owned();
                let project = Project::open(&e.path()).ok()?;
                Some(summarize(&dir_name, &project))
            })
            .collect();
        out.sort_by_key(|p| p.name.to_lowercase());
        Ok(out)
    }

    /// Move a Project into or out of the Archive list. The files are not touched.
    pub fn set_archived(&self, dir_name: &str, archived: bool) -> Result<ProjectSummary> {
        let mut project = self.open_project(dir_name)?;
        project.meta.archived = archived;
        project.save_meta()?;
        Ok(summarize(dir_name, &project))
    }

    /// Change the name Moss shows for a Project. The folder name and any Overleaf link stay as they are.
    pub fn rename_project(&self, dir_name: &str, new_name: &str) -> Result<ProjectSummary> {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err(Error::Entry("A project needs a name.".into()));
        }
        if self
            .list_projects()?
            .iter()
            .any(|p| p.dir_name != dir_name && p.name.to_lowercase() == new_name.to_lowercase())
        {
            return Err(Error::Entry(format!("Another project is already called “{new_name}”.")));
        }
        let mut project = self.open_project(dir_name)?;
        project.meta.name = new_name.to_string();
        project.save_meta()?;
        Ok(summarize(dir_name, &project))
    }

    /// Delete a Project's folder from this PC, history and all. An Overleaf copy is not touched.
    pub fn delete_project(&self, dir_name: &str) -> Result<()> {
        let project = self.open_project(dir_name)?;
        let dir = project.dir().to_path_buf();
        fs::remove_dir_all(&dir).map_err(|e| Error::io(format!("deleting {}", dir.display()), e))
    }

    fn unique_dir(&self, wanted: &str) -> PathBuf {
        let base = sanitize_component(wanted);
        let mut candidate = self.projects_dir().join(&base);
        let mut n = 2;
        while candidate.exists() {
            candidate = self.projects_dir().join(format!("{base} ({n})"));
            n += 1;
        }
        candidate
    }

    /// A new empty Local Project with a starter `main.tex`.
    pub fn create_project(&self, name: &str) -> Result<ProjectSummary> {
        self.create_project_from(name, None)
    }

    /// A new Local Project from a starter document (see [`crate::templates`]), or the empty
    /// starter when `template` is `None`.
    pub fn create_project_from(&self, name: &str, template: Option<&str>) -> Result<ProjectSummary> {
        let chosen = match template {
            Some(id) => Some(crate::templates::find(id).ok_or_else(|| {
                let ids: Vec<&str> = crate::templates::all().iter().map(|t| t.id).collect();
                Error::Entry(format!("unknown template `{id}`; choose one of: {}", ids.join(", ")))
            })?),
            None => None,
        };
        let dir = self.unique_dir(name);
        fs::create_dir_all(&dir).map_err(|e| Error::io(format!("creating {}", dir.display()), e))?;
        let root_file = match chosen {
            Some(t) => {
                for (path, text) in t.files {
                    fsutil::write_text(&dir, path, text)?;
                }
                t.root_file
            }
            None => {
                let starter = "\\documentclass{article}\n\\title{Untitled document}\n\\author{}\n\\date{\\today}\n\\begin{document}\n\\maketitle\nHello from Moss.\n\\end{document}\n";
                fsutil::write_text(&dir, "main.tex", starter)?;
                "main.tex"
            }
        };
        let project = Project::init(&dir, name, root_file)?;
        Ok(summarize(&dir_name_of(&dir), &project))
    }

    /// Import a folder from this PC as a new Local Project (the original is left untouched).
    pub fn import_folder(&self, src: &Path, name: Option<&str>) -> Result<ImportReport> {
        if !src.is_dir() {
            return Err(Error::io(
                format!("{} is not a folder", src.display()),
                std::io::Error::from(std::io::ErrorKind::NotFound),
            ));
        }
        let display_name = name
            .map(str::to_string)
            .or_else(|| src.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "Imported project".to_string());
        self.import_with(&display_name, |sink| {
            for entry in WalkDir::new(src).min_depth(1).into_iter().flatten() {
                let Ok(rel) = entry.path().strip_prefix(src) else {
                    continue;
                };
                let rel = rel.to_string_lossy().replace('\\', "/");
                if entry.file_type().is_dir() {
                    sink.add_dir(&rel);
                } else if entry.file_type().is_file() {
                    let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    sink.check_size(&rel, len)?;
                    let bytes = fs::read(entry.path())
                        .map_err(|e| Error::io(format!("reading {}", entry.path().display()), e))?;
                    sink.add_file(&rel, &bytes)?;
                }
            }
            Ok(())
        })
    }

    /// Pull: turn a zip downloaded from Overleaf into a new Overleaf-tagged Project. The zip is a
    /// temporary file that is deleted as soon as it has been applied (or has failed to apply).
    pub fn create_from_overleaf_zip(
        &self,
        zip_bytes: &[u8],
        link: OverleafLink,
    ) -> Result<ImportReport> {
        let tmp_dir = self.root.join(META_DIR).join("tmp");
        fs::create_dir_all(&tmp_dir)
            .map_err(|e| Error::io(format!("creating {}", tmp_dir.display()), e))?;
        let tmp_zip = tmp_dir.join(format!("pull-{}.zip", uuid::Uuid::new_v4()));
        fs::write(&tmp_zip, zip_bytes).map_err(|e| Error::io("saving the downloaded zip", e))?;

        let imported = self.import_zip(&tmp_zip, Some(&link.name));
        let _ = fs::remove_file(&tmp_zip); // always delete the pulled zip

        let mut report = imported?;
        let project = self.open_project(&report.project.dir_name)?;
        project.link_overleaf(&OverleafLink {
            last_synced: Some(crate::project::now_unix()),
            ..link
        })?;
        project.save_base_snapshot()?;
        report.project = summarize(&report.project.dir_name, &project);
        Ok(report)
    }

    /// Import a zip (for example one downloaded from Overleaf) as a new Local Project.
    pub fn import_zip(&self, zip_path: &Path, name: Option<&str>) -> Result<ImportReport> {
        let file = fs::File::open(zip_path)
            .map_err(|e| Error::io(format!("opening {}", zip_path.display()), e))?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| Error::Zip(e.to_string()))?;
        let display_name = name
            .map(str::to_string)
            .or_else(|| zip_path.file_stem().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "Imported project".to_string());

        // Zips made by "compress folder" wrap everything in one top-level folder; strip it.
        let strip = common_wrapper(&mut archive);

        self.import_with(&display_name, |sink| {
            for i in 0..archive.len() {
                let mut entry = archive.by_index(i).map_err(|e| Error::Zip(e.to_string()))?;
                let raw_name = entry
                    .name()
                    .map(|n| n.into_owned())
                    .unwrap_or_else(|_| "<unreadable name>".to_string());
                let Some(mut rel) = clean_zip_name(&raw_name) else {
                    sink.skip(&raw_name, "path escapes the project folder");
                    continue;
                };
                if let Some(prefix) = &strip {
                    if rel.trim_end_matches('/') == prefix.trim_end_matches('/') {
                        continue; // the wrapper folder entry itself
                    }
                    rel = rel.strip_prefix(prefix.as_str()).unwrap_or(&rel).to_string();
                }
                let rel = rel.trim_matches('/').to_string();
                if rel.is_empty() {
                    continue;
                }
                if entry.is_dir() {
                    sink.add_dir(&rel);
                    continue;
                }
                sink.check_size(&rel, entry.size())?;
                let mut bytes = Vec::with_capacity(entry.size() as usize);
                entry
                    .read_to_end(&mut bytes)
                    .map_err(|e| Error::io(format!("extracting {rel}"), e))?;
                sink.add_file(&rel, &bytes)?;
            }
            Ok(())
        })
    }

    fn import_with(
        &self,
        display_name: &str,
        fill: impl FnOnce(&mut Sink) -> Result<()>,
    ) -> Result<ImportReport> {
        let dir = self.unique_dir(display_name);
        fs::create_dir_all(&dir).map_err(|e| Error::io(format!("creating {}", dir.display()), e))?;
        let mut sink = Sink::new(&dir);
        if let Err(e) = fill(&mut sink) {
            let _ = fs::remove_dir_all(&dir); // never leave a half-imported Project behind
            return Err(e);
        }
        let mut warnings = Vec::new();
        let root = detect_root_file(&dir).unwrap_or_else(|| {
            warnings.push(
                "No .tex file with \\documentclass was found; set the Root file in project settings."
                    .to_string(),
            );
            "main.tex".to_string()
        });
        let project = Project::init(&dir, display_name, &root)?;
        Ok(ImportReport {
            project: summarize(&dir_name_of(&dir), &project),
            files_imported: sink.imported,
            ignored: sink.ignored,
            skipped: sink.skipped,
            warnings,
        })
    }
}

/// Collects incoming files, applying the Ignore list, Windows-name rules and case checks.
struct Sink {
    dir: PathBuf,
    imported: usize,
    ignored: usize,
    skipped: Vec<Skipped>,
    /// lowercase path -> original path, to catch names that differ only by case
    seen: HashMap<String, String>,
    total_bytes: u64,
}

impl Sink {
    fn new(dir: &Path) -> Sink {
        Sink {
            dir: dir.to_path_buf(),
            imported: 0,
            ignored: 0,
            skipped: Vec::new(),
            seen: HashMap::new(),
            total_bytes: 0,
        }
    }

    fn skip(&mut self, path: &str, reason: &str) {
        self.skipped.push(Skipped {
            path: path.to_string(),
            reason: reason.to_string(),
        });
    }

    fn check_size(&mut self, rel: &str, len: u64) -> Result<()> {
        if is_ignored(rel) {
            return Ok(());
        }
        if len > MAX_FILE_BYTES {
            return Err(Error::Zip(format!("{rel} is larger than 512 MB")));
        }
        self.total_bytes += len;
        if self.total_bytes > MAX_TOTAL_BYTES {
            return Err(Error::Zip("project is larger than 4 GB".to_string()));
        }
        Ok(())
    }

    fn accepts(&mut self, rel: &str) -> bool {
        if is_ignored(rel) {
            self.ignored += 1;
            return false;
        }
        if let Some((component, problem)) = check_path(rel) {
            self.skip(rel, &format!("`{component}` {problem}"));
            return false;
        }
        match self.seen.get(&rel.to_lowercase()) {
            Some(first) => {
                let reason = format!("differs only by case from `{first}`");
                self.skip(rel, &reason);
                false
            }
            None => {
                self.seen.insert(rel.to_lowercase(), rel.to_string());
                true
            }
        }
    }

    fn add_dir(&mut self, rel: &str) {
        if self.accepts(rel) {
            let _ = fs::create_dir_all(self.dir.join(rel)); // keeps empty folders
        }
    }

    fn add_file(&mut self, rel: &str, bytes: &[u8]) -> Result<()> {
        if self.accepts(rel) {
            fsutil::write_bytes(&self.dir, rel, bytes)?;
            self.imported += 1;
        }
        Ok(())
    }
}

/// If every real entry sits under one shared top-level folder, return `"<folder>/"`.
pub(crate) fn common_wrapper<R: Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>) -> Option<String> {
    let mut first: Option<String> = None;
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else {
            continue;
        };
        // Entries that escape the folder are reported during extraction; they must not stop
        // the wrapper from being recognised.
        let raw = entry.name().map(|n| n.into_owned()).unwrap_or_default();
        let Some(name) = clean_zip_name(&raw) else {
            continue;
        };
        let name = name.trim_end_matches('/').to_string();
        if name.starts_with("__MACOSX/") {
            continue;
        }
        let (top, rest) = match name.split_once('/') {
            Some((t, r)) => (t.to_string(), r.to_string()),
            None if entry.is_dir() => (name.clone(), String::new()), // the wrapper folder itself
            None => return None, // a file at the top level: no wrapper
        };
        if rest.is_empty() && !entry.is_dir() {
            return None;
        }
        match &first {
            None => first = Some(top),
            Some(f) if *f == top => {}
            Some(_) => return None,
        }
    }
    first.map(|f| format!("{f}/"))
}

/// Normalise an archive entry name, or return `None` if it could leave the Project folder.
/// We do not trust the zip library's own sanitising: it silently rewrites some names (and
/// accepts `a/../b`), which would hide the real reason an entry was skipped.
pub(crate) fn clean_zip_name(raw: &str) -> Option<String> {
    let name = raw.replace('\\', "/");
    let has_drive = name.as_bytes().get(1) == Some(&b':');
    if name.starts_with('/') || has_drive || name.contains('\0') || name.split('/').any(|c| c == "..") {
        return None;
    }
    Some(name.trim_start_matches("./").to_string())
}

fn dir_name_of(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn summarize(dir_name: &str, project: &Project) -> ProjectSummary {
    ProjectSummary {
        dir_name: dir_name.to_string(),
        id: project.meta.id.clone(),
        name: project.meta.name.clone(),
        root_file: project.meta.root_file.clone(),
        origin: project.origin(),
        overleaf: project.overleaf_link(),
        archived: project.meta.archived,
        last_edited_ms: last_edited_ms(project.dir()),
    }
}

/// Newest modification time among the Project's source files (not `.moss`, not build output).
fn last_edited_ms(dir: &Path) -> Option<u64> {
    WalkDir::new(dir)
        .max_depth(6)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || e.file_name() != META_DIR)
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .strip_prefix(dir)
                .ok()
                .map(|r| !is_ignored(&r.to_string_lossy().replace('\\', "/")))
                .unwrap_or(false)
        })
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .max()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            if name.ends_with('/') {
                zip.add_directory(*name, opts).unwrap();
            } else {
                zip.start_file(*name, opts).unwrap();
                zip.write_all(data).unwrap();
            }
        }
        zip.finish().unwrap();
    }

    #[test]
    fn create_and_list_projects_with_local_origin() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = Workspace::open(tmp.path()).unwrap();
        let a = ws.create_project("Thesis").unwrap();
        let b = ws.create_project("Thesis").unwrap(); // same name: second gets a suffix
        assert_eq!(a.dir_name, "Thesis");
        assert_eq!(b.dir_name, "Thesis (2)");
        let listed = ws.list_projects().unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().all(|p| p.origin == Origin::Local && p.root_file == "main.tex"));
    }

    #[test]
    fn import_folder_applies_ignore_list_names_and_lf() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("report");
        fs::create_dir_all(src.join("chapters")).unwrap();
        fs::write(src.join("0Synopsis.tex"), "\\documentclass{report}\r\n\\begin{document}\r\n\\end{document}\r\n").unwrap();
        fs::write(src.join("chapters/one.tex"), "x").unwrap();
        fs::write(src.join("0Synopsis.aux"), "junk").unwrap();
        fs::write(src.join("0Synopsis.lof"), "junk").unwrap();
        fs::write(src.join("fig.png"), [0x89, b'P', b'N', b'G', b'\r', b'\n']).unwrap();
        fs::create_dir_all(src.join("empty")).unwrap();

        let ws = Workspace::open(tmp.path().join("ws")).unwrap();
        let report = ws.import_folder(&src, None).unwrap();

        assert_eq!(report.project.name, "report");
        assert_eq!(report.project.root_file, "0Synopsis.tex");
        assert_eq!(report.project.origin, Origin::Local);
        assert_eq!(report.files_imported, 3);
        assert_eq!(report.ignored, 2);
        assert!(report.skipped.is_empty());
        let dir = ws.projects_dir().join(&report.project.dir_name);
        assert_eq!(fs::read_to_string(dir.join("0Synopsis.tex")).unwrap().matches('\r').count(), 0);
        assert_eq!(fs::read(dir.join("fig.png")).unwrap(), [0x89, b'P', b'N', b'G', b'\r', b'\n']);
        assert!(dir.join("empty").is_dir());
        assert!(!dir.join("0Synopsis.aux").exists());
        assert!(src.join("0Synopsis.aux").exists(), "original untouched");
    }

    #[test]
    fn import_zip_strips_wrapper_blocks_zip_slip_and_reports_bad_names() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("Main_Project_Report.zip");
        make_zip(
            &zip_path,
            &[
                ("Main_Project_Report/", b""),
                ("Main_Project_Report/main.tex", b"\\documentclass{article}"),
                ("Main_Project_Report/chapters/aux.tex", b"x"),
                ("Main_Project_Report/Fig.png", b"1"),
                ("Main_Project_Report/fig.png", b"2"),
                ("Main_Project_Report/../evil.tex", b"x"),
                ("__MACOSX/Main_Project_Report/._main.tex", b"x"),
            ],
        );
        let ws = Workspace::open(tmp.path().join("ws")).unwrap();
        let report = ws.import_zip(&zip_path, None).unwrap();

        assert_eq!(report.project.name, "Main_Project_Report");
        assert_eq!(report.project.root_file, "main.tex");
        let dir = ws.projects_dir().join(&report.project.dir_name);
        assert!(dir.join("main.tex").is_file(), "wrapper folder stripped");
        assert!(!tmp.path().join("evil.tex").exists());
        let reasons: Vec<&str> = report.skipped.iter().map(|s| s.reason.as_str()).collect();
        assert!(reasons.iter().any(|r| r.contains("reserved")), "{reasons:?}");
        assert!(reasons.iter().any(|r| r.contains("only by case")), "{reasons:?}");
        assert!(reasons.iter().any(|r| r.contains("escapes")), "{reasons:?}");
        assert!(!dir.join("chapters/aux.tex").exists());
    }

    #[test]
    fn zip_without_a_wrapper_imports_at_the_root() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("p.zip");
        make_zip(&zip_path, &[("main.tex", b"\\documentclass{article}"), ("refs.bib", b"@book{}")]);
        let ws = Workspace::open(tmp.path().join("ws")).unwrap();
        let report = ws.import_zip(&zip_path, Some("My paper")).unwrap();
        let dir = ws.projects_dir().join(&report.project.dir_name);
        assert!(dir.join("main.tex").is_file() && dir.join("refs.bib").is_file());
        assert_eq!(report.project.name, "My paper");
    }

    #[test]
    fn pulling_makes_an_overleaf_tagged_project_with_a_base_snapshot_and_no_leftover_zip() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("remote.zip");
        make_zip(
            &zip_path,
            &[
                ("main.tex", b"\\documentclass{article}\r\n"),
                ("chapters/one.tex", b"x"),
                ("main.aux", b"junk"),
            ],
        );
        let bytes = fs::read(&zip_path).unwrap();
        let ws = Workspace::open(tmp.path().join("ws")).unwrap();
        let link = OverleafLink {
            project_id: "abc123".into(),
            name: "Thesis".into(),
            last_synced: None,
        };
        let report = ws.create_from_overleaf_zip(&bytes, link).unwrap();

        assert_eq!(report.project.origin, Origin::Overleaf);
        assert_eq!(report.project.overleaf.as_ref().unwrap().project_id, "abc123");
        assert!(report.project.overleaf.as_ref().unwrap().last_synced.is_some());
        let dir = ws.projects_dir().join(&report.project.dir_name);
        assert!(dir.join(".moss/base/main.tex").is_file());
        assert!(dir.join(".moss/base/chapters/one.tex").is_file());
        assert!(!dir.join(".moss/base/main.aux").exists(), "ignored files stay out");
        let leftovers = fs::read_dir(ws.root().join(".moss/tmp")).unwrap().count();
        assert_eq!(leftovers, 0, "pulled zip must be deleted");
    }

    #[test]
    fn archive_hides_nothing_on_disk_and_delete_removes_the_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = Workspace::open(tmp.path()).unwrap();
        let p = ws.create_project("Thesis").unwrap();
        assert!(!p.archived);
        assert!(ws.set_archived(&p.dir_name, true).unwrap().archived);
        assert!(ws.list_projects().unwrap()[0].archived, "persisted in project.json");
        assert!(!ws.set_archived(&p.dir_name, false).unwrap().archived);

        let other = ws.create_project("Notes").unwrap();
        let renamed = ws.rename_project(&p.dir_name, "  My thesis ").unwrap();
        assert_eq!((renamed.name.as_str(), renamed.dir_name.as_str()), ("My thesis", "Thesis"));
        assert!(ws.rename_project(&p.dir_name, "   ").is_err());
        assert!(ws.rename_project(&p.dir_name, "NOTES").is_err(), "names stay unique");
        assert!(ws.rename_project(&other.dir_name, "Notes").is_ok(), "keeping its own name is fine");
        ws.delete_project(&other.dir_name).unwrap();

        ws.delete_project(&p.dir_name).unwrap();
        assert!(ws.list_projects().unwrap().is_empty());
        assert!(ws.delete_project(&p.dir_name).is_err());
        assert!(ws.delete_project("../x").is_err());
    }

    #[test]
    fn open_project_rejects_path_tricks() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = Workspace::open(tmp.path()).unwrap();
        assert!(ws.open_project("../x").is_err());
        assert!(matches!(ws.open_project("nope"), Err(Error::ProjectNotFound(_))));
    }
}
