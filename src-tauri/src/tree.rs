//! File tree actions: new file, new folder, rename, delete, and adding files from this PC.
//! Each one keeps the user's typing as a Version first, runs, then records what it did as a Version,
//! so every action shows in the History tab and can be undone.

use std::path::PathBuf;

use moss_core::entries::{self, AddReport};
use moss_core::history::{AUTHOR_YOU, History, VersionKind};
use moss_core::project::Project;
use tauri::State;

use crate::{AppState, CmdResult, blocking, err};

/// Keep anything typed since the last Version, so undoing the action does not lose it.
fn before(p: &Project) -> CmdResult<()> {
    History::new(p.dir()).capture("Your changes").map(|_| ()).map_err(err)
}

/// Record what the action did as one Version.
fn after(p: &Project, label: String) -> CmdResult<()> {
    History::new(p.dir())
        .capture_as(VersionKind::Auto, AUTHOR_YOU, &label)
        .map(|_| ())
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn tree_create(
    state: State<'_, AppState>,
    project: String,
    path: String,
    is_dir: bool,
) -> CmdResult<()> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        before(&p)?;
        if is_dir {
            entries::create_folder(p.dir(), &path).map_err(err)?;
        } else {
            entries::create_file(p.dir(), &path).map_err(err)?;
        }
        after(&p, format!("Created {path}"))
    })
    .await
}

#[tauri::command]
pub(crate) async fn tree_rename(
    state: State<'_, AppState>,
    project: String,
    from: String,
    to: String,
) -> CmdResult<()> {
    let ws = state.workspace.clone();
    blocking(move || {
        let mut p = ws.open_project(&project).map_err(err)?;
        before(&p)?;
        entries::rename(p.dir(), &from, &to).map_err(err)?;
        // Keep the Root file pointing at the same file when it, or its folder, moved.
        let root = p.meta.root_file.clone();
        if root == from {
            p.meta.root_file = to.clone();
            p.save_meta().map_err(err)?;
        } else if let Some(rest) = root.strip_prefix(&format!("{from}/")) {
            p.meta.root_file = format!("{to}/{rest}");
            p.save_meta().map_err(err)?;
        }
        after(&p, format!("Renamed {from} to {to}"))
    })
    .await
}

#[tauri::command]
pub(crate) async fn tree_delete(
    state: State<'_, AppState>,
    project: String,
    path: String,
) -> CmdResult<()> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        before(&p)?;
        entries::delete(p.dir(), &path).map_err(err)?;
        after(&p, format!("Deleted {path}"))
    })
    .await
}

#[tauri::command]
pub(crate) async fn tree_add_files(
    state: State<'_, AppState>,
    project: String,
    folder: String,
    sources: Vec<String>,
) -> CmdResult<AddReport> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        before(&p)?;
        let sources: Vec<PathBuf> = sources.into_iter().map(PathBuf::from).collect();
        let report = entries::add_files(p.dir(), &folder, &sources).map_err(err)?;
        if !report.added.is_empty() {
            let n = report.added.len();
            after(&p, format!("Added {n} file{} from this PC", if n == 1 { "" } else { "s" }))?;
        }
        Ok(report)
    })
    .await
}
