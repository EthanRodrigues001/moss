//! Connecting a Project to an Overleaf project, or to a Practice project, and choosing the
//! `Remote` a link points at.

use std::fs;
use std::path::{Path, PathBuf};

use super::fileset::FileSet;
use super::remote::{FolderRemote, Remote};
use crate::overleaf::{Client, OverleafRemote, Session};
use crate::project::{META_DIR, OverleafLink, Project, now_unix};
use crate::workspace::{ImportReport, Workspace};
use crate::{Error, Result};

/// A link whose project id starts with this is a Practice project: a folder, not Overleaf.
pub const PRACTICE_PREFIX: &str = "practice:";

pub fn is_practice(link: &OverleafLink) -> bool {
    link.project_id.starts_with(PRACTICE_PREFIX)
}

/// `<workspace>/.moss/practice/<name>/`: the folder that plays Overleaf for one Practice project.
pub fn practice_dir(workspace_root: &Path, link: &OverleafLink) -> Option<PathBuf> {
    let name = link.project_id.strip_prefix(PRACTICE_PREFIX)?;
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return None;
    }
    Some(workspace_root.join(META_DIR).join("practice").join(name))
}

/// The other side of this Project's link. `session` is only needed for a real Overleaf link.
pub fn open_remote(
    workspace_root: &Path,
    link: &OverleafLink,
    session: Option<&Session>,
) -> Result<Box<dyn Remote>> {
    if is_practice(link) {
        let dir = practice_dir(workspace_root, link)
            .ok_or_else(|| Error::Sync("this Practice project has an invalid name".into()))?;
        return Ok(Box::new(FolderRemote::new(dir)?));
    }
    let session = session.ok_or(Error::OverleafAuth)?;
    Ok(Box::new(OverleafRemote::new(Client::new(session)?, link.project_id.clone())))
}

const SAMPLE: &[(&str, &str)] = &[
    (
        "main.tex",
        "\\documentclass{article}\n\\title{Practice project}\n\\author{Moss}\n\\begin{document}\n\\maketitle\n\\input{chapters/intro}\n\\input{chapters/method}\n\\end{document}\n",
    ),
    (
        "chapters/intro.tex",
        "\\section{Introduction}\nThis project lives in a folder on your PC and stands in for Overleaf.\nEdit a file in that folder with any editor, then press Pull in Moss.\nEdit the same file in Moss, and Moss will merge or ask.\n",
    ),
    (
        "chapters/method.tex",
        "\\section{Method}\nFirst we do this.\nThen we do that.\nFinally we write it down.\n",
    ),
    ("refs.bib", "@book{knuth,\n  author = {Donald Knuth},\n  title = {The TeXbook},\n  year = {1984}\n}\n"),
];

/// Make a Practice project from a few sample files and Pull it as a new Overleaf-tagged Project.
pub fn create_practice_sample(workspace: &Workspace, name: &str) -> Result<ImportReport> {
    let name = name.trim();
    let id = unique_practice_id(workspace.root(), name);
    let link = OverleafLink {
        project_id: format!("{PRACTICE_PREFIX}{id}"),
        name: name.to_string(),
        last_synced: None,
    };
    let remote = FolderRemote::new(
        practice_dir(workspace.root(), &link).ok_or_else(|| Error::Sync("invalid practice name".into()))?,
    )?;
    for (path, text) in SAMPLE {
        remote.upload(path, text.as_bytes())?;
    }
    let zip = remote.fetch()?.files.to_zip()?;
    workspace.create_from_overleaf_zip(&zip, link)
}

/// Link a Local Project to a fresh Practice copy of its files. The Base snapshot is saved, so
/// the first Sync has nothing to ask about.
pub fn link_practice_copy(workspace_root: &Path, project: &Project) -> Result<OverleafLink> {
    ensure_unlinked(project)?;
    let id = unique_practice_id(workspace_root, &project.meta.name);
    let link = OverleafLink {
        project_id: format!("{PRACTICE_PREFIX}{id}"),
        name: project.meta.name.clone(),
        last_synced: Some(now_unix()),
    };
    let remote = FolderRemote::new(
        practice_dir(workspace_root, &link).ok_or_else(|| Error::Sync("invalid practice name".into()))?,
    )?;
    for (path, blob) in &FileSet::from_project_dir(project.dir())?.files {
        remote.upload(path, &blob.bytes)?;
    }
    project.link_overleaf(&link)?;
    project.save_base_snapshot()?;
    Ok(link)
}

/// Create a new Overleaf project from the Project's files and link to it.
pub fn link_new_overleaf(project: &Project, session: &Session) -> Result<OverleafLink> {
    ensure_unlinked(project)?;
    let zip = FileSet::from_project_dir(project.dir())?.to_zip()?;
    let id = Client::new(session)?.create_project_from_zip(&project.meta.name, zip)?;
    let link = OverleafLink {
        project_id: id,
        name: project.meta.name.clone(),
        last_synced: Some(now_unix()),
    };
    project.link_overleaf(&link)?;
    project.save_base_snapshot()?;
    Ok(link)
}

/// Link to an Overleaf project that already exists. There is no Base snapshot yet, so the first
/// Sync deletes nothing and asks about every file that differs.
pub fn link_existing(project: &Project, project_id: &str, name: &str) -> Result<OverleafLink> {
    ensure_unlinked(project)?;
    let link = OverleafLink {
        project_id: project_id.to_string(),
        name: name.to_string(),
        last_synced: None,
    };
    project.link_overleaf(&link)?;
    Ok(link)
}

fn ensure_unlinked(project: &Project) -> Result<()> {
    if project.overleaf_link().is_some() {
        return Err(Error::Sync(
            "this Project is already linked to Overleaf; unlink it first".into(),
        ));
    }
    Ok(())
}

