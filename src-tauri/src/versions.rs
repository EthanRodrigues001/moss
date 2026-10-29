//! The History tab and Sync, as Tauri commands: versions, diffs, restore, Pull/Push/Sync with a
//! Review, and linking a Project to Overleaf or to a Practice project.

use moss_core::history::{FileChange, FileView, History, Rev, SnapshotInfo};
use moss_core::overleaf::Session;
use moss_core::project::{OverleafLink, Project};
use moss_core::sync::link::{self, is_practice, practice_dir};
use moss_core::sync::{
    Applied, ApplyRequest, LocalChange, Mode, Preview, Staged, Syncer, local_changes,
};
use moss_core::workspace::ImportReport;
use serde::Serialize;
use tauri::State;

use crate::overleaf::current_session;
use crate::{AppState, CmdResult, blocking, err};

/// What a Version (or the Working copy) changed, and what to compare it with.
#[derive(Serialize)]
pub(crate) struct VersionDetail {
    from: Rev,
    to: Rev,
    files: Vec<FileChange>,
}

#[derive(Serialize)]
pub(crate) struct SyncStatus {
    link: Option<OverleafLink>,
    practice: bool,
    /// Where the Practice project's stand-in Overleaf lives, to open in Explorer.
    practice_folder: Option<String>,
    changes: Vec<LocalChange>,
}

fn open(state: &AppState, project: &str) -> CmdResult<Project> {
    state.workspace.open_project(project).map_err(err)
}

// ----- History -----

#[tauri::command]
pub(crate) async fn history_save(
    state: State<'_, AppState>,
    project: String,
    message: String,
) -> CmdResult<Option<SnapshotInfo>> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        History::new(p.dir()).save_version(&message).map_err(err)
    })
    .await
}

#[tauri::command]
pub(crate) async fn history_version(
    state: State<'_, AppState>,
    project: String,
    id: u64,
) -> CmdResult<VersionDetail> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        let h = History::new(p.dir());
        Ok(VersionDetail {
            from: h.parent_of(id).map_err(err)?,
            to: Rev::Snapshot { id },
            files: h.compare_with_parent(id).map_err(err)?,
        })
    })
    .await
}

/// The Working copy row: what changed since the latest Version.
#[tauri::command]
pub(crate) async fn history_working(
    state: State<'_, AppState>,
    project: String,
) -> CmdResult<VersionDetail> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        let h = History::new(p.dir());
        let from = h.cursor_rev().map_err(err)?;
        Ok(VersionDetail {
            from,
            to: Rev::Working,
            files: h.compare(from, Rev::Working).map_err(err)?,
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn history_file(
    state: State<'_, AppState>,
    project: String,
    rev: Rev,
    path: String,
) -> CmdResult<FileView> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        History::new(p.dir()).file_at(rev, &path).map_err(err)
    })
    .await
}

#[tauri::command]
pub(crate) async fn history_restore(
    state: State<'_, AppState>,
    project: String,
    id: u64,
) -> CmdResult<SnapshotInfo> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        History::new(p.dir()).restore(id).map_err(err)
    })
    .await
}

#[tauri::command]
pub(crate) async fn history_restore_file(
    state: State<'_, AppState>,
    project: String,
    id: u64,
    path: String,
) -> CmdResult<SnapshotInfo> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        History::new(p.dir()).restore_file(id, &path).map_err(err)
    })
    .await
}

// ----- Sync -----

#[tauri::command]
pub(crate) async fn sync_status(state: State<'_, AppState>, project: String) -> CmdResult<SyncStatus> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        let link = p.overleaf_link();
        let practice = link.as_ref().is_some_and(is_practice);
        let practice_folder = link
            .as_ref()
            .and_then(|l| practice_dir(ws.root(), l))
            .map(|d| d.to_string_lossy().into_owned());
        // Without a link there is no Base snapshot to compare with.
        let changes = if link.is_some() { local_changes(&p).map_err(err)? } else { Vec::new() };
        Ok(SyncStatus { link, practice, practice_folder, changes })
    })
    .await
}

/// A session only matters for a real Overleaf link; a Practice project needs none.
fn session_for(state: &AppState, link: &OverleafLink) -> CmdResult<Option<Session>> {
    if is_practice(link) {
        Ok(None)
    } else {
        current_session(state).map(Some)
    }
}

