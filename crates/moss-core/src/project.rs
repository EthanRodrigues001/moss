//! A Project: one folder holding a LaTeX document, plus its `.moss/` metadata.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::ignore::is_ignored;
use crate::tex::Engine;
use crate::{Error, Result};

pub const META_DIR: &str = ".moss";
const PROJECT_FILE: &str = "project.json";
const SYNC_FILE: &str = "sync.json";
const SCHEMA: u32 = 1;

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub root_file: String,
    /// `None` means pick automatically from the root file each compile.
    #[serde(default)]
    pub engine: Option<Engine>,
    #[serde(default)]
    pub shell_escape: bool,
    /// Trusted project: Agents skip Approvals.
    #[serde(default)]
    pub trusted: bool,
    /// Archived Projects sit in the sidebar's Archive list; nothing about the files changes.
    #[serde(default)]
    pub archived: bool,
    pub created_at: i64,
}

/// Present only while the Project is linked to an Overleaf project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverleafLink {
    pub project_id: String,
    pub name: String,
    #[serde(default)]
    pub last_synced: Option<i64>,
}

/// The tag shown on a Project card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Local,
    Overleaf,
}

#[derive(Debug, Clone)]
pub struct Project {
    dir: PathBuf,
    pub meta: ProjectMeta,
}

impl Project {
    /// Create `.moss/project.json` for a folder that already holds the sources.
    pub fn init(dir: &Path, name: &str, root_file: &str) -> Result<Project> {
        let meta = ProjectMeta {
            schema: SCHEMA,
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            root_file: root_file.to_string(),
            engine: None,
            shell_escape: false,
            trusted: false,
            archived: false,
            created_at: now_unix(),
        };
        let project = Project {
            dir: dir.to_path_buf(),
            meta,
        };
        project.save_meta()?;
        Ok(project)
    }

    pub fn open(dir: &Path) -> Result<Project> {
        let file = dir.join(META_DIR).join(PROJECT_FILE);
        let text = fs::read_to_string(&file).map_err(|_| Error::NotAProject(dir.to_path_buf()))?;
        let meta: ProjectMeta = serde_json::from_str(&text)
            .map_err(|e| Error::json(format!("reading {}", file.display()), e))?;
        Ok(Project {
            dir: dir.to_path_buf(),
            meta,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn save_meta(&self) -> Result<()> {
        let meta_dir = self.dir.join(META_DIR);
        fs::create_dir_all(&meta_dir)
            .map_err(|e| Error::io(format!("creating {}", meta_dir.display()), e))?;
        let json = serde_json::to_string_pretty(&self.meta)
            .map_err(|e| Error::json("serializing project.json", e))?;
        fs::write(meta_dir.join(PROJECT_FILE), json)
            .map_err(|e| Error::io("writing project.json", e))
    }

    pub fn overleaf_link(&self) -> Option<OverleafLink> {
        let text = fs::read_to_string(self.dir.join(META_DIR).join(SYNC_FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn link_overleaf(&self, link: &OverleafLink) -> Result<()> {
        let json = serde_json::to_string_pretty(link)
            .map_err(|e| Error::json("serializing sync.json", e))?;
        fs::write(self.dir.join(META_DIR).join(SYNC_FILE), json)
            .map_err(|e| Error::io("writing sync.json", e))
    }

    pub fn origin(&self) -> Origin {
        if self.overleaf_link().is_some() {
            Origin::Overleaf
        } else {
            Origin::Local
        }
    }

    /// Replace the Base snapshot (`.moss/base/`) with the Project's current sources. Call it right
    /// after a successful Pull or Push: it is the common ancestor for the next three-way merge.
    pub fn save_base_snapshot(&self) -> Result<()> {
        let base = self.dir.join(META_DIR).join("base");
        if base.exists() {
            fs::remove_dir_all(&base)
                .map_err(|e| Error::io(format!("clearing {}", base.display()), e))?;
        }
        fs::create_dir_all(&base).map_err(|e| Error::io(format!("creating {}", base.display()), e))?;
        for entry in crate::fsutil::list_files(&self.dir)? {
            let to = base.join(&entry.path);
            if entry.is_dir {
                fs::create_dir_all(&to).map_err(|e| Error::io(format!("creating {}", to.display()), e))?;
            } else {
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| Error::io(format!("creating {}", parent.display()), e))?;
                }
                fs::copy(self.dir.join(&entry.path), &to)
                    .map_err(|e| Error::io(format!("snapshotting {}", entry.path), e))?;
            }
        }
        Ok(())
    }

    /// Where compile output for `root_file` lives: `<project>/.moss/build/<root stem>`.
    pub fn build_dir(&self, root_file: &str) -> PathBuf {
        let stem = Path::new(root_file)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "main".to_string());
        self.dir.join(META_DIR).join("build").join(stem)
    }
}

/// Find the file to compile: any `.tex` with a real `\documentclass`. `main.tex` wins, then the
/// shallowest path, then alphabetical order.
pub fn detect_root_file(project_dir: &Path) -> Option<String> {
    root_file_candidates(project_dir).into_iter().next()
}

/// Every `.tex` file that could be the Root file, best guess first.
pub fn root_file_candidates(project_dir: &Path) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();
    for entry in WalkDir::new(project_dir).min_depth(1).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(project_dir) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if is_ignored(&rel) || !rel.to_ascii_lowercase().ends_with(".tex") {
            continue;
        }
        let Ok(bytes) = fs::read(entry.path()) else {
            continue;
        };
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(64 * 1024)]).into_owned();
        if has_documentclass(&head) {
            candidates.push(rel);
        }
    }
    candidates.sort_by_key(|p| {
        (
            p != "main.tex",
            p.matches('/').count(),
            p.to_ascii_lowercase(),
        )
    });
    candidates
}

