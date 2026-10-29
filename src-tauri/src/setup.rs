//! First-run setup commands: look at this PC, install LaTeX and Perl, run a test compile.
//! The work lives in `moss_core::setup`; this file only carries progress to the window.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use moss_core::settings::Settings;
use moss_core::setup::{self, PartId, SelfTest, SetupReport};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::UpdaterExt;

use crate::{AppState, CmdResult, TexStatus, blocking, detect_tex, err};

#[derive(Default)]
pub(crate) struct SetupState {
    cancel: Arc<AtomicBool>,
    running: std::sync::Mutex<bool>,
}

#[tauri::command]
pub(crate) async fn setup_check() -> CmdResult<SetupReport> {
    blocking(|| Ok(setup::check(Settings::load().tex_bin_dir.as_deref()))).await
}

/// Install the chosen parts (MiKTeX, then Perl), then optionally pre-install common packages.
/// Progress arrives as `setup-progress` events; the result is the fresh TeX status.
#[tauri::command]
pub(crate) async fn setup_install(
    app: AppHandle,
    state: State<'_, AppState>,
    setup_state: State<'_, SetupState>,
    parts: Vec<PartId>,
    warm: bool,
) -> CmdResult<TexStatus> {
    {
        let mut running = setup_state.running.lock().unwrap();
        if *running {
            return Err("Setup is already running".into());
        }
        *running = true;
    }
    setup_state.cancel.store(false, Ordering::Relaxed);
    let cancel = setup_state.cancel.clone();

    let result = blocking(move || {
        let mut on = |p: setup::Progress| {
            let _ = app.emit("setup-progress", p);
        };
        let downloads = setup::setup_dir();
        let mut bin_dir = None;
        if parts.contains(&PartId::Miktex) {
            let install = setup::install_miktex(&setup::miktex_source(), &downloads, &cancel, &mut on)
                .map_err(err)?;
            bin_dir = Some(install.bin_dir);
        }
        if parts.contains(&PartId::Perl) {
            let home = moss_core::tex::moss_perl_bin()
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf())
                .ok_or("could not work out where to put Perl")?;
            setup::install_perl(&setup::perl_source(), &downloads, &home, &cancel, &mut on)
                .map_err(err)?;
        }
        if warm {
            let dir = bin_dir
                .or_else(|| {
                    moss_core::tex::detect(Settings::load().tex_bin_dir.as_deref())
                        .ok()
                        .map(|t| t.bin_dir)
                })
                .ok_or("No MiKTeX to add packages to")?;
            setup::warm_packages(&dir, setup::COMMON_PACKAGES, &cancel, &mut on).map_err(err)?;
        }
        Ok(())
    })
    .await;

    *setup_state.running.lock().unwrap() = false;
    result?;
    Ok(detect_tex(&state))
}

#[tauri::command]
pub(crate) fn setup_cancel(setup_state: State<'_, SetupState>) {
    setup_state.cancel.store(true, Ordering::Relaxed);
}

#[tauri::command]
pub(crate) async fn setup_self_test() -> CmdResult<SelfTest> {
    blocking(|| Ok(setup::self_test(Settings::load().tex_bin_dir.as_deref()))).await
}

#[tauri::command]
pub(crate) fn setup_onboarded() -> bool {
    Settings::load().onboarded
}

#[tauri::command]
pub(crate) fn setup_mark_onboarded() -> CmdResult<()> {
    let mut settings = Settings::load();
    settings.onboarded = true;
    settings.save().map_err(err)
}

/// Create the "Moss tour" Project from its Template.
#[tauri::command]
pub(crate) fn setup_create_tour(
    state: State<'_, AppState>,
) -> CmdResult<moss_core::workspace::ProjectSummary> {
    state
        .workspace
        .create_project_from("Moss tour", Some("tour"))
        .map_err(err)
}

#[derive(Serialize)]
pub(crate) struct UpdateInfo {
    version: String,
    current: String,
    notes: String,
    pre_release: bool,
}

/// Is a newer Moss published? The updater reads `latest.json` from the release channel set in
/// `tauri.conf.json` and checks its signature. `None` when up to date.
#[tauri::command]
pub(crate) async fn update_check(app: AppHandle) -> CmdResult<Option<UpdateInfo>> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let found = updater.check().await.map_err(|e| e.to_string())?;
    Ok(found.map(|u| UpdateInfo {
        pre_release: u.version.contains('-'),
        version: u.version,
        current: u.current_version,
        notes: u.body.unwrap_or_default(),
    }))
}

/// Download the update (progress arrives as `update-progress` events), install it, and restart.
#[tauri::command]
pub(crate) async fn update_install(app: AppHandle) -> CmdResult<()> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Err("Moss is already up to date".into());
    };
    let mut got: u64 = 0;
    let events = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                got += chunk as u64;
                let _ = events.emit("update-progress", (got, total));
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    app.restart()
}