#[tauri::command]
pub(crate) async fn sync_preview(
    state: State<'_, AppState>,
    project: String,
    mode: Mode,
) -> CmdResult<Preview> {
    let ws = state.workspace.clone();
    let p = open(&state, &project)?;
    let link = p.overleaf_link().ok_or("This Project is not linked to Overleaf.")?;
    let session = session_for(&state, &link)?;
    let (preview, staged) = blocking(move || {
        let remote = link::open_remote(ws.root(), &link, session.as_ref()).map_err(err)?;
        Syncer::new(&p, remote.as_ref()).preview(mode).map_err(err)
    })
    .await?;
    state.staged.lock().unwrap().insert(project, staged);
    Ok(preview)
}

/// Both sides and the Base of one file in the open Review, for the diff.
#[tauri::command]
pub(crate) fn sync_file(
    state: State<'_, AppState>,
    project: String,
    path: String,
) -> CmdResult<moss_core::sync::StagedFile> {
    let staged = state.staged.lock().unwrap();
    let staged: &Staged = staged
        .get(&project)
        .ok_or("There is no open Review; open it again.")?;
    Ok(staged.views(&path))
}

#[tauri::command]
pub(crate) async fn sync_apply(
    state: State<'_, AppState>,
    project: String,
    request: ApplyRequest,
) -> CmdResult<Applied> {
    let ws = state.workspace.clone();
    let p = open(&state, &project)?;
    let link = p.overleaf_link().ok_or("This Project is not linked to Overleaf.")?;
    let session = session_for(&state, &link)?;
    let applied = blocking(move || {
        let remote = link::open_remote(ws.root(), &link, session.as_ref()).map_err(err)?;
        Syncer::new(&p, remote.as_ref()).apply(&request).map_err(err)
    })
    .await?;
    if matches!(applied, Applied::Done { .. }) {
        state.staged.lock().unwrap().remove(&project);
    }
    Ok(applied)
}

/// Overleaf's own change history (who edited what), read-only. A Practice project has none.
#[tauri::command]
pub(crate) async fn overleaf_history(
    state: State<'_, AppState>,
    project: String,
    before: Option<i64>,
) -> CmdResult<moss_core::overleaf::RemoteHistory> {
    let p = open(&state, &project)?;
    let link = p.overleaf_link().ok_or("This Project is not linked to Overleaf.")?;
    if is_practice(&link) {
        return Err("A practice project has no Overleaf history.".into());
    }
    let session = current_session(&state)?;
    blocking(move || {
        moss_core::overleaf::Client::new(&session)
            .and_then(|c| c.history(&link.project_id, before))
            .map_err(err)
    })
    .await
}

#[tauri::command]
pub(crate) async fn sync_link_practice(
    state: State<'_, AppState>,
    project: String,
) -> CmdResult<OverleafLink> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        link::link_practice_copy(ws.root(), &p).map_err(err)
    })
    .await
}

/// Create a new Overleaf project from this Project's files and link to it.
#[tauri::command]
pub(crate) async fn sync_link_new(
    state: State<'_, AppState>,
    project: String,
) -> CmdResult<OverleafLink> {
    let ws = state.workspace.clone();
    let session = current_session(&state)?;
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        link::link_new_overleaf(&p, &session).map_err(err)
    })
    .await
}

#[tauri::command]
pub(crate) async fn sync_link_existing(
    state: State<'_, AppState>,
    project: String,
    overleaf_id: String,
    name: String,
) -> CmdResult<OverleafLink> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        link::link_existing(&p, &overleaf_id, &name).map_err(err)
    })
    .await
}

#[tauri::command]
pub(crate) async fn sync_unlink(state: State<'_, AppState>, project: String) -> CmdResult<()> {
    let ws = state.workspace.clone();
    let key = project.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        link::unlink(&p).map_err(err)
    })
    .await?;
    state.staged.lock().unwrap().remove(&key);
    Ok(())
}

/// A Practice project: sample files in a folder that plays Overleaf, pulled as a new Project.
#[tauri::command]
pub(crate) async fn practice_create(
    state: State<'_, AppState>,
    name: String,
) -> CmdResult<ImportReport> {
    let ws = state.workspace.clone();
    blocking(move || link::create_practice_sample(&ws, &name).map_err(err)).await
}
