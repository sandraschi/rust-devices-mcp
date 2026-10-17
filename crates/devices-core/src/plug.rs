//! Smart plug power protocols across vendors.
//!
//! `PlugController` is the single door: every caller names a [`Vendor`] and
//! the dispatch lands in exactly one vendor module. `Demo` is the
//! deterministic oracle (42.5 W / 0.85 kWh) for tests and probes —
//! explicitly not hardware, explicitly named, never mistaken for telemetry.
use crate::creds::{Credentials, Vendor};
use crate::error::{require_device_id, DeviceError};
use serde::{Deserialize, Serialize};

/// Snapshot returned by plug stats. `vendor` records which driver produced
/// the numbers so agents can tell demo values from hardware truth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerStats {
    pub device_ip: String,
    pub vendor: String,
    pub is_on: bool,
    pub current_power_watts: f32,
    pub today_kwh: f32,
}

#[derive(Debug, Default, Clone)]
pub struct PlugController;

impl PlugController {
    pub fn new() -> Self {
        Self
    }

    pub async fn get_stats(
        &self,
        vendor: Vendor,
        device_ip: &str,
        creds: &Credentials,
    ) -> Result<PowerStats, DeviceError> {
        match vendor {
            Vendor::Demo => {
                let id = require_device_id(device_ip)?;
                Ok(PowerStats {
                    device_ip: id.to_string(),
                    vendor: vendor.as_str().to_string(),
                    is_on: true,
                    current_power_watts: 42.5,
                    today_kwh: 0.85,
                })
            }
            Vendor::Tapo => super::plug_tapo::get_stats(device_ip, creds).await,
            Vendor::Kasa => super::plug_kasa::get_stats(device_ip, creds).await,
            Vendor::Shelly => super::plug_shelly::get_stats(device_ip, creds).await,
        }
    }

    pub async fn toggle(
        &self,
        vendor: Vendor,
        device_ip: &str,
        state: bool,
        creds: &Credentials,
    ) -> Result<(), DeviceError> {
        match vendor {
            Vendor::Demo => {
                let _ = require_device_id(device_ip)?;
                let _ = state;
                Ok(())
            }
            Vendor::Tapo => super::plug_tapo::toggle(device_ip, state, creds).await,
            Vendor::Kasa => super::plug_kasa::toggle(device_ip, state, creds).await,
            Vendor::Shelly => super::plug_shelly::toggle(device_ip, state, creds).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_creds() -> Credentials {
        Credentials::default()
    }

    #[tokio::test]
    async fn demo_stats_round_trip_through_json() {
        let ctl = PlugController::new();
        let stats = ctl
            .get_stats(Vendor::Demo, "192.168.1.50", &demo_creds())
            .await
            .unwrap_or(PowerStats {
                device_ip: String::new(),
                vendor: String::new(),
                is_on: false,
                current_power_watts: 0.0,
                today_kwh: 0.0,
            });
        let json = serde_json::to_string(&stats).unwrap_or_default();
        let back: PowerStats = serde_json::from_str(&json).unwrap_or(PowerStats {
            device_ip: String::new(),
            vendor: String::new(),
            is_on: false,
            current_power_watts: 0.0,
            today_kwh: 0.0,
        });
        assert_eq!(stats, back);
        assert_eq!(back.vendor, "demo");
        assert!((back.current_power_watts - 42.5).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn toggle_rejects_empty_ip() {
        let ctl = PlugController::new();
        let result = ctl.toggle(Vendor::Demo, "", true, &demo_creds()).await;
        assert!(matches!(result, Err(DeviceError::InvalidInput(_))));
    }

    #[tokio::test]
    async fn tapo_without_creds_fails_typed_not_panic() {
        let ctl = PlugController::new();
        let result = ctl
            .get_stats(Vendor::Tapo, "192.168.1.50", &demo_creds())
            .await;
        // Either a typed Auth error (no creds anywhere) or hardware truth if
        // the operator exported TAPO_EMAIL/TAPO_PASSWORD — both are fine.
        if let Err(e) = result {
            assert!(matches!(
                e,
                DeviceError::Auth(_) | DeviceError::Timeout(_) | DeviceError::Protocol(_)
            ));
        }
    }
}
