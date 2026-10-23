use std::env::{self, VarError};
use std::fmt::Display;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::num::{NonZeroU64, NonZeroUsize};
use std::str::FromStr;
use std::time::Duration;

use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;

const DEFAULT_LISTEN_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 3000);
const DEFAULT_MAX_REQUEST_BYTES: usize = 25 * 1024 * 1024;
const DEFAULT_SMTP_TIMEOUT: Duration = Duration::from_secs(30);
const IMPLICIT_TLS_PORT: u16 = 465;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("environment variable {0} must be set together with {1}")]
    Incomplete(&'static str, &'static str),
    #[error("invalid value of environment variable {name}: {reason}")]
    Invalid { name: &'static str, reason: String },
}

pub struct Config {
    pub listen_addr: SocketAddr,
    pub max_request_bytes: usize,
    pub api_token: Option<String>,
    pub default_from: Option<Mailbox>,
    pub smtp: SmtpConfig,
}

pub struct SmtpConfig {
    pub domain: String,
    pub port: u16,
    pub tls: TlsMode,
    pub credentials: Option<Credentials>,
    pub helo_name: Option<String>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsMode {
    StartTls,
    Opportunistic,
    Implicit,
    None,
}

impl TlsMode {
    fn default_for(port: u16) -> Self {
        if port == IMPLICIT_TLS_PORT {
            return Self::Implicit;
        }
        Self::StartTls
    }
}

impl FromStr for TlsMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "starttls" => Ok(Self::StartTls),
            "opportunistic" => Ok(Self::Opportunistic),
            "tls" => Ok(Self::Implicit),
            "none" => Ok(Self::None),
            _ => Err(format!(
                "expected one of starttls, opportunistic, tls, none but got {value:?}"
            )),
        }
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::load(&Vars(|name: &str| env::var(name)))
    }

    fn load<F>(vars: &Vars<F>) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Result<String, VarError>,
    {
        Ok(Self {
            listen_addr: vars.parse("LISTEN_ADDR")?.unwrap_or(DEFAULT_LISTEN_ADDR),
            max_request_bytes: vars
                .parse("MAX_REQUEST_BYTES")?
                .map_or(DEFAULT_MAX_REQUEST_BYTES, NonZeroUsize::get),
            api_token: vars.get("API_TOKEN")?,
            default_from: vars.parse("MAIL_DEFAULT_FROM")?,
            smtp: SmtpConfig::load(vars)?,
        })
    }
}

impl SmtpConfig {
    fn load<F>(vars: &Vars<F>) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Result<String, VarError>,
    {
        let port = vars.parse_required("SMTP_PORT")?;
        Ok(Self {
            domain: vars.require("SMTP_DOMAIN")?,
            port,
            tls: vars
                .parse("SMTP_TLS")?
                .unwrap_or_else(|| TlsMode::default_for(port)),
            credentials: credentials(vars)?,
            helo_name: vars.get("SMTP_HELO_NAME")?,
            timeout: vars
                .parse("SMTP_TIMEOUT_SECS")?
                .map_or(DEFAULT_SMTP_TIMEOUT, |secs: NonZeroU64| {
                    Duration::from_secs(secs.get())
                }),
        })
    }
}

fn credentials<F>(vars: &Vars<F>) -> Result<Option<Credentials>, ConfigError>
where
    F: Fn(&str) -> Result<String, VarError>,
{
    match (vars.get("SMTP_USERNAME")?, vars.get("SMTP_PASSWORD")?) {
        (Some(username), Some(password)) => Ok(Some(Credentials::new(username, password))),
        (None, None) => Ok(None),
        (Some(_), None) => Err(ConfigError::Incomplete("SMTP_USERNAME", "SMTP_PASSWORD")),
        (None, Some(_)) => Err(ConfigError::Incomplete("SMTP_PASSWORD", "SMTP_USERNAME")),
    }
}

struct Vars<F>(F);

