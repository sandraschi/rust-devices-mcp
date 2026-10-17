//! Camera controls (RTSP describe / snapshot metadata).
//!
//! Same rule as `plug.rs`: deterministic stub behind the real surface so the
//! dispatch loop, MCP schemas, and Tauri commands prove out first.
use crate::error::{require_device_id, DeviceError};
use serde::{Deserialize, Serialize};

/// Static description returned by `camera.describe`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraInfo {
    pub device_ip: String,
    pub model: String,
    pub rtsp_url: String,
    pub reachable: bool,
}

/// Metadata returned by `camera.snapshot_meta` (no binary blobs over MCP).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub device_ip: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}

#[derive(Debug, Default, Clone)]
pub struct CameraController;

impl CameraController {
    pub fn new() -> Self {
        Self
    }

    pub fn describe(&self, device_ip: &str) -> Result<CameraInfo, DeviceError> {
        let id = require_device_id(device_ip)?;
        Ok(CameraInfo {
            device_ip: id.to_string(),
            model: "Tapo C200 (stub)".to_string(),
            rtsp_url: format!("rtsp://{id}/stream1"),
            reachable: true,
        })
    }

    pub fn snapshot_meta(&self, device_ip: &str) -> Result<SnapshotMeta, DeviceError> {
        let id = require_device_id(device_ip)?;
        // Stub 1280x720 JPEG size class; real frame grab is scorecard-tier-3.
        let width: u32 = 1280;
        let height: u32 = 720;
        let bytes: u64 = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|px| px.checked_mul(3))
            .unwrap_or(0);
        Ok(SnapshotMeta {
            device_ip: id.to_string(),
            width,
            height,
            bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_rejects_empty_ip() {
        let ctl = CameraController::new();
        let result = ctl.describe("");
        assert!(matches!(result, Err(DeviceError::InvalidInput(_))));
    }

    #[test]
    fn snapshot_meta_math_is_checked() {
        let ctl = CameraController::new();
        let meta = match ctl.snapshot_meta("192.168.1.51") {
            Ok(m) => m,
            Err(_) => SnapshotMeta {
                device_ip: String::new(),
                width: 0,
                height: 0,
                bytes: 0,
            },
        };
        assert_eq!(meta.width, 1280);
        assert_eq!(meta.bytes, 1280u64 * 720 * 3);
    }
}
