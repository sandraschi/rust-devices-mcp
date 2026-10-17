//! ONVIF snapshots: profiles → snapshot URI → JPEG fetch.
//!
//! `oxvif` handles SOAP/WS-Security; the JPEG itself rides plain HTTP(S)
//! with basic auth. If a camera insists on digest auth for the image URL,
//! the fetch fails typed (not silent) — that gap is documented, not hidden.
use crate::creds::{classify_vendor_error, with_timeout_secs};
use crate::error::DeviceError;
use std::time::Duration;

pub async fn grab(
    ip: &str,
    port: u16,
    username: &str,
    password: &str,
) -> Result<Vec<u8>, DeviceError> {
    let base = format!("http://{ip}:{port}/onvif/device_service");
    let session = with_timeout_secs("onvif.session", 20, async {
        oxvif::OnvifSession::builder(&base)
            .with_credentials(username, password)
            .with_clock_sync()
            .build()
            .await
    })
    .await?
    .map_err(|e| classify_vendor_error(format!("onvif session: {e}")))?;

    let profiles = with_timeout_secs("onvif.profiles", 20, session.get_profiles())
        .await?
        .map_err(|e| classify_vendor_error(format!("onvif profiles: {e}")))?;
    let token = profiles
        .first()
        .map(|p| p.token.clone())
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| {
            DeviceError::Unsupported("onvif: camera returned no media profiles".to_string())
        })?;

    let snap = with_timeout_secs("onvif.snapshot_uri", 20, session.get_snapshot_uri(&token))
        .await?
        .map_err(|e| classify_vendor_error(format!("onvif snapshot_uri: {e}")))?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| DeviceError::Protocol(format!("snapshot http client: {e}")))?;
    let resp = with_timeout_secs(
        "onvif.snapshot_fetch",
        20,
        http.get(&snap.uri)
            .basic_auth(username, Some(password))
            .send(),
    )
    .await?
    .map_err(|e| classify_vendor_error(format!("onvif snapshot fetch: {e}")))?;
    if !resp.status().is_success() {
        return Err(DeviceError::Protocol(format!(
            "onvif snapshot fetch HTTP {} (digest-only image URL? re-check with --source tapo)",
            resp.status()
        )));
    }
    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| DeviceError::Protocol(format!("onvif snapshot body: {e}")))
}
