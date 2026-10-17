//! Tapo camera snapshots (C210/C220/C225/C325WB/C520WS) via the `tapo` crate.
//!
//! Two credentials cooperate: the Tapo cloud account (handshake) and the
//! camera-local user (snapshot). Model auto-tries C220 → C210 → C225 →
//! C325WB → C520WS unless `--model` pins one; every attempt is a fresh
//! `ApiClient` (cheap, no I/O in the constructor).
use crate::creds::{classify_vendor_error, with_timeout_secs};
use crate::error::DeviceError;

const MODELS: &[&str] = &["c220", "c210", "c225", "c325wb", "c520ws"];

async fn try_model(
    model: &str,
    ip: &str,
    account: &(String, String),
    cam_user: &str,
    cam_pass: &str,
) -> Result<Vec<u8>, DeviceError> {
    let map = |e: tapo::Error| classify_vendor_error(format!("tapo cam {model}: {e}"));
    let client = tapo::ApiClient::new(account.0.clone(), account.1.clone());
    let snapshot = match model {
        "c210" => {
            let cam = with_timeout_secs("tapo.cam.handshake", 20, client.c210(ip))
                .await?
                .map_err(map)?;
            with_timeout_secs(
                "tapo.cam.snapshot",
                25,
                cam.get_snapshot(cam_user, cam_pass),
            )
            .await?
            .map_err(map)?
        }
        "c220" => {
            let cam = with_timeout_secs("tapo.cam.handshake", 20, client.c220(ip))
                .await?
                .map_err(map)?;
            with_timeout_secs(
                "tapo.cam.snapshot",
                25,
                cam.get_snapshot(cam_user, cam_pass),
            )
            .await?
            .map_err(map)?
        }
        "c225" => {
            let cam = with_timeout_secs("tapo.cam.handshake", 20, client.c225(ip))
                .await?
                .map_err(map)?;
            with_timeout_secs(
                "tapo.cam.snapshot",
                25,
                cam.get_snapshot(cam_user, cam_pass),
            )
            .await?
            .map_err(map)?
        }
        "c325wb" => {
            let cam = with_timeout_secs("tapo.cam.handshake", 20, client.c325wb(ip))
                .await?
                .map_err(map)?;
            with_timeout_secs(
                "tapo.cam.snapshot",
                25,
                cam.get_snapshot(cam_user, cam_pass),
            )
            .await?
            .map_err(map)?
        }
        "c520ws" => {
            let cam = with_timeout_secs("tapo.cam.handshake", 20, client.c520ws(ip))
                .await?
                .map_err(map)?;
            with_timeout_secs(
                "tapo.cam.snapshot",
                25,
                cam.get_snapshot(cam_user, cam_pass),
            )
            .await?
            .map_err(map)?
        }
        _ => {
            return Err(DeviceError::Unsupported(format!(
                "tapo camera model '{model}': expected c210|c220|c225|c325wb|c520ws"
            )));
        }
    };
    Ok(snapshot.data)
}

pub async fn grab(
    ip: &str,
    model: Option<String>,
    account: &(String, String),
    cam_user: &str,
    cam_pass: &str,
) -> Result<Vec<u8>, DeviceError> {
    if let Some(pinned) = model {
        let name = pinned.trim().to_ascii_lowercase();
        return try_model(&name, ip, account, cam_user, cam_pass).await;
    }
    let mut last_err: Option<DeviceError> = None;
    for candidate in MODELS {
        match try_model(candidate, ip, account, cam_user, cam_pass).await {
            Ok(bytes) => return Ok(bytes),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err
        .unwrap_or_else(|| DeviceError::Protocol("tapo cam: no models attempted".to_string())))
}
