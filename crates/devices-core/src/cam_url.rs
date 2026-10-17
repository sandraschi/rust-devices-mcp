//! Public-webcam snapshots: plain HTTPS GET, JPEG magic decides.
use crate::creds::{classify_vendor_error, with_timeout_secs};
use crate::error::DeviceError;
use std::time::Duration;

pub async fn grab(url: &str) -> Result<Vec<u8>, DeviceError> {
    let target = url.trim();
    if !(target.starts_with("http://") || target.starts_with("https://")) {
        return Err(DeviceError::InvalidInput(format!(
            "snapshot url must be http(s): '{target}'"
        )));
    }
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| DeviceError::Protocol(format!("snapshot http client: {e}")))?;
    let resp = with_timeout_secs("cam_url.get", 20, http.get(target).send())
        .await?
        .map_err(|e| classify_vendor_error(format!("cam_url.get: {e}")))?;
    if !resp.status().is_success() {
        return Err(DeviceError::Protocol(format!(
            "cam_url.get HTTP {}",
            resp.status()
        )));
    }
    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| DeviceError::Protocol(format!("cam_url.get body: {e}")))
}
