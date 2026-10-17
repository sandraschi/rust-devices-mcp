//! Shelly plugs and switches (Plus Plug S, Plug S Gen3, Pro PM…) via
//! `shelly-core`: Gen1 `/status` + Gen2/Gen3 RPC behind one probe.
//! No account needed on the LAN; optional device password (`admin` user).
use crate::creds::{with_timeout, with_timeout_secs, Credentials};
use crate::error::{require_device_id, DeviceError};
use crate::plug::PowerStats;
use std::time::Duration;

fn map_shelly(context: String) -> DeviceError {
    let lower = context.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        DeviceError::Timeout(context)
    } else if lower.contains("401")
        || lower.contains("403")
        || lower.contains("unauthorized")
        || lower.contains("auth")
    {
        DeviceError::Auth(context)
    } else if lower.contains("unsupported") {
        DeviceError::Unsupported(context)
    } else {
        DeviceError::Protocol(context)
    }
}

fn http_client() -> Result<reqwest::Client, DeviceError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| DeviceError::Protocol(format!("shelly http client: {e}")))
}

async fn device(ip: &str, creds: &Credentials) -> Result<shelly_core::ShellyDevice, DeviceError> {
    let http = http_client()?;
    let info = with_timeout_secs("shelly.probe", 8, shelly_core::probe_target(ip, &http))
        .await?
        .map_err(|e| map_shelly(format!("probe {ip}: {e}")))?;
    Ok(shelly_core::create_device_with_host(
        info,
        ip.to_string(),
        http,
        creds.password.clone(),
    ))
}

pub async fn get_stats(device_ip: &str, creds: &Credentials) -> Result<PowerStats, DeviceError> {
    let id = require_device_id(device_ip)?.to_string();
    let dev = device(&id, creds).await?;
    let reading = with_timeout("shelly.power", dev.power(0))
        .await?
        .map_err(|e| map_shelly(format!("power: {e}")))?;
    let switch = with_timeout("shelly.switch_status", dev.switch_status(0))
        .await?
        .map_err(|e| map_shelly(format!("switch_status: {e}")))?;
    Ok(PowerStats {
        device_ip: id,
        vendor: "shelly".to_string(),
        is_on: switch.output,
        current_power_watts: reading.power_watts as f32,
        today_kwh: (reading.total_energy_wh / 1000.0) as f32,
    })
}

pub async fn toggle(device_ip: &str, state: bool, creds: &Credentials) -> Result<(), DeviceError> {
    let id = require_device_id(device_ip)?.to_string();
    let dev = device(&id, creds).await?;
    with_timeout("shelly.switch_set", dev.switch_set(0, state))
        .await?
        .map_err(|e| map_shelly(format!("switch_set: {e}")))?;
    Ok(())
}
