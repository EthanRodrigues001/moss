//! Talking to Overleaf with the signed-in browser session (see ADR 0002).
//!
//! This uses Overleaf's private web endpoints, the same ones its own web app calls. They are
//! unsupported and can change without notice, so everything Overleaf-specific lives in this
//! module behind `Client`; nothing else in Moss knows an Overleaf URL.

mod html;
pub mod remote;
pub mod socket;
pub mod store;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::{Client as Http, RequestBuilder, Response, multipart};
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

pub use remote::OverleafRemote;
pub use socket::{RemoteEntry, RemoteKind, RemoteTree};

pub const DEFAULT_HOST: &str = "www.overleaf.com";

/// A normal desktop-browser user agent. Cookies issued to a browser are only honoured by some
/// edge services when the same agent comes back, so the login window uses this exact string too.
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
(KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub host: String,
    pub cookies: Vec<Cookie>,
}

impl Session {
    pub fn new(host: impl Into<String>, cookies: Vec<Cookie>) -> Session {
        Session {
            host: host.into(),
            cookies,
        }
    }

    /// Build a session from text the user pasted: either the bare `overleaf_session2` value or a
    /// full `name=value; name2=value2` cookie string copied from the browser.
    pub fn from_pasted(host: &str, text: &str) -> Result<Session> {
        let text = text.trim().trim_start_matches("Cookie:").trim();
        if text.is_empty() {
            return Err(Error::Overleaf("nothing was pasted".into()));
        }
        let cookies: Vec<Cookie> = if text.contains('=') {
            text.split(';')
                .filter_map(|pair| {
                    let (name, value) = pair.trim().split_once('=')?;
                    Some(Cookie {
                        name: name.trim().to_string(),
                        value: value.trim().to_string(),
                    })
                })
                .collect()
        } else {
            vec![Cookie {
                name: "overleaf_session2".into(),
                value: text.to_string(),
            }]
        };
        if !cookies.iter().any(|c| c.name.starts_with("overleaf_session")) {
            return Err(Error::Overleaf(
                "no overleaf_session2 cookie found in what was pasted".into(),
            ));
        }
        Ok(Session::new(host, cookies))
    }