fn has_documentclass(source: &str) -> bool {
    source.lines().any(|line| {
        let code = line.split('%').next().unwrap_or("");
        code.contains("\\documentclass")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_round_trips_and_origin_follows_the_link() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Project::init(tmp.path(), "Report", "main.tex").unwrap();
        assert_eq!(p.origin(), Origin::Local);
        p.link_overleaf(&OverleafLink {
            project_id: "abc".into(),
            name: "Report".into(),
            last_synced: None,
        })
        .unwrap();
        let reopened = Project::open(tmp.path()).unwrap();
        assert_eq!(reopened.meta.name, "Report");
        assert_eq!(reopened.origin(), Origin::Overleaf);
    }

    #[test]
    fn opening_a_plain_folder_is_not_a_project() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(Project::open(tmp.path()), Err(Error::NotAProject(_))));
    }

    #[test]
    fn root_detection_prefers_main_then_shallow_and_ignores_comments() {
        let tmp = tempfile::tempdir().unwrap();
        let w = |p: &str, s: &str| {
            let path = tmp.path().join(p);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, s).unwrap();
        };
        w("chapters/intro.tex", "\\chapter{Intro}");
        w("notes.tex", "% \\documentclass{article}\ntext");
        w("sub/talk.tex", "\\documentclass{beamer}");
        w("zeta.tex", "\\documentclass{report}");
        assert_eq!(detect_root_file(tmp.path()).as_deref(), Some("zeta.tex"));
        w("main.tex", "\\documentclass{article}");
        assert_eq!(detect_root_file(tmp.path()).as_deref(), Some("main.tex"));
    }

    #[test]
    fn build_dir_is_per_root_file() {
        let p = Project {
            dir: PathBuf::from("C:/w/proj"),
            meta: ProjectMeta {
                schema: 1,
                id: "x".into(),
                name: "x".into(),
                root_file: "0Synopsis.tex".into(),
                engine: None,
                shell_escape: false,
                trusted: false,
                archived: false,
                created_at: 0,
            },
        };
        assert!(p.build_dir("0Synopsis.tex").ends_with(".moss/build/0Synopsis"));
    }
}
