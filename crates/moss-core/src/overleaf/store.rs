//! Keeping the Overleaf session in Windows Credential Manager (never in a file or in Moss's own
//! database, and never the password: only the session cookie).

use crate::overleaf::Session;
use crate::{Error, Result};

const SERVICE: &str = "Moss";

/// Credential Manager blobs are limited to 2560 bytes and the Windows store keeps them as UTF-16,
/// so keep the serialized session comfortably below ~1200 characters.
const MAX_CHARS: usize = 1200;

fn entry(host: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, &format!("overleaf:{host}"))
        .map_err(|e| Error::Overleaf(format!("credential store unavailable: {e}")))
}

pub fn save(session: &Session) -> Result<()> {
    let mut slim = session.essential();
    let mut json = serde_json::to_string(&slim).map_err(|e| Error::json("serializing session", e))?;
    if json.chars().count() > MAX_CHARS {
        // Fall back to just the session cookie itself.
        slim.cookies.retain(|c| c.name.starts_with("overleaf_session"));
        json = serde_json::to_string(&slim).map_err(|e| Error::json("serializing session", e))?;
    }
    entry(&session.host)?
        .set_password(&json)
        .map_err(|e| Error::Overleaf(format!("could not save the session: {e}")))
}

pub fn load(host: &str) -> Result<Option<Session>> {
    match entry(host)?.get_password() {
        Ok(json) => serde_json::from_str(&json)
            .map(Some)
            .map_err(|e| Error::json("reading the saved session", e)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(Error::Overleaf(format!("could not read the saved session: {e}"))),
    }
}

pub fn clear(host: &str) -> Result<()> {
    match entry(host)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(Error::Overleaf(format!("could not remove the saved session: {e}"))),
    }
}
