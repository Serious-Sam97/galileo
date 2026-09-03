use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use figment::providers::{Env, Format, Serialized, Toml};
use figment::Figment;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub clickhouse: ClickHouseConfig,
    pub postgres: PostgresConfig,
    pub ingest: IngestConfig,
    pub retention: RetentionConfig,
    pub alerts: AlertsConfig,
    #[serde(default)]
    pub smtp: SmtpConfig,
    #[serde(default)]
    pub public_url: String,
    #[serde(default)]
    pub auth: AuthConfig,
}

/// Outbound e-mail. Empty host = e-mail channels disabled.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SmtpConfig {
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_smtp_port")]
    pub port: u16,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_from")]
    pub from: String,
    /// "starttls" (default), "tls" or "none" (plain, for local sinks).
    #[serde(default = "default_starttls")]
    pub security: String,
}
fn default_smtp_port() -> u16 {
    587
}
fn default_from() -> String {
    "Galileo <galileo@localhost>".into()
}
fn default_starttls() -> String {
    "starttls".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub oidc: Option<OidcConfig>,
}

/// OIDC single sign-on (`[auth.oidc]`): Google, GitHub (via an OIDC bridge), Keycloak, Authentik…
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    /// e.g. https://galileo.example.com:8080/api/auth/oidc/callback
    pub redirect_url: String,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    /// Org new users auto-join (uuid) and the role they get.
    #[serde(default)]
    pub default_org: Option<String>,
    #[serde(default = "d_role")]
    pub default_role: String,
    #[serde(default = "d_label")]
    pub label: String,
}
fn d_role() -> String { "member".into() }
fn d_label() -> String { "Continue with SSO".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub api_addr: SocketAddr,
    pub otlp_grpc_addr: SocketAddr,
    pub otlp_http_addr: SocketAddr,
    #[serde(default)]
    pub cors_origins: Vec<String>,
    /// 32-byte hex. Used for at-rest encryption of provider secrets and session signing.
    pub secret_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickHouseConfig {
    pub url: String,
    pub database: String,
    pub user: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresConfig {
    pub url: String,
    #[serde(default = "default_pg_conns")]
    pub max_connections: u32,
}
fn default_pg_conns() -> u32 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestConfig {
    #[serde(default = "default_batch_rows")]
    pub batch_max_rows: usize,
    #[serde(with = "humantime_serde", default = "default_batch_wait")]
    pub batch_max_wait: Duration,
    #[serde(default = "default_max_queued")]
    pub max_queued_rows: usize,
}
fn default_batch_rows() -> usize {
    5000
}
fn default_batch_wait() -> Duration {
    Duration::from_secs(1)
}
fn default_max_queued() -> usize {
    200_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionConfig {
    #[serde(with = "humantime_serde", default = "d30")]
    pub spans: Duration,
    #[serde(with = "humantime_serde", default = "d14")]
    pub logs: Duration,
    #[serde(with = "humantime_serde", default = "d90")]
    pub metrics: Duration,
    /// ClickHouse storage policy name for hot/cold tiering (see deploy/clickhouse/storage.xml).
    /// Absent = single disk.
    #[serde(default)]
    pub storage_policy: Option<String>,
    /// Days on the hot volume before rows move to the cold volume (tiered policy only).
    #[serde(default = "d_hot")]
    pub hot_days: u32,
}
fn d_hot() -> u32 { 7 }
fn d30() -> Duration {
    Duration::from_secs(30 * 86400)
}
fn d14() -> Duration {
    Duration::from_secs(14 * 86400)
}
fn d90() -> Duration {
    Duration::from_secs(90 * 86400)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertsConfig {
    #[serde(with = "humantime_serde", default = "default_tick")]
    pub tick: Duration,
}
fn default_tick() -> Duration {
    Duration::from_secs(30)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                api_addr: "127.0.0.1:8080".parse().unwrap(),
                otlp_grpc_addr: "127.0.0.1:4317".parse().unwrap(),
                otlp_http_addr: "127.0.0.1:4318".parse().unwrap(),
                cors_origins: vec!["http://localhost:3000".into()],
                secret_key: "0".repeat(64),
            },
            clickhouse: ClickHouseConfig {
                url: "http://127.0.0.1:8123".into(),
                database: "galileo".into(),
                user: "galileo".into(),
                password: "galileo".into(),
            },
            postgres: PostgresConfig {
                url: "postgres://galileo:galileo@127.0.0.1:5433/galileo".into(),
                max_connections: 10,
            },
            ingest: IngestConfig {
                batch_max_rows: default_batch_rows(),
                batch_max_wait: default_batch_wait(),
                max_queued_rows: default_max_queued(),
            },
            retention: RetentionConfig { spans: d30(), logs: d14(), metrics: d90(), storage_policy: None, hot_days: 7 },
            auth: AuthConfig::default(),
            alerts: AlertsConfig { tick: default_tick() },
            smtp: SmtpConfig::default(),
            public_url: "http://localhost:3000".into(),
        }
    }
}

impl Config {
    /// Layering: defaults < `galileo.toml` (if present) < `GALILEO_*` env vars.
    /// Env keys use `__` as the section separator: `GALILEO_CLICKHOUSE__URL`.
    pub fn load(path: Option<&Path>) -> Result<Self, Box<figment::Error>> {
        let mut fig = Figment::from(Serialized::defaults(Config::default()));
        let path = path.map(Path::to_path_buf).unwrap_or_else(|| "galileo.toml".into());
        if path.exists() {
            fig = fig.merge(Toml::file(path));
        }
        fig = fig.merge(Env::prefixed("GALILEO_").split("__"));
        fig.extract().map_err(Box::new)
    }

    pub fn secret_key_bytes(&self) -> Result<[u8; 32], String> {
        let v = hex::decode(self.secret_key_trimmed()).map_err(|e| e.to_string())?;
        if v.len() != 32 {
            return Err(format!("secret_key must be 32 bytes (64 hex chars), got {}", v.len()));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&v);
        Ok(out)
    }

    fn secret_key_trimmed(&self) -> String {
        self.server.secret_key.trim().to_owned()
    }

    pub fn is_default_secret(&self) -> bool {
        self.secret_key_trimmed().chars().all(|c| c == '0')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::result_large_err)]
    fn env_overrides_defaults() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("GALILEO_CLICKHOUSE__URL", "http://ch:8123");
            jail.set_env("GALILEO_INGEST__BATCH_MAX_WAIT", "250ms");
            let c = Config::load(Some(Path::new("nope.toml"))).unwrap();
            assert_eq!(c.clickhouse.url, "http://ch:8123");
            assert_eq!(c.ingest.batch_max_wait, Duration::from_millis(250));
            assert_eq!(c.server.api_addr.port(), 8080);
            Ok(())
        });
    }

    #[test]
    fn secret_key_validation() {
        let mut c = Config::default();
        assert!(c.is_default_secret());
        assert!(c.secret_key_bytes().is_ok());
        c.server.secret_key = "abc".into();
        assert!(c.secret_key_bytes().is_err());
    }
}
