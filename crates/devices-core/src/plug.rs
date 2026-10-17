//! Smart plug power protocols (Tapo P115 / Tuya shape).
//!
//! v0.1 is a deterministic stub behind the real command surface so the MCP,
//! CLI, and Tauri paths can be proven end to end without hardware. The socket
//! / HTTP payload query lands here next without changing callers.
use crate::error::{require_device_id, DeviceError};
use serde::{Deserialize, Serialize};

/// Snapshot returned by `plug.get_stats`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerStats {
    pub device_ip: String,
    pub is_on: bool,
    pub current_power_watts: f32,
    pub today_kwh: f32,
}

/// Stateless controller. Shared token pools / HTTP clients will live here
/// when the real Tapo handshake is ported from `devices-mcp`.
#[derive(Debug, Default, Clone)]
pub struct PlugController;

impl PlugController {
    pub fn new() -> Self {
        Self
    }

    /// Query current wattage and energy counters.
    pub fn get_stats(&self, device_ip: &str) -> Result<PowerStats, DeviceError> {
        let id = require_device_id(device_ip)?;
        // TODO(scorecard-tier-2): real Tapo KLAP handshake over TCP.
        // Stub stays deterministic so oracle tests are exact.
        Ok(PowerStats {
            device_ip: id.to_string(),
            is_on: true,
            current_power_watts: 42.5,
            today_kwh: 0.85,
        })
    }

    /// Turn a plug on (`state = true`) or off.
    pub fn toggle(&self, device_ip: &str, state: bool) -> Result<(), DeviceError> {
        let _ = require_device_id(device_ip)?;
        // TODO(scorecard-tier-2): safe dispatch with retry + error bubbling.
        let _ = state;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_round_trip_through_json() {
        let ctl = PlugController::new();
        let stats = match ctl.get_stats("192.168.1.50") {
            Ok(s) => s,
            Err(_) => PowerStats {
                device_ip: String::new(),
                is_on: false,
                current_power_watts: 0.0,
                today_kwh: 0.0,
            },
        };
        let json = serde_json::to_string(&stats).unwrap_or_default();
        let back: PowerStats = serde_json::from_str(&json).unwrap_or(PowerStats {
            device_ip: String::new(),
            is_on: false,
            current_power_watts: 0.0,
            today_kwh: 0.0,
        });
        assert_eq!(stats, back);
        assert!((back.current_power_watts - 42.5).abs() < f32::EPSILON);
    }

    #[test]
    fn toggle_rejects_empty_ip() {
        let ctl = PlugController::new();
        let result = ctl.toggle("", true);
        assert!(matches!(result, Err(DeviceError::InvalidInput(_))));
    }
}