    pub fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Keep only the cookies Overleaf needs, so the session fits in Windows Credential Manager.
    pub fn essential(&self) -> Session {
        let keep: Vec<Cookie> = self
            .cookies
            .iter()
            .filter(|c| c.name.starts_with("overleaf_session") || c.name == "cf_clearance")
            .cloned()
            .collect();
        Session::new(self.host.clone(), if keep.is_empty() { self.cookies.clone() } else { keep })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteProject {
    pub id: String,
    pub name: String,
    pub last_updated: Option<String>,
    pub access_level: Option<String>,
    pub owner: Option<String>,
    pub archived: bool,
    pub trashed: bool,
}

/// One entry of Overleaf's own project history ("All history" in its History view).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RemoteUpdate {
    pub from_version: u64,
    pub to_version: u64,
    /// Display names of the people who made the edits.
    pub users: Vec<String>,
    /// Unix seconds.
    pub started: i64,
    pub ended: i64,
    pub edited: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// `old -> new`.
    pub renamed: Vec<String>,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteHistory {
    pub updates: Vec<RemoteUpdate>,
    /// Pass back as `before` to get older entries; `None` at the end.
    pub next_before: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub signed_in: bool,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Uploaded {
    pub entity_id: String,
    pub entity_type: String,
}

pub struct Client {
    http: Http,
    host: String,
    cookie: String,
    /// One CSRF token per project page, reused for every write in a Sync.
    csrf_tokens: Mutex<HashMap<String, String>>,
}

/// Overleaf answers 429/5xx when it is busy; a few quick retries usually get through.
const RETRIES: u32 = 3;

impl Client {
    pub fn new(session: &Session) -> Result<Client> {
        let http = Http::builder()
            .user_agent(USER_AGENT)
            .redirect(Policy::limited(5))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| Error::Overleaf(format!("could not create HTTP client: {e}")))?;
        Ok(Client {
            http,
            host: session.host.clone(),
            cookie: session.cookie_header(),
            csrf_tokens: Mutex::new(HashMap::new()),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("https://{}{}", self.host, path)
    }

    fn get(&self, path: &str) -> RequestBuilder {
        self.http.get(self.url(path)).header("Cookie", &self.cookie)
    }

    fn send(&self, req: RequestBuilder) -> Result<Response> {
        let resp = req
            .send()
            .map_err(|e| Error::Overleaf(format!("request failed: {e}")))?;
        let status = resp.status();
        if status == 401 || status == 403 {
            return Err(Error::OverleafAuth);
        }
        // A signed-out session is redirected to the login page, which still answers 200.
        if resp.url().path().starts_with("/login") {
            return Err(Error::OverleafAuth);
        }
        if !status.is_success() {
            return Err(Error::Overleaf(format!("{} returned HTTP {status}", resp.url().path())));
        }
        Ok(resp)
    }

    fn page(&self, path: &str) -> Result<String> {
        self.send(self.get(path))?
            .text()
            .map_err(|e| Error::Overleaf(format!("could not read page: {e}")))
    }

    /// Is this session still signed in? Cheap: one page load.
    pub fn check_session(&self) -> Result<SessionInfo> {
        match self.page("/project") {
            Ok(page) => Ok(SessionInfo {
                signed_in: true,
                email: html::meta_content(&page, "ol-usersEmail")
                    .or_else(|| html::meta_content(&page, "ol-user_email")),
            }),
            Err(Error::OverleafAuth) => Ok(SessionInfo {
                signed_in: false,
                email: None,
            }),
            Err(e) => Err(e),
        }
    }

    pub fn list_projects(&self) -> Result<Vec<RemoteProject>> {
        let page = self.page("/project")?;
        let blob = html::meta_content(&page, "ol-prefetchedProjectsBlob").ok_or_else(|| {
            Error::Overleaf(
                "the project list was not on the page; Overleaf may have changed its layout".into(),
            )
        })?;
        parse_projects(&blob)
    }

    /// Overleaf's own change history for a project, newest first (who changed which files, when).
    /// `before` is a millisecond timestamp from the previous page.
    pub fn history(&self, project_id: &str, before: Option<i64>) -> Result<RemoteHistory> {
        let mut path = format!("/project/{project_id}/updates?min_count=30");
        if let Some(ts) = before {
            path.push_str(&format!("&before={ts}"));
        }
        let resp = self.send(self.get(&path).header("Accept", "application/json"))?;
        let body: Value = resp
            .json()
            .map_err(|e| Error::Overleaf(format!("unreadable history response: {e}")))?;
        parse_history(&body)
    }

    /// CSRF token for write requests. Overleaf issues one per page load; it is kept for the
    /// life of this client so a Sync of many files loads the page once.
    fn csrf(&self, project_id: &str) -> Result<String> {
        if let Some(token) = self.csrf_tokens.lock().unwrap().get(project_id) {
            return Ok(token.clone());
        }
        let page = self.page(&format!("/project/{project_id}"))?;
        let token = html::meta_content(&page, "ol-csrfToken")
            .ok_or_else(|| Error::Overleaf("no CSRF token on the project page".into()))?;
        self.csrf_tokens
            .lock()
            .unwrap()
            .insert(project_id.to_string(), token.clone());
        Ok(token)
    }

    /// Send a write, retrying when Overleaf is busy. `make` builds a fresh request each time
    /// because a request body can only be sent once.
    fn send_write(&self, make: impl Fn() -> RequestBuilder) -> Result<Response> {
        let mut attempt = 0;
        loop {
            match self.send(make()) {
                Err(e) if attempt < RETRIES && is_transient(&e) => {
                    std::thread::sleep(Duration::from_millis(400 << attempt));
                    attempt += 1;
                }
                other => return other,
            }
        }
    }

    /// The whole project as a zip (this is what Pull downloads).
    pub fn download_zip(&self, project_id: &str) -> Result<Vec<u8>> {
        let resp = self.send(self.get(&format!("/project/{project_id}/download/zip")))?;
        let bytes = resp
            .bytes()
            .map_err(|e| Error::Overleaf(format!("download failed: {e}")))?
            .to_vec();
        if !bytes.starts_with(b"PK") {
            return Err(Error::Overleaf("the download was not a zip file".into()));
        }
        Ok(bytes)
    }

    /// File tree with ids, plus the root folder id that uploads need.
    pub fn project_tree(&self, project_id: &str) -> Result<RemoteTree> {
        let mut attempt = 0;
        loop {
            match self.project_tree_once(project_id) {
                Err(e) if attempt < RETRIES && is_transient(&e) => {
                    std::thread::sleep(Duration::from_millis(400 << attempt));
                    attempt += 1;
                }
                other => return other,
            }
        }
    }

    fn project_tree_once(&self, project_id: &str) -> Result<RemoteTree> {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let resp =
            self.send(self.get(&format!("/socket.io/1/?projectId={project_id}&t={millis}")))?;
        // The handshake sets a load-balancer cookie (GCLB) that pins the websocket to the same
        // realtime server; without it the upgrade can land elsewhere and fail with a 502.
        let cookie = merge_set_cookies(&self.cookie, resp.headers());
        let handshake = resp
            .text()
            .map_err(|e| Error::Overleaf(format!("realtime handshake failed: {e}")))?;
        let sid = handshake
            .split(':')
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error::Overleaf("realtime handshake returned no session id".into()))?;
        socket::read_tree(&self.host, project_id, sid, &cookie, USER_AGENT)
    }

    fn write_headers(&self, req: RequestBuilder, project_id: &str, csrf: &str) -> RequestBuilder {
        req.header("Cookie", &self.cookie)
            .header("x-csrf-token", csrf)
            .header("Referer", self.url(&format!("/project/{project_id}")))
            .header("Origin", format!("https://{}", self.host))
            .header("Accept", "application/json")
            .header("Cache-Control", "no-cache")
    }

    /// Upload a file into a folder, replacing a file of the same name (which keeps its history).
    pub fn upload_file(
        &self,
        project_id: &str,
        folder_id: &str,
        name: &str,
        bytes: Vec<u8>,
    ) -> Result<Uploaded> {
        let csrf = self.csrf(project_id)?;
        let resp = self.send_write(|| {
            let form = multipart::Form::new()
                .text("relativePath", "null")
                .text("name", name.to_string())
                .text("type", "application/octet-stream")
                .part(
                    "qqfile",
                    multipart::Part::bytes(bytes.clone()).file_name(name.to_string()),
                );
            let req = self
                .http
                .post(self.url(&format!("/project/{project_id}/upload?folder_id={folder_id}")))
                .multipart(form);
            self.write_headers(req, project_id, &csrf)
        })?;
        let body: Value = resp
            .json()
            .map_err(|e| Error::Overleaf(format!("unreadable upload response: {e}")))?;
        if body["success"].as_bool() == Some(false) {
            return Err(Error::Overleaf(format!(
                "upload rejected: {}",
                body["error"].as_str().unwrap_or("unknown reason")
            )));
        }
        Ok(Uploaded {
            entity_id: body["entity_id"].as_str().unwrap_or_default().to_string(),
            entity_type: body["entity_type"].as_str().unwrap_or_default().to_string(),
        })
    }

    pub fn delete_entity(&self, project_id: &str, kind: RemoteKind, entity_id: &str) -> Result<()> {
        let csrf = self.csrf(project_id)?;
        self.send_write(|| {
            let req = self
                .http
                .delete(self.url(&format!(
                    "/project/{project_id}/{}/{entity_id}",
                    kind.url_segment()
                )))
                .json(&serde_json::json!({}));
            self.write_headers(req, project_id, &csrf)
        })?;
        Ok(())
    }

    /// Create a folder inside `parent_folder_id`; returns the new folder's id.
    pub fn create_folder(&self, project_id: &str, parent_folder_id: &str, name: &str) -> Result<String> {
        let csrf = self.csrf(project_id)?;
        let resp = self.send_write(|| {
            let req = self
                .http
                .post(self.url(&format!("/project/{project_id}/folder")))
                .json(&serde_json::json!({ "parent_folder_id": parent_folder_id, "name": name }));
            self.write_headers(req, project_id, &csrf)
        })?;
        let body: Value = resp
            .json()
            .map_err(|e| Error::Overleaf(format!("unreadable folder response: {e}")))?;
        body["_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| Error::Overleaf("the new folder had no id".into()))
    }

    /// Give a file, doc or folder a new name in place.
    pub fn rename_entity(&self, project_id: &str, kind: RemoteKind, entity_id: &str, name: &str) -> Result<()> {
        let csrf = self.csrf(project_id)?;
        self.send_write(|| {
            let req = self
                .http
                .post(self.url(&format!(
                    "/project/{project_id}/{}/{entity_id}/rename",
                    kind.url_segment()
                )))
                .json(&serde_json::json!({ "name": name }));
            self.write_headers(req, project_id, &csrf)
        })?;
        Ok(())
    }

    /// Move a file, doc or folder into another folder.
    pub fn move_entity(&self, project_id: &str, kind: RemoteKind, entity_id: &str, folder_id: &str) -> Result<()> {
        let csrf = self.csrf(project_id)?;
        self.send_write(|| {
            let req = self
                .http
                .post(self.url(&format!(
                    "/project/{project_id}/{}/{entity_id}/move",
                    kind.url_segment()
                )))
                .json(&serde_json::json!({ "folder_id": folder_id }));
            self.write_headers(req, project_id, &csrf)
        })?;
        Ok(())
    }

    /// Create a new Overleaf project from a zip (what "Upload project" does). Returns its id.
    pub fn create_project_from_zip(&self, name: &str, zip: Vec<u8>) -> Result<String> {
        let page = self.page("/project")?;
        let csrf = html::meta_content(&page, "ol-csrfToken")
            .ok_or_else(|| Error::Overleaf("no CSRF token on the project list page".into()))?;
        let file_name = format!("{name}.zip");
        let resp = self.send_write(|| {
            let form = multipart::Form::new()
                .text("relativePath", "null")
                .text("name", file_name.clone())
                .text("type", "application/zip")
                .part(
                    "qqfile",
                    multipart::Part::bytes(zip.clone()).file_name(file_name.clone()),
                );
            self.http
                .post(self.url("/project/new/upload"))
                .multipart(form)
                .header("Cookie", &self.cookie)
                .header("x-csrf-token", &csrf)
                .header("Referer", self.url("/project"))
                .header("Origin", format!("https://{}", self.host))
                .header("Accept", "application/json")
        })?;
        let body: Value = resp
            .json()
            .map_err(|e| Error::Overleaf(format!("unreadable response to the project upload: {e}")))?;
        if body["success"].as_bool() == Some(false) {
            return Err(Error::Overleaf(format!(
                "Overleaf rejected the project upload: {}",
                body["error"].as_str().unwrap_or("unknown reason")
            )));
        }
        body["project_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| Error::Overleaf("the upload response had no project id".into()))
    }
}

/// Read the `/updates` response. Every field is optional on purpose: Overleaf's history format has
/// changed before, and a missing field must not hide the rest.
pub fn parse_history(body: &Value) -> Result<RemoteHistory> {
    let list = body["updates"]
        .as_array()
        .ok_or_else(|| Error::Overleaf("the history response had an unexpected shape".into()))?;
    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect()
    };
    let updates = list
        .iter()
        .map(|u| {
            let meta = &u["meta"];
            let users: Vec<String> = meta["users"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|user| {
                    let name = [user["first_name"].as_str(), user["last_name"].as_str()]
                        .into_iter()
                        .flatten()
                        .filter(|p| !p.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !name.is_empty() {
                        Some(name)
                    } else {
                        user["email"].as_str().map(str::to_string)
                    }
                })
                .collect();
            let ops = |key: &str, field: &str| -> Vec<String> {
                u["project_ops"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|op| op[key][field].as_str().map(str::to_string))
                    .collect()
            };
            let renamed = u["project_ops"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|op| {
                    let r = &op["rename"];
                    Some(format!("{} -> {}", r["pathname"].as_str()?, r["newPathname"].as_str()?))
                })
                .collect();
            RemoteUpdate {
                from_version: u["fromV"].as_u64().unwrap_or(0),
                to_version: u["toV"].as_u64().unwrap_or(0),
                users,
                started: meta["start_ts"].as_i64().unwrap_or(0) / 1000,
                ended: meta["end_ts"].as_i64().unwrap_or(0) / 1000,
                edited: strings(&u["pathnames"]),
                added: ops("add", "pathname"),
                removed: ops("remove", "pathname"),
                renamed,
                labels: u["labels"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|l| l["comment"].as_str().map(str::to_string))
                    .collect(),
            }
        })
        .collect();
    Ok(RemoteHistory {
        updates,
        next_before: body["nextBeforeTimestamp"].as_i64(),
    })
}

/// `base` (a `Cookie` header value) plus the cookies a response sets, a new value replacing an
/// old one of the same name.
fn merge_set_cookies(base: &str, headers: &reqwest::header::HeaderMap) -> String {
    let mut pairs: Vec<(String, String)> = base
        .split(';')
        .filter_map(|p| p.trim().split_once('='))
        .map(|(n, v)| (n.to_string(), v.to_string()))
        .collect();
    for value in headers.get_all(reqwest::header::SET_COOKIE) {
        let Ok(text) = value.to_str() else { continue };
        let Some((name, val)) = text.split(';').next().and_then(|p| p.split_once('=')) else {
            continue;
        };
        let (name, val) = (name.trim().to_string(), val.trim().to_string());
        match pairs.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = val,
            None => pairs.push((name, val)),
        }
    }
    pairs
        .iter()
        .map(|(n, v)| format!("{n}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Worth trying again: Overleaf is busy, or the connection dropped.
fn is_transient(e: &Error) -> bool {
    match e {
        Error::Overleaf(msg) => {
            msg.contains("HTTP 429")
                || msg.contains("502 Bad Gateway")
                || msg.contains("503 Service")
                || msg.contains("504 Gateway")
                || msg.contains("HTTP 500")
                || msg.contains("HTTP 502")
                || msg.contains("HTTP 503")
                || msg.contains("HTTP 504")
                || msg.starts_with("request failed")
        }
        _ => false,
    }
}

pub fn parse_projects(blob: &str) -> Result<Vec<RemoteProject>> {
    let v: Value = serde_json::from_str(blob)
        .map_err(|e| Error::json("reading the Overleaf project list", e))?;
    let list = v["projects"].as_array().ok_or_else(|| {
        Error::Overleaf("the project list had an unexpected shape".into())
    })?;
    Ok(list
        .iter()
        .filter_map(|p| {
            let owner = &p["owner"];
            let owner_name = [owner["firstName"].as_str(), owner["lastName"].as_str()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            Some(RemoteProject {
                id: p["id"].as_str()?.to_string(),
                name: p["name"].as_str()?.to_string(),
                last_updated: p["lastUpdated"].as_str().map(str::to_string),
                access_level: p["accessLevel"].as_str().map(str::to_string),
                owner: (!owner_name.is_empty()).then_some(owner_name),
                archived: p["archived"].as_bool().unwrap_or(false),
                trashed: p["trashed"].as_bool().unwrap_or(false),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read-only diagnostic against the real Overleaf: does the realtime handshake work with and
    /// without the cookies the handshake response sets? Run with
    /// `cargo test -p moss-core live_realtime -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs a signed-in Moss session and network"]
    fn live_realtime_handshake() {
        let session = store::load(DEFAULT_HOST).unwrap().expect("not signed in");
        let client = Client::new(&session).unwrap();
        let projects = client.list_projects().unwrap();
        let project = std::env::var("MOSS_LIVE_PROJECT")
            .ok()
            .and_then(|id| projects.iter().find(|p| p.id == id))
            .or_else(|| projects.iter().find(|p| !p.archived && !p.trashed))
            .expect("no project");
        println!("project {} ({})", project.name, project.id);
        let resp = client
            .send(client.get(&format!("/socket.io/1/?projectId={}&t=1", project.id)))
            .unwrap();
        for value in resp.headers().get_all(reqwest::header::SET_COOKIE) {
            let text = value.to_str().unwrap_or("");
            println!("set-cookie name: {}", text.split('=').next().unwrap_or(""));
        }
        let body = resp.text().unwrap();
        println!("handshake body: {body}");
        // The websocket upgrade without the handshake's cookies (how Moss used to do it), a few times.
        for i in 1..=4 {
            let resp = client
                .send(client.get(&format!("/socket.io/1/?projectId={}&t={i}", project.id)))
                .unwrap();
            let body = resp.text().unwrap();
            let sid = body.split(':').next().unwrap();
            let bare = socket::read_tree(&client.host, &project.id, sid, &client.cookie, USER_AGENT);
            println!("without cookies, try {i}: {}", bare.map(|t| t.entries.len().to_string()).unwrap_or_else(|e| e.to_string()));
        }
        let tree = client.project_tree(&project.id).unwrap();
        println!("file tree: {} entries", tree.entries.len());
    }

    #[test]
    fn handshake_cookies_are_merged_into_the_cookie_header() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.append(
            reqwest::header::SET_COOKIE,
            "GCLB=abc; path=/; HttpOnly".parse().unwrap(),
        );
        headers.append(reqwest::header::SET_COOKIE, "overleaf_session2=new; Secure".parse().unwrap());
        let merged = merge_set_cookies("overleaf_session2=old; other=1", &headers);
        assert_eq!(merged, "overleaf_session2=new; other=1; GCLB=abc");
    }

    #[test]
    fn pasted_bare_value_becomes_the_session_cookie() {
        let s = Session::from_pasted(DEFAULT_HOST, "  s%3Aabc.def  ").unwrap();
        assert_eq!(s.cookies[0].name, "overleaf_session2");
        assert_eq!(s.cookie_header(), "overleaf_session2=s%3Aabc.def");
    }

    #[test]
    fn pasted_cookie_string_is_split() {
        let s = Session::from_pasted(
            DEFAULT_HOST,
            "Cookie: overleaf_session2=s%3Aabc; GCLB=xyz; cf_clearance=q",
        )
        .unwrap();
        assert_eq!(s.cookies.len(), 3);
        let essential = s.essential();
        assert_eq!(essential.cookies.len(), 2); // GCLB dropped
    }

    #[test]
    fn pasting_without_a_session_cookie_is_rejected() {
        assert!(Session::from_pasted(DEFAULT_HOST, "GCLB=xyz").is_err());
        assert!(Session::from_pasted(DEFAULT_HOST, "   ").is_err());
    }

    #[test]
    fn parses_overleaf_history_and_tolerates_missing_fields() {
        let body: Value = serde_json::from_str(
            r#"{"updates":[
              {"fromV":10,"toV":12,"meta":{"users":[{"first_name":"Riya","last_name":"R","email":"r@x.com"}],
               "start_ts":1761400000000,"end_ts":1761400060000},
               "pathnames":["Chapter5.tex"],
               "project_ops":[{"add":{"pathname":"fig.jpeg"},"atV":11},{"remove":{"pathname":"old.tex"},"atV":12},
                              {"rename":{"pathname":"a.tex","newPathname":"b.tex"},"atV":12}],
               "labels":[{"comment":"v1"}]},
              {"toV":9,"meta":{"users":[{"email":"only@mail.com"}]}}
             ],"nextBeforeTimestamp":1761399000000}"#,
        )
        .unwrap();
        let h = parse_history(&body).unwrap();
        assert_eq!(h.next_before, Some(1761399000000));
        let first = &h.updates[0];
        assert_eq!(first.users, ["Riya R"]);
        assert_eq!((first.from_version, first.to_version, first.started), (10, 12, 1761400000));
        assert_eq!(first.edited, ["Chapter5.tex"]);
        assert_eq!((first.added.as_slice(), first.removed.as_slice()), (&["fig.jpeg".to_string()][..], &["old.tex".to_string()][..]));
        assert_eq!(first.renamed, ["a.tex -> b.tex"]);
        assert_eq!(first.labels, ["v1"]);
        assert_eq!(h.updates[1].users, ["only@mail.com"]);
        assert!(h.updates[1].edited.is_empty());
        assert!(parse_history(&serde_json::json!({"nope":1})).is_err());
    }

    #[test]
    fn parses_the_project_blob() {
        let blob = r#"{"totalSize":2,"projects":[
          {"id":"p1","name":"Report","lastUpdated":"2026-10-01T10:00:00.000Z","accessLevel":"owner",
           "archived":false,"trashed":false,"owner":{"firstName":"Ethan","lastName":"R"}},
          {"id":"p2","name":"Old","archived":true,"trashed":false},
          {"name":"no id, skipped"}]}"#;
        let p = parse_projects(blob).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].owner.as_deref(), Some("Ethan R"));
        assert!(p[1].archived);
    }
}
