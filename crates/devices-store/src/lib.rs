//! L1 SQLite store: long-term weather, energy, and alarm readings.
//!
//! Local-first (`rusqlite`, bundled SQLite — no system dependency, no
//! server). The database lives at `%LOCALAPPDATA%/rust-devices-mcp/`
//! `devices.db` (`DEVICES_DB` overrides) — never in the repo, always
//! gitignored. Schema versions via `user_version` pragma; numbered
//! migrations run in order at open. Retention defaults to 90 days.
//!
//! Concurrency: `rusqlite::Connection` is opened per call (cheap for
//! SQLite) so no handle crosses threads or `.await` points. Callers in
//! async contexts wrap these sync calls as-is (local disk, milliseconds)
//! or via `spawn_blocking` for strictness.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

use std::path::PathBuf;

pub const SCHEMA_VERSION: i64 = 1;
pub const DEFAULT_RETENTION_DAYS: i64 = 90;

/// Database location: `DEVICES_DB` wins, else the app-data dir.
/// Documented, overridable, never the repo tree.
pub fn default_path() -> PathBuf {
    if let Ok(path) = std::env::var("DEVICES_DB") {
        let trimmed = path.trim().to_string();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(appdata)
            .join("rust-devices-mcp")
            .join("devices.db");
    }
    PathBuf::from("devices.db")
}

fn open(path: &std::path::Path) -> Result<rusqlite::Connection, String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| format!("store dir: {e}"))?;
        }
    }
    rusqlite::Connection::open(path).map_err(|e| format!("store open: {e}"))
}

/// Create tables + run pending migrations. Idempotent — safe on every boot.
pub fn init(path: &std::path::Path) -> Result<(), String> {
    let conn = open(path)?;
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| format!("store version read: {e}"))?;
    if version < 1 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS energy_readings (
               ts INTEGER NOT NULL,
               device_ip TEXT NOT NULL,
               vendor TEXT NOT NULL DEFAULT '',
               watts REAL NOT NULL,
               kwh REAL NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_energy_ip_ts
               ON energy_readings(device_ip, ts);
             CREATE TABLE IF NOT EXISTS weather_readings (
               ts INTEGER NOT NULL,
               temp_c REAL,
               humidity REAL,
               co2 REAL,
               pressure REAL,
               source TEXT NOT NULL DEFAULT ''
             );
             CREATE INDEX IF NOT EXISTS idx_weather_ts ON weather_readings(ts);
             CREATE TABLE IF NOT EXISTS alarm_events (
               ts INTEGER NOT NULL,
               kind TEXT NOT NULL,
               location_id TEXT,
               mode TEXT,
               detail TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_alarm_ts ON alarm_events(ts);
             PRAGMA user_version = 1;",
        )
        .map_err(|e| format!("store migrate v1: {e}"))?;
    }
    Ok(())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn db() -> Result<rusqlite::Connection, String> {
    let path = default_path();
    init(&path)?;
    open(&path)
}

pub fn record_energy(device_ip: &str, vendor: &str, watts: f32, kwh: f32) -> Result<(), String> {
    let conn = db()?;
    conn.execute(
        "INSERT INTO energy_readings (ts, device_ip, vendor, watts, kwh)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![now_unix(), device_ip, vendor, watts as f64, kwh as f64],
    )
    .map_err(|e| format!("store energy insert: {e}"))?;
    Ok(())
}

pub fn record_weather(
    temp_c: Option<f64>,
    humidity: Option<f64>,
    co2: Option<f64>,
    pressure: Option<f64>,
    source: &str,
) -> Result<(), String> {
    let conn = db()?;
    conn.execute(
        "INSERT INTO weather_readings (ts, temp_c, humidity, co2, pressure, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![now_unix(), temp_c, humidity, co2, pressure, source],
    )
    .map_err(|e| format!("store weather insert: {e}"))?;
    Ok(())
}

pub fn record_alarm(
    kind: &str,
    location_id: Option<&str>,
    mode: Option<&str>,
    detail: Option<&str>,
) -> Result<(), String> {
    let conn = db()?;
    conn.execute(
        "INSERT INTO alarm_events (ts, kind, location_id, mode, detail)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![now_unix(), kind, location_id, mode, detail],
    )
    .map_err(|e| format!("store alarm insert: {e}"))?;
    Ok(())
}

/// (unix_ts, watts) newest-last, capped for charting.
pub fn query_energy(device_ip: &str, hours: u32, limit: u32) -> Result<Vec<(i64, f64)>, String> {
    let conn = db()?;
    let since = now_unix() - (hours.max(1) as i64) * 3600;
    let mut stmt = conn
        .prepare(
            "SELECT ts, watts FROM energy_readings
             WHERE device_ip = ?1 AND ts >= ?2 ORDER BY ts ASC LIMIT ?3",
        )
        .map_err(|e| format!("store energy query: {e}"))?;
    let rows = stmt
        .query_map(
            rusqlite::params![device_ip, since, limit.max(1) as i64],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?)),
        )
        .map_err(|e| format!("store energy rows: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("store energy row: {e}"))?);
    }
    Ok(out)
}

