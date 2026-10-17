//! LAN device discovery: concurrent TCP port scan + identification.
//!
//! Joe Schmoe onboarding: no config, no checkout — point at a subnet, get a
//! device list, adopt. Identification is precise where protocols allow it
//! (Hue `/api/config.json` and Shelly `/shelly` need no auth) and an honest
//! port-based guess elsewhere.
use crate::error::DeviceError;
use futures::StreamExt;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct FoundDevice {
    pub ip: String,
    pub port: u16,
    pub kind: String,
    pub label: String,
}

/// Probes per port: (port, fallback guess when nothing identifies).
const PROBES: &[(u16, &str)] = &[
    (80, "http"),
    (443, "https"),
    (554, "rtsp?"),
    (2020, "onvif?"),
    (8080, "http-alt"),
    (9999, "kasa?"),
];

/// Parse "192.168.0", "192.168.0.0/24", "10.0.5.0/24" → 3-octet prefix.
/// Hostile input → typed error, never a scan of nonsense.
pub fn parse_subnet(raw: &str) -> Result<String, DeviceError> {
    let trimmed = raw.trim().trim_end_matches("/24");
    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() != 3 && parts.len() != 4 {
        return Err(DeviceError::InvalidInput(format!(
            "subnet '{raw}': expected 192.168.0 or 192.168.0.0/24"
        )));
    }
    let mut octets = [0u8; 3];
    for (index, part) in parts.iter().take(3).enumerate() {
        octets[index] = part.parse().map_err(|_| {
            DeviceError::InvalidInput(format!("subnet '{raw}': bad octet '{part}'"))
        })?;
    }
    Ok(format!("{}.{}.{}", octets[0], octets[1], octets[2]))
}

fn http_client() -> Result<reqwest::Client, DeviceError> {
    reqwest::Client::builder()
        .timeout(Duration::from_millis(900))
        .build()
        .map_err(|e| DeviceError::Protocol(format!("scan http client: {e}")))
}

async fn tcp_open(ip: &str, port: u16) -> bool {
    tokio::time::timeout(
        Duration::from_millis(450),
        tokio::net::TcpStream::connect(format!("{ip}:{port}")),
    )
    .await
    .map(|r| r.is_ok())
    .unwrap_or(false)
}

/// Identify an open HTTP port precisely where possible.
async fn identify(http: &reqwest::Client, ip: &str, port: u16, guess: &str) -> FoundDevice {
    let base = || format!("http://{ip}:{port}");
    // Hue bridges answer /api/config.json with their name, no auth.
    if port == 80 || port == 8080 || port == 443 {
        let url = format!("{}/api/config.json", base());
        if let Ok(resp) = http.get(&url).send().await {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(name) = body.get("name").and_then(|v| v.as_str()) {
                    return FoundDevice {
                        ip: ip.to_string(),
                        port,
                        kind: "hue".to_string(),
                        label: format!("Hue bridge ({name})"),
                    };
                }
            }
        }
        // Shelly Gen1 answers /shelly with type + mac, no auth.
        let url = format!("{}/shelly", base());
        if let Ok(resp) = http.get(&url).send().await {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if body.get("type").is_some() {
                    let model = body
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Shelly");
                    return FoundDevice {
                        ip: ip.to_string(),
                        port,
                        kind: "shelly".to_string(),
                        label: model.to_string(),
                    };
                }
            }
        }
    }
    FoundDevice {
        ip: ip.to_string(),
        port,
        kind: "unknown".to_string(),
        label: format!("{guess} :{port}"),
    }
}

/// Scan 1..=254 × probe ports with bounded concurrency. Typical /24 in
/// ~10–20s on WiFi. Returns open-port hits, sorted by IP then port.
pub async fn scan_subnet(prefix: &str) -> Result<Vec<FoundDevice>, DeviceError> {
    let prefix = parse_subnet(prefix)?;
    let http = http_client()?;
    // Owned triples on purpose: a borrowed `&str` guess would chain a local
    // lifetime through the stream combinators into the returned future and
    // poison `Send + 'static` for the whole dispatch tree (found 2026-10-17:
    // every spawn site failed with an inscrutable HRTB error from this spot).
    let mut targets: Vec<(String, u16, String)> = Vec::with_capacity(254 * PROBES.len());
    for host in 1u8..=254u8 {
        // Skip .1 (usually the gateway) — faster, fewer false friends.
        if host == 1 {
            continue;
        }
        for (port, guess) in PROBES {
            targets.push((format!("{prefix}.{host}"), *port, (*guess).to_string()));
        }
    }
    let found: Vec<FoundDevice> = futures::stream::iter(targets)
        .map(|(ip, port, guess): (String, u16, String)| {
            let http = http.clone();
            async move {
                if !tcp_open(&ip, port).await {
                    return None;
                }
                Some(identify(&http, &ip, port, &guess).await)
            }
        })
        .buffer_unordered(256)
        .filter_map(|hit| async move { hit })
        .collect()
        .await;
    let mut found = found;
    found.sort_by(|a, b| a.ip.cmp(&b.ip).then_with(|| a.port.cmp(&b.port)));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subnet_parse_accepts_forms_rejects_garbage() {
        assert_eq!(parse_subnet("192.168.0").unwrap_or_default(), "192.168.0");
        assert_eq!(parse_subnet(" 10.0.5.0/24 ").unwrap_or_default(), "10.0.5");
        assert!(parse_subnet("999.1.2").is_err());
        assert!(parse_subnet("example.com").is_err());
        assert!(parse_subnet("").is_err());
    }

    #[tokio::test]
    async fn closed_port_is_not_open() {
        // Discard port: must report closed, fast.
        assert!(!tcp_open("127.0.0.1", 9).await);
    }
}
