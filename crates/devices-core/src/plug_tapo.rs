//! Tapo plugs (P100/P105/P110/P110M/P115, strips) via the `tapo` crate.
//!
//! Auth is a Tapo cloud account (email + password) resolved by
//! [`Credentials`]. Model split: P115/P110 carry energy monitoring, P100/P105
//! don't — the handshake tries P115 first, then P110, then plain-plug P100.
use crate::creds::{with_timeout, with_timeout_secs, Credentials};
use crate::error::{require_device_id, DeviceError};
use crate::plug::PowerStats;

fn map_tapo(context: String) -> DeviceError {
    crate::creds::classify_vendor_error(context)
}

async fn handshake(
    ip: &str,
    user: &str,
    pass: &str,
) -> Result<tapo::PlugEnergyMonitoringHandler, DeviceError> {
    let attempt_p115 = with_timeout_secs("tapo.handshake(p115)", 15, async {
        tapo::ApiClient::new(user, pass).p115(ip).await
    })
    .await?;
    match attempt_p115 {
        Ok(device) => Ok(device),
        Err(first) => {
            let attempt_p110 = with_timeout_secs("tapo.handshake(p110)", 15, async {
                tapo::ApiClient::new(user, pass).p110(ip).await
            })
            .await?;
            attempt_p110.map_err(|second| map_tapo(format!("p115: {first}; p110: {second}")))
        }
    }
}

pub async fn get_stats(device_ip: &str, creds: &Credentials) -> Result<PowerStats, DeviceError> {
    let id = require_device_id(device_ip)?.to_string();
    let (user, pass) = creds.require_tapo_account()?;
    let device = handshake(&id, &user, &pass).await?;

    let info = with_timeout("tapo.device_info", device.get_device_info())
        .await?
        .map_err(|e| map_tapo(format!("device_info: {e}")))?;
    let usage = with_timeout("tapo.energy_usage", device.get_energy_usage())
        .await?
        .map_err(|e| map_tapo(format!("energy_usage: {e}")))?;

    let watts = match usage.current_power {
        Some(w) => w as f32,
        None => {
            let current = with_timeout("tapo.current_power", device.get_current_power())
                .await?
                .map_err(|e| map_tapo(format!("current_power: {e}")))?;
            current.current_power as f32
        }
    };
    Ok(PowerStats {
        device_ip: id,
        vendor: "tapo".to_string(),
        is_on: info.device_on,
        current_power_watts: watts,
        today_kwh: usage.today_energy as f32 / 1000.0,
    })
}

pub async fn toggle(device_ip: &str, state: bool, creds: &Credentials) -> Result<(), DeviceError> {
    let id = require_device_id(device_ip)?.to_string();
    let (user, pass) = creds.require_tapo_account()?;
    let device = handshake(&id, &user, &pass).await?;
    let op = if state { "tapo.on" } else { "tapo.off" };
    let result = with_timeout(op, async {
        if state {
            device.on().await
        } else {
            device.off().await
        }
    })
    .await?;
    result.map_err(|e| map_tapo(format!("{op}: {e}")))
}
