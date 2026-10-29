//! Tauri commands for Overleaf: sign in, list, pull, and a safe write test.
//!
//! The session cookie is read out of a dedicated sign-in window (so Google or SSO sign-in works
//! as it would in a browser) and then kept in Windows Credential Manager. Moss never sees or
//! stores the Overleaf password.

use std::time::{Duration, Instant};

use moss_core::overleaf::{
    Client, Cookie, DEFAULT_HOST, RemoteKind, RemoteProject, Session, USER_AGENT, store,
};
use moss_core::project::OverleafLink;
use moss_core::workspace::ImportReport;
use serde::Serialize;
use tauri::webview::{NewWindowResponse, WebviewWindowBuilder};
use tauri::{AppHandle, Emitter, Manager, State, Url, WebviewUrl};

use crate::{AppState, CmdResult, blocking, err};

const LOGIN_LABEL: &str = "overleaf-login";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const TEST_FILE: &str = "moss-write-test.txt";
const TEST_RENAMED: &str = "moss-write-test-renamed.txt";
const TEST_FOLDER: &str = "moss-write-test-folder";

#[derive(Serialize, Clone)]
pub struct OverleafStatus {
    pub(crate) connected: bool,
    pub(crate) email: Option<String>,
    pub(crate) host: String,
}

impl OverleafStatus {
    fn off() -> Self {
        OverleafStatus {
            connected: false,
            email: None,
            host: DEFAULT_HOST.to_string(),
        }
    }
}

#[derive(Serialize)]
pub struct TestStep {
    name: String,
    ok: bool,
    detail: String,
    ms: u64,
}

#[derive(Serialize)]
pub struct WriteTestReport {
    ok: bool,
    steps: Vec<TestStep>,
}

pub(crate) fn current_session(state: &AppState) -> CmdResult<Session> {
    if let Some(s) = state.overleaf.lock().unwrap().clone() {
        return Ok(s);
    }
    match store::load(DEFAULT_HOST).map_err(err)? {
        Some(s) => {
            *state.overleaf.lock().unwrap() = Some(s.clone());
            Ok(s)
        }
        None => Err(moss_core::Error::OverleafAuth.to_string()),
    }
}

/// Verify a session, and if it is good keep it in memory and in Credential Manager.
pub(crate) fn adopt(state: &AppState, session: Session) -> CmdResult<OverleafStatus> {
    let info = Client::new(&session)
        .and_then(|c| c.check_session())
        .map_err(err)?;
    if !info.signed_in {
        return Err(moss_core::Error::OverleafAuth.to_string());
    }
    store::save(&session).map_err(err)?;
    *state.overleaf.lock().unwrap() = Some(session.clone());
    Ok(OverleafStatus {
        connected: true,
        email: info.email,
        host: session.host,
    })
}

#[tauri::command]
pub async fn overleaf_status(state: State<'_, AppState>) -> CmdResult<OverleafStatus> {
    let Ok(session) = current_session(&state) else {
        return Ok(OverleafStatus::off());
    };
    let host = session.host.clone();
    let info = blocking(move || {
        Client::new(&session)
            .and_then(|c| c.check_session())
            .map_err(err)
    })
    .await;
    match info {
        Ok(i) if i.signed_in => Ok(OverleafStatus {
            connected: true,
            email: i.email,
            host,
        }),
        Ok(_) => {
            // Expired: forget it so the UI offers to sign in again.
            *state.overleaf.lock().unwrap() = None;
            let _ = store::clear(&host);
            Ok(OverleafStatus::off())
        }
        // Offline or Overleaf is down: stay "connected" but let the caller see no email.
        Err(_) => Ok(OverleafStatus {
            connected: true,
            email: None,
            host,
        }),
    }
}

/// Fallback for when the sign-in window cannot be used: paste the cookie from a normal browser.
#[tauri::command]
pub async fn overleaf_connect_pasted(
    state: State<'_, AppState>,
    text: String,
) -> CmdResult<OverleafStatus> {
    let session = Session::from_pasted(DEFAULT_HOST, &text).map_err(err)?;
    let app_state: &AppState = &state;
    let verified = {
        let s = session.clone();
        blocking(move || {
            let info = Client::new(&s)
                .and_then(|c| c.check_session())
                .map_err(err)?;
            Ok(info)
        })
        .await?
    };
    if !verified.signed_in {
        return Err(moss_core::Error::OverleafAuth.to_string());
    }
    store::save(&session).map_err(err)?;
    *app_state.overleaf.lock().unwrap() = Some(session.clone());
    Ok(OverleafStatus {
        connected: true,
        email: verified.email,
        host: session.host,
    })
}

#[tauri::command]
pub fn overleaf_sign_out(state: State<'_, AppState>) -> CmdResult<()> {
    *state.overleaf.lock().unwrap() = None;
    store::clear(DEFAULT_HOST).map_err(err)
}

