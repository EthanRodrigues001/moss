//! "Connect from Chrome": a short-lived, loopback-only listener that the Moss Connector browser
//! extension posts the Overleaf session to.
//!
//! Why this exists: Chrome encrypts its cookies so other programs cannot read them (and Moss
//! must not try). An extension, however, may read the user's own Overleaf cookies through the
//! browser's cookies API, so it can hand them to Moss with one click, reusing whatever Google
//! account Chrome is already signed in to.
//!
//! Safety: it listens on 127.0.0.1 only, for at most three minutes, accepts one session, checks
//! the `Host` header (defeats DNS rebinding), and requires a six-digit code that Moss shows on
//! screen. Five wrong codes shut it down.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use moss_core::overleaf::{Cookie, DEFAULT_HOST, Session};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::overleaf::adopt;
use crate::{AppState, CmdResult};

pub const PORTS: std::ops::RangeInclusive<u16> = 47831..=47835;
const LIFETIME: Duration = Duration::from_secs(180);
const MAX_FAILURES: u8 = 5;
const MAX_BODY: usize = 64 * 1024;

pub struct Bridge {
    code: String,
    deadline: Instant,
    failures: AtomicU8,
    stop: AtomicBool,
}

impl Bridge {
    fn active(&self) -> bool {
        !self.stop.load(Ordering::SeqCst) && Instant::now() < self.deadline
    }
}

#[derive(Serialize)]
pub struct Pairing {
    code: String,
    port: u16,
    seconds: u64,
    extension_dir: Option<String>,
}

#[derive(Deserialize)]
struct SessionPost {
    code: String,
    cookies: Vec<CookiePost>,
}

#[derive(Deserialize)]
struct CookiePost {
    name: String,
    value: String,
}

fn six_digits() -> String {
    format!("{:06}", uuid::Uuid::new_v4().as_u128() % 1_000_000)
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Find the unpacked extension next to the app (installed build) or in the repo (dev).
fn extension_dir(app: &AppHandle) -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        roots.push(res);
    }
    if let Ok(exe) = std::env::current_exe() {
        roots.extend(exe.ancestors().map(PathBuf::from));
    }
    roots
        .into_iter()
        .flat_map(|r| [r.join("extensions").join("chrome"), r.join("chrome")])
        .find(|p| p.join("manifest.json").is_file())
}

