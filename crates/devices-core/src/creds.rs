//! Vendor selection and credential resolution.
//!
//! Rule: explicit CLI/MCP args win, environment is the fallback, and a
//! missing secret is a typed [`DeviceError::Auth`] — never a panic, never a
//! log line containing the secret itself.
use crate::error::DeviceError;
use std::future::Future;
use std::time::Duration;

/// Single-call I/O budget. Multi-step handshakes use explicit multiples.
pub const DEVICE_TIMEOUT: Duration = Duration::from_secs(5);

/// Plug vendors. `Demo` is the deterministic oracle for tests and probes —
/// explicitly not hardware, explicitly named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Tapo,
    Kasa,
    Shelly,
    Demo,
}

impl Vendor {
    /// Parse hostile input: case-insensitive, whitespace-tolerant, aliases
    /// accepted, everything else a typed error.
    pub fn parse(raw: &str) -> Result<Self, DeviceError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "tapo" => Ok(Self::Tapo),
            "kasa" | "tplink" | "tp-link" => Ok(Self::Kasa),
            "shelly" => Ok(Self::Shelly),
            "demo" | "stub" | "mock" => Ok(Self::Demo),
            _ => Err(DeviceError::InvalidInput(format!(
                "unknown vendor '{raw}': expected tapo|kasa|shelly|demo"
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tapo => "tapo",
            Self::Kasa => "kasa",
            Self::Shelly => "shelly",
            Self::Demo => "demo",
        }
    }
}

/// Account material. `None` means "not provided anywhere", which is fine for
/// vendors that don't need it (Kasa legacy, Shelly without auth) and a typed
/// error for vendors that do (Tapo).
#[derive(Debug, Clone, Default)]
pub struct Credentials {
    pub username: Option<String>,
    pub password: Option<String>,
}

impl Credentials {
    /// Explicit args win; per-vendor env vars are the fallback.
    pub fn resolve(
        vendor: Vendor,
        username: Option<String>,
        password: Option<String>,
    ) -> Result<Self, DeviceError> {
        let clean = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let (env_user, env_pass) = match vendor {
            Vendor::Tapo => (
                env_non_empty("TAPO_EMAIL")
                    .or_else(|| env_non_empty("TAPO_USERNAME"))
                    .or_else(|| env_non_empty("TAPO_P115_EMAIL"))
                    .or_else(|| env_non_empty("TAPO_LIGHTING_EMAIL")),
                env_non_empty("TAPO_PASSWORD")
                    .or_else(|| env_non_empty("TAPO_P115_PASSWORD"))
                    .or_else(|| env_non_empty("TAPO_LIGHTING_PASSWORD")),
            ),
            Vendor::Kasa => (
                env_non_empty("KASA_USERNAME"),
                env_non_empty("KASA_PASSWORD"),
            ),
            Vendor::Shelly => (None, env_non_empty("SHELLY_PASSWORD")),
            Vendor::Demo => (None, None),
        };
        let resolved = Self {
            username: clean(username).or(env_user),
            password: clean(password).or(env_pass),
        };
        if vendor == Vendor::Tapo && (resolved.username.is_none() || resolved.password.is_none()) {
            return Err(DeviceError::Auth(
                "Tapo account required: pass username/password or set TAPO_EMAIL + TAPO_PASSWORD (per-device TAPO_P115_* / TAPO_LIGHTING_* work too)"
                    .to_string(),
            ));
        }
        Ok(resolved)
    }

    /// Tapo refuses anonymous handshakes — unwrap the account or fail typed.
    /// Callers hold no secrets in logs: this returns owned clones for the
    /// handshake call only.
    pub fn require_tapo_account(&self) -> Result<(String, String), DeviceError> {
        match (&self.username, &self.password) {
            (Some(u), Some(p)) => Ok((u.clone(), p.clone())),
            _ => Err(DeviceError::Auth(
                "Tapo account required: pass username/password or set TAPO_EMAIL + TAPO_PASSWORD"
                    .to_string(),
            )),
        }
    }

    /// Kasa legacy devices need nothing; newer KLAP devices take the account.
    pub fn kasa_opt(&self) -> Option<kasa_rs::Creds> {
        match (&self.username, &self.password) {
            (Some(email), Some(password)) => Some(kasa_rs::Creds {
                email: email.clone(),
                password: password.clone(),
            }),
            _ => None,
        }
    }
}

fn env_non_empty(name: &str) -> Option<String> {
    // Process env first, devices-mcp `.env` second (operator-permitted
    // fleet import) — so the CLI works with zero setup on fleet machines.
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| crate::import::fleet_secret(name))
}

/// Shared vendor-error classifier: timeout/auth/unsupported/protocol from
/// message text. Every driver maps its foreign error type through here so
/// agents see one stable taxonomy.
pub fn classify_vendor_error(context: String) -> DeviceError {
    let lower = context.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") || lower.contains("deadline") {
        DeviceError::Timeout(context)
    } else if lower.contains("auth")
        || lower.contains("login")
        || lower.contains("password")
        || lower.contains("credential")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("401")
        || lower.contains("403")
    {
        DeviceError::Auth(context)
    } else if lower.contains("unsupported") || lower.contains("not support") {
        DeviceError::Unsupported(context)
    } else {
        DeviceError::Protocol(context)
    }
}

/// Bound any single device op. Timeout carries the op name, never a secret.
pub async fn with_timeout<T>(op: &str, fut: impl Future<Output = T>) -> Result<T, DeviceError> {
    with_timeout_secs(op, DEVICE_TIMEOUT.as_secs(), fut).await
}

/// Multi-step handshakes (Tapo login + queries) get an explicit budget.
pub async fn with_timeout_secs<T>(
    op: &str,
    secs: u64,
    fut: impl Future<Output = T>,
) -> Result<T, DeviceError> {
    match tokio::time::timeout(Duration::from_secs(secs), fut).await {
        Ok(value) => Ok(value),
        Err(_) => Err(DeviceError::Timeout(format!("{op} exceeded {secs}s"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_parse_accepts_aliases() {
        assert_eq!(Vendor::parse("TAPO"), Ok(Vendor::Tapo));
        assert_eq!(Vendor::parse(" tplink "), Ok(Vendor::Kasa));
        assert_eq!(Vendor::parse("mock"), Ok(Vendor::Demo));
        assert!(Vendor::parse("philips").is_err());
    }

    #[test]
    fn tapo_explicit_account_always_resolves() {
        // Machine-independent: explicit args must win regardless of what
        // the operator's env or fleet checkout contains (a previous version
        // asserted the no-creds error, which fails on machines WITH creds —
        // exactly the wrong coupling for a unit test).
        let creds = Credentials::resolve(
            Vendor::Tapo,
            Some("me@example.com".to_string()),
            Some("secret".to_string()),
        )
        .unwrap_or_default();
        assert_eq!(creds.username.as_deref(), Some("me@example.com"));
        let (user, pass) = creds
            .require_tapo_account()
            .unwrap_or((String::new(), String::new()));
        assert_eq!(user, "me@example.com");
        assert_eq!(pass, "secret");
    }

    #[test]
    fn demo_needs_no_account() {
        let creds = Credentials::resolve(Vendor::Demo, None, None).unwrap_or_default();
        assert!(creds.username.is_none());
    }

    #[test]
    fn explicit_args_win_over_env() {
        let creds =
            Credentials::resolve(Vendor::Kasa, Some("u".to_string()), None).unwrap_or_default();
        assert_eq!(creds.username.as_deref(), Some("u"));
    }
}
