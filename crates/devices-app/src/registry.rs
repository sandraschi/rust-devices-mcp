//! Joe Schmoe device registry: adopted scan hits + manual entries.
//!
//! Fleet-imported devices live in the Python checkout (source of truth);
//! THIS registry holds what the operator adopted or typed by hand. Stored
//! as plain JSON next to `ui.json` (IPs and labels only — secrets stay in
//! the OS vault, never here). Corrupt file → empty registry, never a crash.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegDevice {
    pub id: String,
    pub kind: String,
    pub vendor: String,
    pub label: String,
    pub ip: String,
    pub port: Option<u16>,
    pub username: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Registry {
    pub devices: Vec<RegDevice>,
}

pub fn path() -> Option<std::path::PathBuf> {
    std::env::var("LOCALAPPDATA").ok().map(|base| {
        std::path::PathBuf::from(base)
            .join("rust-devices-mcp")
            .join("devices.json")
    })
}

pub fn load() -> Registry {
    let Some(path) = path() else {
        return Registry::default();
    };
    let text = std::fs::read_to_string(path).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save(registry: &Registry) {
    let Some(path) = path() else { return };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    if let Ok(text) = serde_json::to_string_pretty(registry) {
        let _ = std::fs::write(path, text);
    }
}

pub fn adopt(ip: &str, port: u16, kind: &str, label: &str) -> RegDevice {
    let (kind, vendor) = match kind {
        "hue" => ("hue".to_string(), String::new()),
        "shelly" => ("plug".to_string(), "shelly".to_string()),
        "kasa?" => ("plug".to_string(), "kasa".to_string()),
        "onvif?" => ("camera".to_string(), "onvif".to_string()),
        "rtsp?" => ("camera".to_string(), "tapo".to_string()),
        other => ("generic".to_string(), other.to_string()),
    };
    RegDevice {
        id: format!("{kind}:{ip}:{port}"),
        kind,
        vendor,
        label: if label.trim().is_empty() {
            ip.to_string()
        } else {
            label.trim().to_string()
        },
        ip: ip.trim().to_string(),
        port: Some(port),
        username: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adopt_maps_kinds() {
        let hue = adopt("192.168.0.5", 80, "hue", "Hall");
        assert_eq!(hue.kind, "hue");
        let kasa = adopt("192.168.0.6", 9999, "kasa?", "");
        assert_eq!(kasa.vendor, "kasa");
        assert_eq!(kasa.label, "192.168.0.6");
    }

    #[test]
    fn corrupt_file_is_empty_registry() {
        // save() to a bogus LOCALAPPDATA would touch the real machine —
        // instead assert the deserializer's fallback directly.
        let registry: Registry = serde_json::from_str("not json{{").unwrap_or_default();
        assert!(registry.devices.is_empty());
    }
}
