//! Weather via Open-Meteo (no key, no account — works out of the box).
//!
//! Vienna defaults (48.2082, 16.3738); any lat/lon accepted with hostile
//! range checks. Netatmo stays the precision path when its secret exists;
//! this is the always-on path for the dashboard.
use crate::creds::{classify_vendor_error, with_timeout_secs};
use crate::error::DeviceError;
use std::time::Duration;

pub const VIENNA_LAT: f64 = 48.2082;
pub const VIENNA_LON: f64 = 16.3738;

/// WMO weather-code summary for humans and agents.
pub fn wmo_description(code: i64) -> &'static str {
    match code {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 | 63 | 65 => "Rain",
        66..=67 => "Freezing rain",
        71 | 73 | 75 | 77 => "Snow",
        80..=82 => "Rain showers",
        85..=86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Unknown",
    }
}

pub async fn now(
    latitude: Option<f64>,
    longitude: Option<f64>,
) -> Result<serde_json::Value, DeviceError> {
    let lat = latitude.unwrap_or(VIENNA_LAT);
    let lon = longitude.unwrap_or(VIENNA_LON);
    if !lat.is_finite() || !(-90.0..=90.0).contains(&lat) {
        return Err(DeviceError::InvalidInput(format!(
            "latitude {lat} out of range -90..=90"
        )));
    }
    if !lon.is_finite() || !(-180.0..=180.0).contains(&lon) {
        return Err(DeviceError::InvalidInput(format!(
            "longitude {lon} out of range -180..=180"
        )));
    }
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| DeviceError::Protocol(format!("weather http client: {e}")))?;
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m\
         &hourly=temperature_2m,relative_humidity_2m,precipitation_probability\
         &daily=temperature_2m_max,temperature_2m_min,precipitation_probability_max\
         &forecast_days=1&timezone=auto"
    );
    let resp = with_timeout_secs("weather.now", 15, http.get(&url).send())
        .await?
        .map_err(|e| classify_vendor_error(format!("weather.now: {e}")))?;
    if !resp.status().is_success() {
        return Err(DeviceError::Protocol(format!(
            "weather.now HTTP {}",
            resp.status()
        )));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| DeviceError::Protocol(format!("weather.now decode: {e}")))?;
    let current = body
        .get("current")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let code = current
        .get("weather_code")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    let first = |key: &str| {
        body.get("daily")
            .and_then(|d| d.get(key))
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    };
    Ok(serde_json::json!({
        "place": {
            "latitude": body.get("latitude"),
            "longitude": body.get("longitude"),
            "timezone": body.get("timezone"),
        },
        "current": current,
        "summary": wmo_description(code),
        "today": {
            "max_c": first("temperature_2m_max"),
            "min_c": first("temperature_2m_min"),
            "precipitation_probability_max": first("precipitation_probability_max"),
        },
        "hourly": body.get("hourly"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wmo_table_covers_common_codes() {
        assert_eq!(wmo_description(0), "Clear sky");
        assert_eq!(wmo_description(95), "Thunderstorm");
        assert_eq!(wmo_description(12345), "Unknown");
    }

    #[tokio::test]
    async fn out_of_range_coords_rejected_without_network() {
        // Validation precedes all I/O: no request leaves the machine.
        let bad_lat = now(Some(200.0), None).await;
        assert!(matches!(
            bad_lat,
            Err(crate::error::DeviceError::InvalidInput(_))
        ));
        let bad_lon = now(None, Some(200.0)).await;
        assert!(matches!(
            bad_lon,
            Err(crate::error::DeviceError::InvalidInput(_))
        ));
    }
}
