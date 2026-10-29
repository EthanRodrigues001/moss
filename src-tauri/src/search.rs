//! Project-wide search and replace, as Tauri commands over `moss_core::search`.

use moss_core::search::{self, ReplaceReport, SearchOptions, SearchReport};
use tauri::State;

use crate::{AppState, CmdResult, blocking, err};

#[tauri::command]
pub(crate) async fn search_project(
    state: State<'_, AppState>,
    project: String,
    options: SearchOptions,
) -> CmdResult<SearchReport> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        search::search(p.dir(), &options).map_err(err)
    })
    .await
}

/// Replace in every matching file, or only in `paths`. Takes one restore point first.
#[tauri::command]
pub(crate) async fn replace_in_project(
    state: State<'_, AppState>,
    project: String,
    options: SearchOptions,
    replacement: String,
    paths: Option<Vec<String>>,
) -> CmdResult<ReplaceReport> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        search::replace(p.dir(), &options, &replacement, paths.as_deref()).map_err(err)
    })
    .await
}