#[tauri::command]
pub async fn overleaf_projects(state: State<'_, AppState>) -> CmdResult<Vec<RemoteProject>> {
    let session = current_session(&state)?;
    blocking(move || {
        let mut projects = Client::new(&session)
            .and_then(|c| c.list_projects())
            .map_err(err)?;
        projects.retain(|p| !p.trashed && !p.archived);
        Ok(projects)
    })
    .await
}

/// Pull: download the project zip, create a new Overleaf-tagged Project, delete the zip.
#[tauri::command]
pub async fn overleaf_pull(
    state: State<'_, AppState>,
    project_id: String,
    name: String,
) -> CmdResult<ImportReport> {
    let session = current_session(&state)?;
    let workspace = state.workspace.clone();
    blocking(move || {
        let bytes = Client::new(&session)
            .and_then(|c| c.download_zip(&project_id))
            .map_err(err)?;
        workspace
            .create_from_overleaf_zip(
                &bytes,
                OverleafLink {
                    project_id,
                    name,
                    last_synced: None,
                },
            )
            .map_err(err)
    })
    .await
}

/// Proves write access end to end without touching your files: upload a tiny file to the
/// project root, confirm it appears, delete it, confirm it is gone.
#[tauri::command]
pub async fn overleaf_write_test(
    state: State<'_, AppState>,
    project_id: String,
) -> CmdResult<WriteTestReport> {
    let session = current_session(&state)?;
    blocking(move || Ok(run_write_test(&session, &project_id))).await
}

fn run_write_test(session: &Session, project_id: &str) -> WriteTestReport {
    let mut steps: Vec<TestStep> = Vec::new();
    let mut step = |name: &str, started: Instant, outcome: Result<String, String>| -> bool {
        let ok = outcome.is_ok();
        steps.push(TestStep {
            name: name.to_string(),
            ok,
            detail: outcome.unwrap_or_else(|e| e),
            ms: started.elapsed().as_millis() as u64,
        });
        ok
    };

    let client = match Client::new(session) {
        Ok(c) => c,
        Err(e) => {
            step("Create client", Instant::now(), Err(e.to_string()));
            return WriteTestReport { ok: false, steps };
        }
    };

    let t = Instant::now();
    let tree = match client.project_tree(project_id) {
        Ok(tree) => {
            step(
                "Read project file tree",
                t,
                Ok(format!("{} entries, root folder found", tree.entries.len())),
            );
            tree
        }
        Err(e) => {
            step("Read project file tree", t, Err(e.to_string()));
            return WriteTestReport { ok: false, steps };
        }
    };
    if tree.entries.iter().any(|e| e.path == TEST_FILE) {
        step(
            "Check for leftover test file",
            Instant::now(),
            Err(format!("`{TEST_FILE}` already exists in this project; delete it first")),
        );
        return WriteTestReport { ok: false, steps };
    }

    let t = Instant::now();
    let uploaded = match client.upload_file(
        project_id,
        &tree.root_folder_id,
        TEST_FILE,
        b"Moss write test. Safe to delete.\n".to_vec(),
    ) {
        Ok(u) => {
            step("Upload test file", t, Ok(format!("created {} {}", u.entity_type, u.entity_id)));
            u
        }
        Err(e) => {
            step("Upload test file", t, Err(e.to_string()));
            return WriteTestReport { ok: false, steps };
        }
    };

    let kind = if uploaded.entity_type == "file" {
        RemoteKind::File
    } else {
        RemoteKind::Doc
    };

    let t = Instant::now();
    let seen = client
        .project_tree(project_id)
        .map(|t| t.entries.iter().any(|e| e.path == TEST_FILE))
        .map_err(|e| e.to_string());
    step(
        "Confirm it appears",
        t,
        match seen {
            Ok(true) => Ok("found in the project tree".into()),
            Ok(false) => Err("upload said success but the file is not in the tree".into()),
            Err(e) => Err(e),
        },
    );

    // What Push also needs: make a folder, rename a file, move it. Each step is checked in the
    // tree, and cleanup below removes whatever was made.
    let mut folder_id: Option<String> = None;
    let mut final_name = TEST_FILE.to_string();

    let t = Instant::now();
    let made = client
        .create_folder(project_id, &tree.root_folder_id, TEST_FOLDER)
        .map(|id| {
            folder_id = Some(id.clone());
            format!("created folder {id}")
        })
        .map_err(|e| e.to_string());
    let folder_ok = step("Create a folder", t, made);

    if folder_ok {
        let t = Instant::now();
        let renamed = client
            .rename_entity(project_id, kind, &uploaded.entity_id, TEST_RENAMED)
            .map(|_| {
                final_name = TEST_RENAMED.to_string();
                format!("renamed to {TEST_RENAMED}")
            })
            .map_err(|e| e.to_string());
        step("Rename the file", t, renamed);

        let t = Instant::now();
        let moved = match &folder_id {
            Some(folder) => client
                .move_entity(project_id, kind, &uploaded.entity_id, folder)
                .map(|_| format!("moved into {TEST_FOLDER}/"))
                .map_err(|e| e.to_string()),
            None => Err("no folder to move into".to_string()),
        };
        let moved_ok = moved.is_ok();
        step("Move the file into the folder", t, moved);

        let t = Instant::now();
        let expected = if moved_ok {
            format!("{TEST_FOLDER}/{final_name}")
        } else {
            final_name.clone()
        };
        let seen = client
            .project_tree(project_id)
            .map(|t| t.entries.iter().any(|e| e.path == expected))
            .map_err(|e| e.to_string());
        step(
            "Confirm the new place",
            t,
            match seen {
                Ok(true) => Ok(format!("found at {expected}")),
                Ok(false) => Err(format!("`{expected}` is not in the project tree")),
                Err(e) => Err(e),
            },
        );
    }

    // Always attempt cleanup once the upload succeeded.
    let t = Instant::now();
    let deleted = client
        .delete_entity(project_id, kind, &uploaded.entity_id)
        .map(|_| "deleted".to_string())
        .map_err(|e| format!("{e}; please delete `{final_name}` from the project by hand"));
    let delete_ok = step("Delete test file", t, deleted);

    let mut folder_cleanup_ok = true;
    if let Some(folder) = &folder_id {
        let t = Instant::now();
        let removed = client
            .delete_entity(project_id, RemoteKind::Folder, folder)
            .map(|_| "deleted".to_string())
            .map_err(|e| format!("{e}; please delete the folder `{TEST_FOLDER}` by hand"));
        folder_cleanup_ok = step("Delete test folder", t, removed);
    }

    let t = Instant::now();
    let gone = client
        .project_tree(project_id)
        .map(|t| {
            !t.entries
                .iter()
                .any(|e| e.path == TEST_FILE || e.path == TEST_RENAMED || e.path.starts_with(TEST_FOLDER))
        })
        .map_err(|e| e.to_string());
    step(
        "Confirm it is gone",
        t,
        match gone {
            Ok(true) => Ok("project is back to how it was".into()),
            Ok(false) => Err("some test files are still there; delete `moss-write-test*` by hand".to_string()),
            Err(e) => Err(e),
        },
    );

    let ok = delete_ok && folder_cleanup_ok && steps.iter().all(|s| s.ok);
    WriteTestReport { ok, steps }
}