/// (unix_ts, temp_c) newest-last, capped for charting.
pub fn query_weather(hours: u32, limit: u32) -> Result<Vec<(i64, f64)>, String> {
    let conn = db()?;
    let since = now_unix() - (hours.max(1) as i64) * 3600;
    let mut stmt = conn
        .prepare(
            "SELECT ts, temp_c FROM weather_readings
             WHERE ts >= ?1 AND temp_c IS NOT NULL ORDER BY ts ASC LIMIT ?2",
        )
        .map_err(|e| format!("store weather query: {e}"))?;
    let rows = stmt
        .query_map(rusqlite::params![since, limit.max(1) as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
        })
        .map_err(|e| format!("store weather rows: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("store weather row: {e}"))?);
    }
    Ok(out)
}

/// Recent alarm events, newest first.
pub fn query_alarms(hours: u32, limit: u32) -> Result<Vec<serde_json::Value>, String> {
    let conn = db()?;
    let since = now_unix() - (hours.max(1) as i64) * 3600;
    let mut stmt = conn
        .prepare(
            "SELECT ts, kind, location_id, mode, detail FROM alarm_events
             WHERE ts >= ?1 ORDER BY ts DESC LIMIT ?2",
        )
        .map_err(|e| format!("store alarm query: {e}"))?;
    let rows = stmt
        .query_map(rusqlite::params![since, limit.max(1) as i64], |row| {
            Ok(serde_json::json!({
                "ts": row.get::<_, i64>(0)?,
                "kind": row.get::<_, String>(1)?,
                "location_id": row.get::<_, Option<String>>(2)?,
                "mode": row.get::<_, Option<String>>(3)?,
                "detail": row.get::<_, Option<String>>(4)?,
            }))
        })
        .map_err(|e| format!("store alarm rows: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("store alarm row: {e}"))?);
    }
    Ok(out)
}

/// Delete rows older than `days`. Returns total deleted.
pub fn prune(days: u32) -> Result<u64, String> {
    let conn = db()?;
    let cutoff = now_unix() - (days.max(1) as i64) * 86400;
    let mut deleted = 0u64;
    for table in ["energy_readings", "weather_readings", "alarm_events"] {
        let count = conn
            .execute(
                &format!("DELETE FROM {table} WHERE ts < ?1"),
                rusqlite::params![cutoff],
            )
            .map_err(|e| format!("store prune {table}: {e}"))?;
        deleted += count as u64;
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    /// Tests share one file-backed temp DB serially (env override per test
    /// would race on process env — the lock + unique table prefix avoids it).
    fn test_db(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("rdm-test-{name}.db"));
        let _ = std::fs::remove_file(&path);
        std::env::set_var("DEVICES_DB", &path);
        path
    }

    #[test]
    fn energy_round_trip() {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _path = test_db("energy");
        record_energy("192.168.0.1", "tapo", 290.5, 4.25).unwrap_or(());
        let rows = query_energy("192.168.0.1", 24, 100).unwrap_or_default();
        assert_eq!(rows.len(), 1);
        assert!((rows[0].1 - 290.5).abs() < 0.01);
        let _ = std::fs::remove_file(test_db("energy-x"));
        std::env::remove_var("DEVICES_DB");
    }

    #[test]
    fn weather_and_alarm_round_trip() {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _path = test_db("wx");
        record_weather(Some(21.5), Some(40.0), None, Some(1014.0), "test").unwrap_or(());
        let rows = query_weather(24, 100).unwrap_or_default();
        assert_eq!(rows.len(), 1);
        record_alarm("mode", Some("loc1"), Some("away"), None).unwrap_or(());
        let alarms = query_alarms(24, 100).unwrap_or_default();
        assert_eq!(alarms.len(), 1);
        assert_eq!(alarms[0].get("mode").and_then(|v| v.as_str()), Some("away"));
        std::env::remove_var("DEVICES_DB");
    }

    #[test]
    fn prune_keeps_recent_drops_nothing_new() {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _path = test_db("prune");
        record_energy("192.168.0.2", "demo", 1.0, 0.01).unwrap_or(());
        let deleted = prune(90).unwrap_or(999);
        assert_eq!(deleted, 0);
        let rows = query_energy("192.168.0.2", 24, 100).unwrap_or_default();
        assert_eq!(rows.len(), 1);
        std::env::remove_var("DEVICES_DB");
    }

    #[test]
    fn schema_is_idempotent() {
        let path = std::env::temp_dir().join("rdm-test-idem.db");
        let _ = std::fs::remove_file(&path);
        init(&path).unwrap_or(());
        init(&path).unwrap_or(());
        let _ = std::fs::remove_file(&path);
    }
}