#[tauri::command]
pub fn overleaf_start_chrome_pairing(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<Pairing> {
    let extension_dir = extension_dir(&app).map(|p| p.display().to_string());

    // Reuse a pairing that is still open (the user may have reopened the dialog).
    let mut slot = state.bridge.lock().unwrap();
    if let Some(existing) = slot.as_ref().filter(|b| b.active()) {
        let left = existing.deadline.saturating_duration_since(Instant::now());
        return Ok(Pairing {
            code: existing.code.clone(),
            port: *PORTS.start(),
            seconds: left.as_secs(),
            extension_dir,
        });
    }

    let listener = PORTS
        .clone()
        .find_map(|p| TcpListener::bind(("127.0.0.1", p)).ok())
        .ok_or_else(|| {
            "Could not open a local port for the Chrome connector (ports 47831-47835 are busy)"
                .to_string()
        })?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;

    let bridge = Arc::new(Bridge {
        code: six_digits(),
        deadline: Instant::now() + LIFETIME,
        failures: AtomicU8::new(0),
        stop: AtomicBool::new(false),
    });
    *slot = Some(bridge.clone());
    let code = bridge.code.clone();

    std::thread::spawn(move || serve(listener, port, bridge, app));
    Ok(Pairing {
        code,
        port,
        seconds: LIFETIME.as_secs(),
        extension_dir,
    })
}

#[tauri::command]
pub fn overleaf_stop_chrome_pairing(state: State<'_, AppState>) {
    if let Some(b) = state.bridge.lock().unwrap().take() {
        b.stop.store(true, Ordering::SeqCst);
    }
}

/// Open the extension folder in Explorer so the user can pick it in chrome://extensions.
#[tauri::command]
pub fn reveal_in_explorer(path: String) -> CmdResult<()> {
    let p = PathBuf::from(&path);
    if !p.is_dir() {
        return Err("That folder does not exist".into());
    }
    std::process::Command::new("explorer")
        .arg(p)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Open a link from the PDF preview in the default browser or mail app. Only web and mail links are
/// allowed: a PDF is untrusted input and must not start arbitrary programs. `rundll32` takes the
/// address as one argument, so characters like `&` are not read by a shell.
#[tauri::command]
pub fn open_external(url: String) -> CmdResult<()> {
    let lower = url.to_ascii_lowercase();
    let allowed = ["http://", "https://", "mailto:"];
    if !allowed.iter().any(|p| lower.starts_with(p)) {
        return Err("Only web and mail links can be opened".into());
    }
    std::process::Command::new("rundll32")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn serve(listener: TcpListener, port: u16, bridge: Arc<Bridge>, app: AppHandle) {
    while bridge.active() {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                if handle(stream, port, &bridge, &app) {
                    bridge.stop.store(true, Ordering::SeqCst);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

struct Request {
    method: String,
    path: String,
    host: String,
    body: Vec<u8>,
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();

    let (mut host, mut length) = (String::new(), 0usize);
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok()?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "host" => host = value.trim().to_ascii_lowercase(),
                "content-length" => length = value.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    if length > MAX_BODY {
        return None;
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    Some(Request {
        method,
        path,
        host,
        body,
    })
}

fn respond(mut stream: &TcpStream, status: &str, json: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: content-type\r\nConnection: close\r\n\r\n{json}",
        json.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

/// Returns true when a session was accepted and the bridge should shut down.
fn handle(stream: TcpStream, port: u16, bridge: &Bridge, app: &AppHandle) -> bool {
    let Some(req) = read_request(&stream) else {
        respond(&stream, "400 Bad Request", r#"{"ok":false}"#);
        return false;
    };
    let host_ok = req.host == format!("127.0.0.1:{port}") || req.host == format!("localhost:{port}");
    if !host_ok {
        respond(&stream, "403 Forbidden", r#"{"ok":false,"error":"bad host"}"#);
        return false;
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("OPTIONS", _) => {
            respond(&stream, "204 No Content", "");
            false
        }
        ("GET", "/moss/ping") => {
            respond(&stream, "200 OK", r#"{"moss":true}"#);
            false
        }
        ("POST", "/moss/overleaf-session") => accept_session(&stream, &req, bridge, app),
        _ => {
            respond(&stream, "404 Not Found", r#"{"ok":false}"#);
            false
        }
    }
}

fn accept_session(stream: &TcpStream, req: &Request, bridge: &Bridge, app: &AppHandle) -> bool {
    let Ok(post) = serde_json::from_slice::<SessionPost>(&req.body) else {
        respond(stream, "400 Bad Request", r#"{"ok":false,"error":"unreadable request"}"#);
        return false;
    };
    if !same(&post.code, &bridge.code) {
        let failures = bridge.failures.fetch_add(1, Ordering::SeqCst) + 1;
        respond(stream, "403 Forbidden", r#"{"ok":false,"error":"wrong code"}"#);
        // Too many guesses: stop listening altogether.
        return failures >= MAX_FAILURES;
    }
    let cookies: Vec<Cookie> = post
        .cookies
        .into_iter()
        .map(|c| Cookie {
            name: c.name,
            value: c.value,
        })
        .collect();
    let state = app.state::<AppState>();
    match adopt(&state, Session::new(DEFAULT_HOST, cookies)) {
        Ok(status) => {
            let email = status.email.clone().unwrap_or_default();
            let _ = app.emit("overleaf-connected", status);
            respond(stream, "200 OK", &format!(r#"{{"ok":true,"email":{}}}"#, serde_json::json!(email)));
            true
        }
        Err(e) => {
            respond(
                stream,
                "401 Unauthorized",
                &format!(r#"{{"ok":false,"error":{}}}"#, serde_json::json!(e)),
            );
            false
        }
    }
}
