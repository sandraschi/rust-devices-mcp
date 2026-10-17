//! Camera controls across sources: Tapo, ONVIF, USB, Ring, public URLs.
//!
//! Snapshots are real JPEG frames, never stubs: every source module returns
//! raw bytes, this module verifies the SOI magic, parses dimensions from the
//! SOF marker (pure std, hostile-safe), and optionally writes the file.
//! MCP never carries blobs — `camera.snapshot` returns metadata (+ the path
//! when `--out` was given) while the bytes stay on the server host.
use crate::error::{require_device_id, DeviceError};

use serde::{Deserialize, Serialize};

/// Snapshot sources. `Demo` keeps the deterministic oracle for tests/probes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CamSource {
    Tapo,
    Onvif,
    Usb,
    Ring,
    Url,
    Demo,
}

impl CamSource {
    pub fn parse(raw: &str) -> Result<Self, DeviceError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "tapo" => Ok(Self::Tapo),
            "onvif" => Ok(Self::Onvif),
            "usb" | "webcam" => Ok(Self::Usb),
            "ring" | "doorbell" => Ok(Self::Ring),
            "url" | "http" | "public" => Ok(Self::Url),
            "demo" | "stub" | "mock" => Ok(Self::Demo),
            _ => Err(DeviceError::InvalidInput(format!(
                "unknown camera source '{raw}': expected tapo|onvif|usb|ring|url|demo"
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tapo => "tapo",
            Self::Onvif => "onvif",
            Self::Usb => "usb",
            Self::Ring => "ring",
            Self::Url => "url",
            Self::Demo => "demo",
        }
    }
}

/// Static description (shape info, not telemetry — still a stub by design).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraInfo {
    pub device_ip: String,
    pub model: String,
    pub rtsp_url: String,
    pub reachable: bool,
}

/// Real snapshot metadata. `path` is `Some` only when the frame was written
/// to disk (`--out`); bytes/dimensions always describe the grabbed frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub source: String,
    pub bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub path: Option<String>,
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
}

/// Camera login material: explicit args → CAMERA_USERNAME/CAMERA_PASSWORD →
/// TAPO_CAMERA_USERNAME/TAPO_CAMERA_PASSWORD. Never logged.
pub fn camera_creds(
    username: Option<String>,
    password: Option<String>,
) -> (Option<String>, Option<String>) {
    let clean = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let user = clean(username)
        .or_else(|| env_non_empty("CAMERA_USERNAME"))
        .or_else(|| env_non_empty("TAPO_CAMERA_USERNAME"));
    let pass = clean(password)
        .or_else(|| env_non_empty("CAMERA_PASSWORD"))
        .or_else(|| env_non_empty("TAPO_CAMERA_PASSWORD"));
    (user, pass)
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// JPEG SOI magic — every source must produce real JPEG bytes.
pub fn require_jpeg(source: &str, bytes: &[u8]) -> Result<(), DeviceError> {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        Ok(())
    } else {
        Err(DeviceError::Protocol(format!(
            "{source}: reply is not a JPEG ({} bytes, bad magic)",
            bytes.len()
        )))
    }
}

/// Parse JPEG dimensions from the first SOF0/1/2 marker. Pure std, bounds
/// checked at every step, iteration capped — hostile bytes can't panic it.
pub fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut pos = 2usize;
    for _ in 0..64 {
        let marker_0 = *bytes.get(pos)?;
        let marker_1 = *bytes.get(pos.checked_add(1)?)?;
        if marker_0 != 0xFF {
            return None;
        }
        // Standalone markers carry no length.
        if marker_1 == 0xD8
            || marker_1 == 0xD9
            || (0xD0..=0xD7).contains(&marker_1)
            || marker_1 == 0x01
        {
            pos = pos.checked_add(2)?;
            continue;
        }
        let len = u16::from_be_bytes([
            *bytes.get(pos.checked_add(2)?)?,
            *bytes.get(pos.checked_add(3)?)?,
        ]) as usize;
        if len < 2 {
            return None;
        }
        // SOF0/1/2 (not DHT/C4, JPG/C8, DAC/CC): height/width follow precision byte.
        if marker_1 == 0xC0 || marker_1 == 0xC1 || marker_1 == 0xC2 {
            let h = u16::from_be_bytes([
                *bytes.get(pos.checked_add(5)?)?,
                *bytes.get(pos.checked_add(6)?)?,
            ]) as u32;
            let w = u16::from_be_bytes([
                *bytes.get(pos.checked_add(7)?)?,
                *bytes.get(pos.checked_add(8)?)?,
            ]) as u32;
            if w > 0 && h > 0 {
                return Some((w, h));
            }
            return None;
        }
        // Segment length INCLUDES its own 2 length bytes: next marker sits
        // at pos + 2 (marker) + len. Getting this wrong skips into payload
        // (found live 2026-10-17: APP0 shifted us by 2 and blinded SOF0).
        pos = pos.checked_add(2)?.checked_add(len)?;
    }
    None
}

