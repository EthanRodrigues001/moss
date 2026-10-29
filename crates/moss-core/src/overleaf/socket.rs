//! Reading a project's file tree over Overleaf's legacy socket.io (v0.9) channel.
//!
//! The only way a web client learns the root folder id (needed for uploads) and every file's id
//! (needed for deletes and renames) is the `joinProjectResponse` event the editor receives when
//! it opens a project. We do the same handshake the editor does and read that one event.

use std::net::TcpStream;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, client::IntoClientRequest};

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RemoteKind {
    Folder,
    /// Editable text (`.tex`, `.bib`, …).
    Doc,
    /// Binary file (images, PDFs, …).
    File,
}

impl RemoteKind {
    /// The path segment Overleaf uses for this kind in delete and rename URLs.
    pub fn url_segment(self) -> &'static str {
        match self {
            RemoteKind::Folder => "folder",
            RemoteKind::Doc => "doc",
            RemoteKind::File => "file",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteEntry {
    /// `/`-separated path relative to the project root, no leading slash.
    pub path: String,
    pub id: String,
    pub kind: RemoteKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteTree {
    pub project_name: Option<String>,
    pub root_folder_id: String,
    pub entries: Vec<RemoteEntry>,
}

/// What one socket.io frame means to us.
#[derive(Debug, PartialEq)]
pub enum Frame {
    Heartbeat,
    Error(String),
    Event { name: String, args: Vec<Value> },
    Other,
}

/// Parse a socket.io 0.9 text frame: `type:id:endpoint:data`.
pub fn parse_frame(text: &str) -> Frame {
    let mut parts = text.splitn(4, ':');
    let kind = parts.next().unwrap_or("");
    let _id = parts.next();
    let _endpoint = parts.next();
    let data = parts.next().unwrap_or("");
    match kind {
        "2" => Frame::Heartbeat,
        "7" => Frame::Error(data.to_string()),
        "5" => match serde_json::from_str::<Value>(data) {
            Ok(v) => Frame::Event {
                name: v["name"].as_str().unwrap_or("").to_string(),
                args: v["args"].as_array().cloned().unwrap_or_default(),
            },
            Err(_) => Frame::Other,
        },
        _ => Frame::Other,
    }
}

/// Flatten the `rootFolder` structure into paths with ids.
pub fn flatten_tree(join_response: &Value) -> Result<RemoteTree> {
    let project = &join_response["project"];
    let root = project["rootFolder"]
        .as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| Error::Overleaf("joinProjectResponse had no rootFolder".into()))?;
    let root_folder_id = root["_id"]
        .as_str()
        .ok_or_else(|| Error::Overleaf("root folder has no _id".into()))?
        .to_string();

    let mut entries = Vec::new();
    walk(root, "", &mut entries);
    Ok(RemoteTree {
        project_name: project["name"].as_str().map(str::to_string),
        root_folder_id,
        entries,
    })
}

fn walk(folder: &Value, prefix: &str, out: &mut Vec<RemoteEntry>) {
    let join = |name: &str| {
        if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        }
    };
    let mut push = |list: &str, kind: RemoteKind| {
        for item in folder[list].as_array().into_iter().flatten() {
            if let (Some(id), Some(name)) = (item["_id"].as_str(), item["name"].as_str()) {
                out.push(RemoteEntry {
                    path: join(name),
                    id: id.to_string(),
                    kind,
                });
            }
        }
    };
    push("docs", RemoteKind::Doc);
    push("fileRefs", RemoteKind::File);
    for sub in folder["folders"].as_array().into_iter().flatten() {
        if let (Some(id), Some(name)) = (sub["_id"].as_str(), sub["name"].as_str()) {
            let path = join(name);
            out.push(RemoteEntry {
                path: path.clone(),
                id: id.to_string(),
                kind: RemoteKind::Folder,
            });
            walk(sub, &path, out);
        }
    }
}

/// Open the project's realtime channel and read its file tree.
///
/// `host` is e.g. `www.overleaf.com`; `handshake_sid` comes from
/// `GET /socket.io/1/?projectId=…` (see `Client::project_tree`).
pub fn read_tree(
    host: &str,
    project_id: &str,
    handshake_sid: &str,
    cookie_header: &str,
    user_agent: &str,
) -> Result<RemoteTree> {
    let url = format!("wss://{host}/socket.io/1/websocket/{handshake_sid}?projectId={project_id}");
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|e| Error::Overleaf(format!("bad websocket url: {e}")))?;
    let headers = request.headers_mut();
    let header = |v: &str| {
        tungstenite::http::HeaderValue::from_str(v)
            .map_err(|_| Error::Overleaf("invalid header value".into()))
    };
    headers.insert("Cookie", header(cookie_header)?);
    headers.insert("Origin", header(&format!("https://{host}"))?);
    headers.insert("User-Agent", header(user_agent)?);

