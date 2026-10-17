//! TP-Link Kasa plugs (HS100/HS110/EP25…) via `kasa-rs`.
//!
//! `connect_auto` speaks legacy XOR first, KLAP second — legacy devices need
//! no account at all. Energy reporting degrades honestly: non-metering plugs
//! (plain HS100) report relay state with 0.0 readings rather than failing,
//! because a reachable on/off plug is still worth toggling.
use crate::creds::{with_timeout, with_timeout_secs, Credentials};
use crate::error::{require_device_id, DeviceError};
use crate::plug::PowerStats;
use serde_json::{json, Value};

fn map_kasa(context: String) -> DeviceError {
    let lower = context.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        DeviceError::Timeout(context)
    } else if lower.contains("auth")
        || lower.contains("credential")
        || lower.contains("login")
        || lower.contains("unauthorized")
    {
        DeviceError::Auth(context)
    } else {
        DeviceError::Protocol(context)
    }
}

async fn connect(ip: &str, creds: &Credentials) -> Result<kasa_rs::Transport, DeviceError> {
    let account = creds.kasa_opt();
    let conn = with_timeout_secs("kasa.connect", 8, async {
        kasa_rs::Transport::connect_auto(ip, account.as_ref()).await
    })
    .await?;
    conn.map_err(|e| map_kasa(format!("connect {ip}: {e:#}")))
}

pub async fn get_stats(device_ip: &str, creds: &Credentials) -> Result<PowerStats, DeviceError> {
    let id = require_device_id(device_ip)?.to_string();
    let mut transport = connect(&id, creds).await?;

    let sys_resp = with_timeout(
        "kasa.sysinfo",
        transport.send_json(&kasa_rs::sysinfo_query()),
    )
    .await?
    .map_err(|e| map_kasa(format!("sysinfo: {e:#}")))?;
    let sys = kasa_rs::sysinfo_from_response(&sys_resp)
        .map_err(|e| map_kasa(format!("sysinfo decode: {e:#}")))?;
    let is_on = sys.get("relay_state").and_then(|v| v.as_i64()).unwrap_or(0) == 1;

    // Metering plugs answer emeter; plain relays don't — degrade to 0.0.
    let realtime = with_timeout(
        "kasa.emeter_realtime",
        transport.send_json(&json!({"emeter": {"get_realtime": {}}})),
    )
    .await;
    let (watts, _total_wh) = match realtime {
        Ok(Ok(resp)) if kasa_rs::check_err_code(&resp).is_ok() => {
            let root = resp
                .pointer("/emeter/get_realtime")
                .cloned()
                .unwrap_or(Value::Null);
            let mw = root.get("power_mw").and_then(|v| v.as_f64());
            let watts = mw.map(|m| (m / 1000.0) as f32).unwrap_or_else(|| {
                root.get("power").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32
            });
            let total = root.get("total_wh").and_then(|v| v.as_f64()).unwrap_or(0.0);
            (watts, total)
        }
        _ => (0.0, 0.0),
    };

    // Today's energy from the daystat table (Wh → kWh), best effort.
    let today_kwh = match current_month_year() {
        Some((month, year)) => {
            let daystat = with_timeout(
                "kasa.emeter_daystat",
                transport
                    .send_json(&json!({"emeter": {"get_daystat": {"month": month, "year": year}}})),
            )
            .await;
            match daystat {
                Ok(Ok(resp)) if kasa_rs::check_err_code(&resp).is_ok() => resp
                    .pointer("/emeter/get_daystat/day_list")
                    .and_then(|v| v.as_array())
                    .and_then(|days| days.last())
                    .and_then(|d| d.get("energy_wh"))
                    .and_then(|v| v.as_f64())
                    .map(|wh| (wh / 1000.0) as f32)
                    .unwrap_or(0.0),
                _ => 0.0,
            }
        }
        None => 0.0,
    };

    Ok(PowerStats {
        device_ip: id,
        vendor: "kasa".to_string(),
        is_on,
        current_power_watts: watts,
        today_kwh,
    })
}

pub async fn toggle(device_ip: &str, state: bool, creds: &Credentials) -> Result<(), DeviceError> {
    let id = require_device_id(device_ip)?.to_string();
    let mut transport = connect(&id, creds).await?;
    let resp = with_timeout(
        "kasa.set_relay",
        transport.send_json(&json!({"system": {"set_relay_state": {"state": state as u8}}})),
    )
    .await?
    .map_err(|e| map_kasa(format!("set_relay: {e:#}")))?;
    kasa_rs::check_err_code(&resp).map_err(|e| map_kasa(format!("set_relay refused: {e:#}")))
}

fn current_month_year() -> Option<(u32, u32)> {
    let now = chrono::Local::now().date_naive();
    let month = now.month();
    let year = now.year();
    if (1..=12).contains(&month) && year > 2000 {
        Some((month, year as u32))
    } else {
        None
    }
}

use chrono::Datelike;