/// Write the frame; parents are created, failures are typed.
pub fn save_snapshot(bytes: &[u8], out_path: &str) -> Result<String, DeviceError> {
    let path = out_path.trim();
    if path.is_empty() {
        return Err(DeviceError::InvalidInput(
            "output path must not be empty".to_string(),
        ));
    }
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DeviceError::Protocol(format!("snapshot: cannot create {parent:?}: {e}"))
            })?;
        }
    }
    std::fs::write(path, bytes)
        .map_err(|e| DeviceError::Protocol(format!("snapshot: cannot write {path}: {e}")))?;
    Ok(path.to_string())
}

/// A grabbed frame: metadata for agents, bytes for presenters (base64 in
/// the Tauri layer, files on disk — never blobs over MCP).
pub struct Frame {
    pub meta: Snapshot,
    pub jpeg: Vec<u8>,
}

#[allow(clippy::too_many_arguments)]
pub async fn snapshot_full(
    source: CamSource,
    device_ip: &str,
    model: Option<String>,
    username: Option<String>,
    password: Option<String>,
    onvif_port: Option<u16>,
    device_index: Option<u32>,
    url: Option<String>,
    out_path: Option<String>,
    ring_email: Option<String>,
    ring_token_file: Option<String>,
) -> Result<Frame, DeviceError> {
    let bytes = match source {
        CamSource::Demo => {
            // Deterministic minimal JPEG (SOI + SOF0 4x2 + EOI) so probes and
            // tests exercise the real parse/write path without hardware.
            demo_jpeg()
        }
        CamSource::Tapo => {
            let id = require_device_id(device_ip)?.to_string();
            let (user, pass) = camera_creds(username, password);
            let (tapo_user, tapo_pass) = (
                user.ok_or_else(|| {
                    DeviceError::Auth(
                        "Tapo camera login required: --username or CAMERA_USERNAME".to_string(),
                    )
                })?,
                pass.ok_or_else(|| {
                    DeviceError::Auth(
                        "Tapo camera password required: --password or CAMERA_PASSWORD".to_string(),
                    )
                })?,
            );
            let (account_user, account_pass) = (
                env_non_empty("TAPO_EMAIL").or_else(|| env_non_empty("TAPO_USERNAME")),
                env_non_empty("TAPO_PASSWORD"),
            );
            let (account_user, account_pass) = match (account_user, account_pass) {
                (Some(u), Some(p)) => (u, p),
                _ => {
                    return Err(DeviceError::Auth(
                        "Tapo cloud account required for camera handshake: set TAPO_EMAIL + TAPO_PASSWORD".to_string(),
                    ));
                }
            };
            super::cam_tapo::grab(
                &id,
                model,
                &(account_user, account_pass),
                &tapo_user,
                &tapo_pass,
            )
            .await?
        }
        CamSource::Onvif => {
            let id = require_device_id(device_ip)?.to_string();
            let (user, pass) = camera_creds(username, password);
            let (user, pass) = match (user, pass) {
                (Some(u), Some(p)) => (u, p),
                _ => {
                    return Err(DeviceError::Auth(
                        "ONVIF login required: --username/--password or CAMERA_USERNAME/CAMERA_PASSWORD".to_string(),
                    ));
                }
            };
            let port = onvif_port.unwrap_or(2020);
            super::cam_onvif::grab(&id, port, &user, &pass).await?
        }
        CamSource::Usb => super::cam_usb::grab(device_index.unwrap_or(0)).await?,
        CamSource::Ring => {
            let raw = device_ip.trim();
            if raw.is_empty() {
                return Err(DeviceError::InvalidInput(
                    "Ring doorbell id required in device_ip (see ring.doorbells)".to_string(),
                ));
            }
            let doorbell_id: i64 = raw.parse().map_err(|_| {
                DeviceError::InvalidInput(format!("Ring doorbell id must be numeric, got '{raw}'"))
            })?;
            // Ring login material rides username/password (+ token file env).
            super::ring::snapshot_jpeg(
                doorbell_id,
                username.or(ring_email),
                password,
                ring_token_file,
            )
            .await?
        }
        CamSource::Url => {
            let target = url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    DeviceError::InvalidInput("snapshot source url: --url required".to_string())
                })?;
            super::cam_url::grab(target).await?
        }
    };
    require_jpeg(source.as_str(), &bytes)?;
    let (width, height) = jpeg_dimensions(&bytes)
        .map(|(w, h)| (Some(w), Some(h)))
        .unwrap_or((None, None));
    let path = match out_path {
        Some(p) => Some(save_snapshot(&bytes, &p)?),
        None => None,
    };
    Ok(Frame {
        meta: Snapshot {
            source: source.as_str().to_string(),
            bytes: bytes.len() as u64,
            width,
            height,
            path,
        },
        jpeg: bytes,
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn snapshot(
    source: CamSource,
    device_ip: &str,
    model: Option<String>,
    username: Option<String>,
    password: Option<String>,
    onvif_port: Option<u16>,
    device_index: Option<u32>,
    url: Option<String>,
    out_path: Option<String>,
    ring_email: Option<String>,
    ring_token_file: Option<String>,
) -> Result<Snapshot, DeviceError> {
    snapshot_full(
        source,
        device_ip,
        model,
        username,
        password,
        onvif_port,
        device_index,
        url,
        out_path,
        ring_email,
        ring_token_file,
    )
    .await
    .map(|frame| frame.meta)
}

/// Minimal valid JPEG: SOI, SOF0 (4x2), EOI. Exercises parse + write paths.
fn demo_jpeg() -> Vec<u8> {
    vec![
        0xFF, 0xD8, // SOI
        0xFF, 0xC0, 0x00, 0x0B, 0x08, // SOF0, len 11, precision 8
        0x00, 0x02, // height 2
        0x00, 0x04, // width 4
        0x01, 0x01, 0x11, 0x00, // components
        0xFF, 0xD9, // EOI
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_parse_accepts_aliases() {
        assert_eq!(CamSource::parse("USB"), Ok(CamSource::Usb));
        assert_eq!(CamSource::parse("webcam"), Ok(CamSource::Usb));
        assert_eq!(CamSource::parse("doorbell"), Ok(CamSource::Ring));
        assert!(CamSource::parse("vhs").is_err());
    }

    #[test]
    fn jpeg_dimensions_reads_demo_frame() {
        assert_eq!(jpeg_dimensions(&demo_jpeg()), Some((4, 2)));
    }

    #[test]
    fn jpeg_dimensions_skips_app0_to_sof() {
        // Real-world shape (from a live USB frame): SOI, APP0 len 16,
        // SOF0 640x480, EOI. Regression: segment length includes its own
        // 2 length bytes — pos += len alone lands mid-payload.
        let mut frame = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        frame.extend([0u8; 14]);
        frame.extend([0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0xE0, 0x02, 0x80]);
        frame.extend([0xFF, 0xD9]);
        assert_eq!(jpeg_dimensions(&frame), Some((640, 480)));
    }

    #[test]
    fn jpeg_magic_rejects_garbage() {
        assert!(require_jpeg("test", b"not a jpeg").is_err());
        assert!(require_jpeg("test", &demo_jpeg()).is_ok());
    }

    #[tokio::test]
    async fn demo_snapshot_round_trips() {
        let snap = snapshot(
            CamSource::Demo,
            "ignored",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap_or(Snapshot {
            source: "demo".to_string(),
            bytes: 0,
            width: None,
            height: None,
            path: None,
        });
        assert_eq!(snap.width, Some(4));
        assert_eq!(snap.height, Some(2));
        assert!(snap.path.is_none());
    }
}