/// Open Overleaf's own sign-in page in a separate window and watch for success.
#[tauri::command]
pub async fn overleaf_open_login(app: AppHandle) -> CmdResult<()> {
    if let Some(existing) = app.get_webview_window(LOGIN_LABEL) {
        let _ = existing.set_focus();
        return Ok(());
    }
    let url: Url = format!("https://{DEFAULT_HOST}/login")
        .parse()
        .map_err(|e| format!("bad login url: {e}"))?;
    // Own profile folder: keeps Overleaf's cookies out of the Moss window's storage.
    let profile = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("overleaf-login");

    WebviewWindowBuilder::new(&app, LOGIN_LABEL, WebviewUrl::External(url))
        .title("Sign in to Overleaf")
        .inner_size(520.0, 760.0)
        .user_agent(USER_AGENT)
        .data_directory(profile)
        // "Continue with Google" opens a popup; let it.
        .on_new_window(|_url, _features| NewWindowResponse::Allow)
        .build()
        .map_err(|e| format!("could not open the sign-in window: {e}"))?;

    let handle = app.clone();
    std::thread::spawn(move || watch_login(handle));
    Ok(())
}

/// Poll the sign-in window until Overleaf lands on the project dashboard, then take the cookies.
fn watch_login(app: AppHandle) {
    let started = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(700));
        let Some(window) = app.get_webview_window(LOGIN_LABEL) else {
            let _ = app.emit("overleaf-login-closed", ());
            return;
        };
        if started.elapsed() > LOGIN_TIMEOUT {
            let _ = window.close();
            let _ = app.emit("overleaf-login-closed", ());
            return;
        }
        let on_dashboard = window
            .url()
            .map(|u| u.host_str() == Some(DEFAULT_HOST) && u.path().starts_with("/project"))
            .unwrap_or(false);
        if !on_dashboard {
            continue;
        }
        let Ok(site) = format!("https://{DEFAULT_HOST}/").parse::<Url>() else {
            return;
        };
        let Ok(raw) = window.cookies_for_url(site) else {
            continue;
        };
        let cookies: Vec<Cookie> = raw
            .iter()
            .map(|c| Cookie {
                name: c.name().to_string(),
                value: c.value().to_string(),
            })
            .collect();
        if !cookies.iter().any(|c| c.name.starts_with("overleaf_session")) {
            continue;
        }
        let state = app.state::<AppState>();
        match adopt(&state, Session::new(DEFAULT_HOST, cookies)) {
            Ok(status) => {
                let _ = window.close();
                let _ = app.emit("overleaf-connected", status);
                return;
            }
            Err(_) => continue, // not fully signed in yet
        }
    }
}