    let (mut socket, _) = tungstenite::connect(request)
        .map_err(|e| Error::Overleaf(format!("realtime connection failed: {e}")))?;
    set_read_timeout(&mut socket, Duration::from_secs(8));

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if Instant::now() > deadline {
            return Err(Error::Overleaf(
                "timed out waiting for the project file tree".into(),
            ));
        }
        let message = socket
            .read()
            .map_err(|e| Error::Overleaf(format!("realtime read failed: {e}")))?;
        let Message::Text(text) = message else {
            continue;
        };
        match parse_frame(text.as_str()) {
            Frame::Heartbeat => {
                let _ = socket.send(Message::text("2::"));
            }
            Frame::Error(detail) if detail.contains("unauthorized") => {
                return Err(Error::OverleafAuth);
            }
            Frame::Error(detail) => {
                return Err(Error::Overleaf(format!("realtime error: {detail}")));
            }
            Frame::Event { name, args } if name == "joinProjectResponse" => {
                let first = args
                    .first()
                    .ok_or_else(|| Error::Overleaf("empty joinProjectResponse".into()))?;
                let _ = socket.close(None);
                return flatten_tree(first);
            }
            Frame::Event { name, .. } if name == "connectionRejected" => {
                return Err(Error::Overleaf("project connection was rejected".into()));
            }
            _ => {}
        }
    }
}

fn set_read_timeout(
    socket: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>,
    timeout: Duration,
) {
    match socket.get_mut() {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(Some(timeout));
        }
        MaybeTlsStream::NativeTls(s) => {
            let _ = s.get_ref().set_read_timeout(Some(timeout));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frames() {
        assert_eq!(parse_frame("2::"), Frame::Heartbeat);
        assert!(matches!(parse_frame("7:::unauthorized"), Frame::Error(d) if d == "unauthorized"));
        let ev = parse_frame(r#"5:::{"name":"joinProjectResponse","args":[{"a":1}]}"#);
        match ev {
            Frame::Event { name, args } => {
                assert_eq!(name, "joinProjectResponse");
                assert_eq!(args[0]["a"], 1);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(parse_frame("1::"), Frame::Other);
    }

    #[test]
    fn event_data_may_contain_colons() {
        let ev = parse_frame(r#"5:::{"name":"x","args":["a:b:c"]}"#);
        assert!(matches!(ev, Frame::Event { args, .. } if args[0] == "a:b:c"));
    }

    #[test]
    fn flattens_nested_folders() {
        let v: Value = serde_json::json!({
            "project": {
                "name": "Report",
                "rootFolder": [{
                    "_id": "root", "name": "rootFolder",
                    "docs": [{"_id": "d1", "name": "main.tex"}],
                    "fileRefs": [{"_id": "f1", "name": "logo.jpg"}],
                    "folders": [{
                        "_id": "c", "name": "chapters",
                        "docs": [{"_id": "d2", "name": "one.tex"}],
                        "fileRefs": [], "folders": []
                    }]
                }]
            }
        });
        let tree = flatten_tree(&v).unwrap();
        assert_eq!(tree.root_folder_id, "root");
        assert_eq!(tree.project_name.as_deref(), Some("Report"));
        let paths: Vec<(&str, RemoteKind)> =
            tree.entries.iter().map(|e| (e.path.as_str(), e.kind)).collect();
        assert_eq!(
            paths,
            vec![
                ("main.tex", RemoteKind::Doc),
                ("logo.jpg", RemoteKind::File),
                ("chapters", RemoteKind::Folder),
                ("chapters/one.tex", RemoteKind::Doc),
            ]
        );
    }

    #[test]
    fn missing_root_folder_is_an_error() {
        assert!(flatten_tree(&serde_json::json!({"project": {}})).is_err());
    }
}