fn unique_practice_id(workspace_root: &Path, name: &str) -> String {
    let stem = crate::names::sanitize_component(name).trim_matches('.').to_string();
    let stem = if stem.is_empty() { "practice".to_string() } else { stem };
    let base = workspace_root.join(META_DIR).join("practice");
    let mut candidate = stem.clone();
    let mut n = 2;
    while base.join(&candidate).exists() {
        candidate = format!("{stem} ({n})");
        n += 1;
    }
    candidate
}

/// Remove the link and the Base snapshot, turning the Project back into a Local one. The files
/// are untouched.
pub fn unlink(project: &Project) -> Result<()> {
    let meta = project.dir().join(META_DIR);
    match fs::remove_file(meta.join("sync.json")) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::io("removing the link", e)),
    }
    let base = meta.join("base");
    if base.exists() {
        fs::remove_dir_all(&base).map_err(|e| Error::io("removing the Base snapshot", e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Origin;
    use crate::sync::{ApplyRequest, Applied, Mode, Syncer};

    fn sync_all(project: &Project, remote: &dyn Remote) -> crate::sync::Outcome {
        let s = Syncer::new(project, remote);
        let (p, _) = s.preview(Mode::Sync).unwrap();
        let req = ApplyRequest {
            mode: Mode::Sync,
            token: p.token,
            resolutions: vec![],
            allow_mass_delete: false,
            allow_markers: false,
            skip: vec![],
            actor: "You".into(),
        };
        match s.apply(&req).unwrap() {
            Applied::Done { outcome } => outcome,
            Applied::Stale { .. } => panic!("stale"),
        }
    }

    #[test]
    fn a_practice_project_is_pulled_edited_on_both_sides_and_synced() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = Workspace::open(tmp.path()).unwrap();
        let report = create_practice_sample(&ws, "Try Sync").unwrap();
        assert_eq!(report.project.origin, Origin::Overleaf);
        let project = ws.open_project(&report.project.dir_name).unwrap();
        let link = project.overleaf_link().unwrap();
        assert!(is_practice(&link));
        let remote = open_remote(ws.root(), &link, None).unwrap();

        // Nothing differs right after the Pull.
        assert!(crate::sync::local_changes(&project).unwrap().is_empty());
        let (p, _) = Syncer::new(&project, remote.as_ref()).preview(Mode::Sync).unwrap();
        assert!(p.items.is_empty(), "{:?}", p.items);

        // "Overleaf" edits one file; the user edits another.
        let dir = practice_dir(ws.root(), &link).unwrap();
        fs::write(dir.join("chapters/method.tex"), "\\section{Method}\nChanged on Overleaf.\n").unwrap();
        crate::fsutil::write_text(project.dir(), "chapters/intro.tex", "\\section{Intro}\nChanged here.\n").unwrap();

        let out = sync_all(&project, remote.as_ref());
        assert!(out.complete, "{out:?}");
        assert!(fs::read_to_string(project.dir().join("chapters/method.tex")).unwrap().contains("Overleaf"));
        assert!(fs::read_to_string(dir.join("chapters/intro.tex")).unwrap().contains("Changed here"));
    }

    #[test]
    fn linking_a_local_project_to_a_practice_copy_starts_in_sync() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = Workspace::open(tmp.path()).unwrap();
        let summary = ws.create_project("Local one").unwrap();
        let project = ws.open_project(&summary.dir_name).unwrap();
        assert_eq!(project.origin(), Origin::Local);

        let link = link_practice_copy(ws.root(), &project).unwrap();
        assert_eq!(project.origin(), Origin::Overleaf);
        assert!(link_practice_copy(ws.root(), &project).is_err(), "already linked");
        let remote = open_remote(ws.root(), &link, None).unwrap();
        let (p, _) = Syncer::new(&project, remote.as_ref()).preview(Mode::Sync).unwrap();
        assert!(p.items.is_empty() && p.conflicts.is_empty());

        unlink(&project).unwrap();
        assert_eq!(project.origin(), Origin::Local);
        assert!(project.dir().join("main.tex").exists(), "files untouched");
        assert!(!project.dir().join(".moss/base").exists());
    }

    #[test]
    fn linking_to_an_existing_remote_deletes_nothing_on_first_sync() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = Workspace::open(tmp.path()).unwrap();
        let summary = ws.create_project("Mine").unwrap();
        let project = ws.open_project(&summary.dir_name).unwrap();
        let link = link_existing(&project, "practice:other", "Other").unwrap();
        let remote = open_remote(ws.root(), &link, None).unwrap();
        remote.upload("main.tex", b"\\documentclass{article}\n% remote copy\n").unwrap();
        remote.upload("extra.tex", b"x\n").unwrap();

        let (p, _) = Syncer::new(&project, remote.as_ref()).preview(Mode::Sync).unwrap();
        assert_eq!(p.conflicts.len(), 1, "main.tex differs and nothing says which is newer");
        assert!(p.items.iter().all(|i| i.op != crate::sync::Op::Delete));
    }

    #[test]
    fn practice_names_cannot_escape_the_practice_folder() {
        let bad = OverleafLink { project_id: "practice:../evil".into(), name: "x".into(), last_synced: None };
        assert!(practice_dir(Path::new("C:/ws"), &bad).is_none());
        assert!(open_remote(Path::new("C:/ws"), &bad, None).is_err());
    }

    #[test]
    fn a_real_overleaf_link_without_a_session_is_an_auth_error() {
        let link = OverleafLink { project_id: "abc123".into(), name: "x".into(), last_synced: None };
        assert!(matches!(open_remote(Path::new("C:/ws"), &link, None), Err(Error::OverleafAuth)));
    }
}