impl<F> Vars<F>
where
    F: Fn(&str) -> Result<String, VarError>,
{
    fn get(&self, name: &'static str) -> Result<Option<String>, ConfigError> {
        match self.0(name) {
            Ok(value) if value.is_empty() => Ok(None),
            Ok(value) => Ok(Some(value)),
            Err(VarError::NotPresent) => Ok(None),
            Err(VarError::NotUnicode(_)) => Err(ConfigError::Invalid {
                name,
                reason: "value is not valid unicode".to_owned(),
            }),
        }
    }

    fn require(&self, name: &'static str) -> Result<String, ConfigError> {
        self.get(name)?.ok_or(ConfigError::Missing(name))
    }

    fn parse<T>(&self, name: &'static str) -> Result<Option<T>, ConfigError>
    where
        T: FromStr,
        T::Err: Display,
    {
        self.get(name)?
            .map(|value| parse_value(name, &value))
            .transpose()
    }

    fn parse_required<T>(&self, name: &'static str) -> Result<T, ConfigError>
    where
        T: FromStr,
        T::Err: Display,
    {
        parse_value(name, &self.require(name)?)
    }
}

fn parse_value<T>(name: &'static str, value: &str) -> Result<T, ConfigError>
where
    T: FromStr,
    T::Err: Display,
{
    value.parse().map_err(|error: T::Err| ConfigError::Invalid {
        name,
        reason: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn load(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let values: HashMap<String, String> = pairs
            .iter()
            .map(|&(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        Config::load(&Vars(|name: &str| {
            values.get(name).cloned().ok_or(VarError::NotPresent)
        }))
    }

    #[test]
    fn applies_defaults() -> Result<(), ConfigError> {
        let config = load(&[("SMTP_DOMAIN", "smtp.domain.tld"), ("SMTP_PORT", "587")])?;
        assert_eq!(config.listen_addr, DEFAULT_LISTEN_ADDR);
        assert_eq!(config.max_request_bytes, DEFAULT_MAX_REQUEST_BYTES);
        assert!(config.api_token.is_none());
        assert!(config.default_from.is_none());
        assert_eq!(config.smtp.domain, "smtp.domain.tld");
        assert_eq!(config.smtp.port, 587);
        assert_eq!(config.smtp.tls, TlsMode::StartTls);
        assert!(config.smtp.credentials.is_none());
        assert_eq!(config.smtp.timeout, DEFAULT_SMTP_TIMEOUT);
        Ok(())
    }

    #[test]
    fn defaults_to_implicit_tls_on_port_465() -> Result<(), ConfigError> {
        let config = load(&[("SMTP_DOMAIN", "smtp.domain.tld"), ("SMTP_PORT", "465")])?;
        assert_eq!(config.smtp.tls, TlsMode::Implicit);
        Ok(())
    }

    #[test]
    fn reads_all_values() -> Result<(), ConfigError> {
        let config = load(&[
            ("LISTEN_ADDR", "127.0.0.1:8080"),
            ("MAX_REQUEST_BYTES", "1024"),
            ("API_TOKEN", "secret"),
            ("MAIL_DEFAULT_FROM", "Foo <foo@domain.tld>"),
            ("SMTP_DOMAIN", "smtp.domain.tld"),
            ("SMTP_PORT", "2525"),
            ("SMTP_TLS", "None"),
            ("SMTP_USERNAME", "user"),
            ("SMTP_PASSWORD", "pass"),
            ("SMTP_HELO_NAME", "proxy.domain.tld"),
            ("SMTP_TIMEOUT_SECS", "5"),
        ])?;
        assert_eq!(config.listen_addr.to_string(), "127.0.0.1:8080");
        assert_eq!(config.max_request_bytes, 1024);
        assert_eq!(config.api_token.as_deref(), Some("secret"));
        assert_eq!(
            config.default_from.map(|from| from.to_string()).as_deref(),
            Some("Foo <foo@domain.tld>")
        );
        assert_eq!(config.smtp.tls, TlsMode::None);
        assert!(config.smtp.credentials.is_some());
        assert_eq!(config.smtp.helo_name.as_deref(), Some("proxy.domain.tld"));
        assert_eq!(config.smtp.timeout, Duration::from_secs(5));
        Ok(())
    }

    #[test]
    fn treats_empty_values_as_missing() {
        let result = load(&[("SMTP_DOMAIN", ""), ("SMTP_PORT", "587")]);
        assert!(matches!(result, Err(ConfigError::Missing("SMTP_DOMAIN"))));
    }

    #[test]
    fn requires_port() {
        let result = load(&[("SMTP_DOMAIN", "smtp.domain.tld")]);
        assert!(matches!(result, Err(ConfigError::Missing("SMTP_PORT"))));
    }

    #[test]
    fn rejects_invalid_values() {
        let cases = [
            ("SMTP_PORT", "smtp"),
            ("SMTP_TLS", "ssl"),
            ("SMTP_TIMEOUT_SECS", "0"),
            ("MAX_REQUEST_BYTES", "0"),
            ("MAIL_DEFAULT_FROM", "not an address"),
            ("LISTEN_ADDR", "localhost"),
        ];
        for (name, value) in cases {
            let mut pairs = vec![("SMTP_DOMAIN", "smtp.domain.tld"), ("SMTP_PORT", "587")];
            pairs.retain(|(existing, _)| *existing != name);
            pairs.push((name, value));
            let result = load(&pairs);
            assert!(
                matches!(result, Err(ConfigError::Invalid { name: invalid, .. }) if invalid == name),
                "{name}={value} should be rejected"
            );
        }
    }

    #[test]
    fn requires_complete_credentials() {
        let result = load(&[
            ("SMTP_DOMAIN", "smtp.domain.tld"),
            ("SMTP_PORT", "587"),
            ("SMTP_USERNAME", "user"),
        ]);
        assert!(matches!(
            result,
            Err(ConfigError::Incomplete("SMTP_USERNAME", "SMTP_PASSWORD"))
        ));
    }
}
