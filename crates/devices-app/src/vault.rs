//! OS secret vault (Windows Credential Manager via `keyring`).
//!
//! Joe Schmoe rule: passwords must survive restarts WITHOUT living in
//! files. `ui.json` holds config; this vault holds the 8 secrets. Reads
//! fail soft (session-only mode + a note) when the vault is unavailable
//! (headless CI, locked keychains) — never a crash, never a log line.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

pub const SERVICE: &str = "rust-devices-mcp";

/// Secret slots. Names are stable — renaming orphans stored credentials.
/// Add a slot here only with a UI field that reads/writes it (YAGNI).
pub const TAPO_PASSWORD: &str = "tapo-password";
pub const CAMERA_PASSWORD: &str = "camera-password";
pub const RING_PASSWORD: &str = "ring-password";
pub const LLM_API_KEY: &str = "llm-api-key";

pub fn get(slot: &str) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(SERVICE, slot).map_err(|e| format!("vault open: {e}"))?;
    match entry.get_password() {
        Ok(secret) if !secret.trim().is_empty() => Ok(Some(secret)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("vault read: {e}")),
    }
}

pub fn set(slot: &str, secret: &str) -> Result<(), String> {
    if secret.trim().is_empty() {
        return delete(slot);
    }
    let entry = keyring::Entry::new(SERVICE, slot).map_err(|e| format!("vault open: {e}"))?;
    entry
        .set_password(secret)
        .map_err(|e| format!("vault write: {e}"))
}

pub fn delete(slot: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, slot).map_err(|e| format!("vault open: {e}"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("vault delete: {e}")),
    }
}
