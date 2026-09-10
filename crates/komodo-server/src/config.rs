use std::{env, sync::Arc, time::Duration};

use thiserror::Error;
use url::Url;
use zeroize::Zeroizing;

use komodo_api::Credentials;

use crate::auth::{AuthConfigError, GatewayBearers, IdentityVerifierSettings};

const MAXIMUM_SECRET_BYTES: usize = 16 * 1024;
type EnvironmentLookup<'a> = dyn Fn(&'static str) -> Result<String, env::VarError> + 'a;

#[derive(Debug)]
pub struct Settings {
    pub host: String,
    pub port: u16,
    pub log_level: String,
    pub upstream_url: Url,
    pub upstream_timeout: Duration,
    pub read_credentials: Credentials,
    pub admin_credentials: Credentials,
    pub allowed_hosts: Vec<String>,
    pub allowed_origins: Vec<String>,
    pub request_timeout: Duration,
    pub max_concurrent_requests: usize,
    pub max_body_bytes: usize,
    pub bearers: Arc<GatewayBearers>,
    pub identity: IdentityVerifierSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerSettings {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("invalid value for {variable}: {message}")]
    Invalid {
        variable: &'static str,
        message: String,
    },
    #[error("secret environment variable {0} is unavailable or invalid")]
    SecretEnvironment(&'static str),
    #[error(transparent)]
    Auth(#[from] AuthConfigError),
    #[error(transparent)]
    Api(#[from] komodo_api::ApiError),
}

impl Settings {
    /// Read the complete runtime configuration, including secrets supplied by environment.
    ///
    /// # Errors
    ///
    /// Returns an error when required values are absent, unsafe, malformed, or out of bounds.
    pub fn from_env() -> Result<Self, SettingsError> {
        Self::from_environment(&|variable| env::var(variable))
    }

    fn from_environment(environment: &EnvironmentLookup<'_>) -> Result<Self, SettingsError> {
        let listener = Self::listener_from_environment(environment)?;
        let read_credentials = Credentials::from_protected(
            required_secret(environment, "KOMODO_MCP_READ_API_KEY")?,
            required_secret(environment, "KOMODO_MCP_READ_API_SECRET")?,
        )?;
        let admin_credentials = Credentials::from_protected(
            required_secret(environment, "KOMODO_MCP_ADMIN_API_KEY")?,
            required_secret(environment, "KOMODO_MCP_ADMIN_API_SECRET")?,
        )?;
        let current = required_secret(environment, "KOMODO_MCP_GATEWAY_BEARER_CURRENT")?;
        let previous = optional_secret(environment, "KOMODO_MCP_GATEWAY_BEARER_PREVIOUS")?;
        let jwks_url = parse_url(
            "KOMODO_MCP_IDENTITY_JWKS_URL",
            &required(environment, "KOMODO_MCP_IDENTITY_JWKS_URL")?,
        )?;

        Ok(Self {
            host: listener.host,
            port: listener.port,
            log_level: value_or(environment, "KOMODO_MCP_LOG_LEVEL", "info"),
            upstream_url: parse_url(
                "KOMODO_MCP_UPSTREAM_URL",
                &value_or(environment, "KOMODO_MCP_UPSTREAM_URL", "http://core:9120/"),
            )?,
            upstream_timeout: Duration::from_secs(parse_number(
                environment,
                "KOMODO_MCP_UPSTREAM_TIMEOUT_SECONDS",
                20_u64,
                1,
                120,
            )?),
            read_credentials,
            admin_credentials,
            allowed_hosts: parse_csv(&value_or(
                environment,
                "KOMODO_MCP_ALLOWED_HOSTS",
                "komodo-mcp,komodo-mcp:8000,localhost,127.0.0.1",
            )),
            allowed_origins: parse_csv(
                &environment("KOMODO_MCP_ALLOWED_ORIGINS").unwrap_or_default(),
            ),
            request_timeout: Duration::from_secs(parse_number(
                environment,
                "KOMODO_MCP_REQUEST_TIMEOUT_SECONDS",
                30_u64,
                1,
                120,
            )?),
            max_concurrent_requests: parse_number(
                environment,
                "KOMODO_MCP_MAX_CONCURRENT_REQUESTS",
                32_usize,
                1,
                256,
            )?,
            max_body_bytes: parse_number(
                environment,
                "KOMODO_MCP_MAX_BODY_BYTES",
                1024_usize * 1024,
                1024,
                4 * 1024 * 1024,
            )?,
            bearers: Arc::new(GatewayBearers::from_protected(current, previous)?),
            identity: IdentityVerifierSettings {
                jwks_url,
                issuer: required(environment, "KOMODO_MCP_IDENTITY_ISSUER")?,
                actor: required_unmodified(environment, "KOMODO_MCP_IDENTITY_ACTOR")?,
                request_timeout: Duration::from_secs(3),
                cache_ttl: Duration::from_mins(5),
            },
        })
    }

    /// Read only non-secret listener coordinates for the container healthcheck.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid port values.
    pub fn listener_from_env() -> Result<ListenerSettings, SettingsError> {
        Self::listener_from_environment(&|variable| env::var(variable))
    }

    fn listener_from_environment(
        environment: &EnvironmentLookup<'_>,
    ) -> Result<ListenerSettings, SettingsError> {
        Ok(ListenerSettings {
            host: value_or(environment, "KOMODO_MCP_HOST", "0.0.0.0"),
            port: parse_number(environment, "KOMODO_MCP_PORT", 8000_u16, 1, u16::MAX)?,
        })
    }
}

fn required_secret(
    environment: &EnvironmentLookup<'_>,
    variable: &'static str,
) -> Result<Zeroizing<String>, SettingsError> {
    let value = environment(variable).map_err(|_| SettingsError::SecretEnvironment(variable))?;
    validate_secret(variable, value)
}

fn optional_secret(
    environment: &EnvironmentLookup<'_>,
    variable: &'static str,
) -> Result<Option<Zeroizing<String>>, SettingsError> {
    match environment(variable) {
        Ok(value) if value.is_empty() => Ok(None),
        Ok(value) => validate_secret(variable, value).map(Some),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(SettingsError::SecretEnvironment(variable)),
    }
}

fn validate_secret(
    variable: &'static str,
    value: String,
) -> Result<Zeroizing<String>, SettingsError> {
    if value.is_empty() || value.len() > MAXIMUM_SECRET_BYTES {
        return Err(SettingsError::SecretEnvironment(variable));
    }
    Ok(Zeroizing::new(value))
}

fn required(
    environment: &EnvironmentLookup<'_>,
    variable: &'static str,
) -> Result<String, SettingsError> {
    optional(environment, variable).ok_or(SettingsError::Missing(variable))
}

fn required_unmodified(
    environment: &EnvironmentLookup<'_>,
    variable: &'static str,
) -> Result<String, SettingsError> {
    environment(variable).map_err(|_| SettingsError::Missing(variable))
}

fn optional(environment: &EnvironmentLookup<'_>, variable: &'static str) -> Option<String> {
    environment(variable)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn value_or(environment: &EnvironmentLookup<'_>, variable: &'static str, default: &str) -> String {
    optional(environment, variable).unwrap_or_else(|| default.to_owned())
}

fn parse_number<T>(
    environment: &EnvironmentLookup<'_>,
    variable: &'static str,
    default: T,
    minimum: T,
    maximum: T,
) -> Result<T, SettingsError>
where
    T: Copy + PartialOrd + std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let raw = optional(environment, variable);
    parse_number_value(variable, raw.as_deref(), default, minimum, maximum)
}

fn parse_number_value<T>(
    variable: &'static str,
    raw: Option<&str>,
    default: T,
    minimum: T,
    maximum: T,
) -> Result<T, SettingsError>
where
    T: Copy + PartialOrd + std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let value = raw.map_or(Ok(default), |raw| {
        raw.parse::<T>().map_err(|error| SettingsError::Invalid {
            variable,
            message: error.to_string(),
        })
    })?;
    if value < minimum || value > maximum {
        return Err(SettingsError::Invalid {
            variable,
            message: "value is outside the supported range".into(),
        });
    }
    Ok(value)
}

fn parse_url(variable: &'static str, value: &str) -> Result<Url, SettingsError> {
    Url::parse(value).map_err(|error| SettingsError::Invalid {
        variable,
        message: error.to_string(),
    })
}

fn parse_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        MAXIMUM_SECRET_BYTES, Settings, SettingsError, parse_csv, parse_number_value,
        validate_secret,
    };

    use crate::auth::{AuthConfigError, IdentityVerifier};

    #[test]
    fn padded_actor_fails_the_complete_settings_and_verifier_path() {
        let environment = |variable| {
            let value = match variable {
                "KOMODO_MCP_READ_API_KEY" => "test-read-key",
                "KOMODO_MCP_READ_API_SECRET" => "test-read-secret",
                "KOMODO_MCP_ADMIN_API_KEY" => "test-admin-key",
                "KOMODO_MCP_ADMIN_API_SECRET" => "test-admin-secret",
                "KOMODO_MCP_GATEWAY_BEARER_CURRENT" => "0123456789abcdef0123456789abcdef",
                "KOMODO_MCP_IDENTITY_JWKS_URL" => "http://127.0.0.1:65533/jwks",
                "KOMODO_MCP_IDENTITY_ISSUER" => "https://gateway.test",
                "KOMODO_MCP_IDENTITY_ACTOR" => " padded-gateway-actor ",
                _ => return Err(std::env::VarError::NotPresent),
            };
            Ok(value.to_owned())
        };
        let settings = Settings::from_environment(&environment).expect("complete settings");
        assert_eq!(
            settings.identity.actor, " padded-gateway-actor ",
            "the production settings path must not normalize the trust anchor"
        );
        assert_eq!(
            IdentityVerifier::new(settings.identity).err(),
            Some(AuthConfigError::InvalidIdentitySettings)
        );
    }

    #[test]
    fn environment_secrets_are_bounded_nonempty_and_preserve_values() {
        let valid = validate_secret("TEST_SECRET", " secret-value ".into()).expect("valid secret");
        assert_eq!(valid.as_str(), " secret-value ");
        assert_eq!(
            validate_secret("TEST_SECRET", "x".repeat(MAXIMUM_SECRET_BYTES))
                .expect("maximum secret")
                .len(),
            MAXIMUM_SECRET_BYTES
        );
        for value in [String::new(), "x".repeat(MAXIMUM_SECRET_BYTES + 1)] {
            assert!(matches!(
                validate_secret("TEST_SECRET", value),
                Err(SettingsError::SecretEnvironment("TEST_SECRET"))
            ));
        }
    }

    #[test]
    fn bounded_numbers_accept_only_the_inclusive_contract() {
        assert_eq!(
            parse_number_value("TEST_NUMBER", None, 20_u64, 1, 120).expect("default"),
            20
        );
        assert_eq!(
            parse_number_value("TEST_NUMBER", Some("1"), 20_u64, 1, 120).expect("minimum"),
            1
        );
        assert_eq!(
            parse_number_value("TEST_NUMBER", Some("120"), 20_u64, 1, 120).expect("maximum"),
            120
        );
        for invalid in ["0", "121", "not-a-number"] {
            assert!(parse_number_value("TEST_NUMBER", Some(invalid), 20_u64, 1, 120).is_err());
        }
    }

    #[test]
    fn comma_separated_allowlists_are_trimmed_lowercase_and_nonempty() {
        assert_eq!(
            parse_csv(" Komodo-MCP, ,LOCALHOST,komodo-mcp:8000 "),
            ["komodo-mcp", "localhost", "komodo-mcp:8000"]
        );
        assert!(parse_csv(" , ").is_empty());
    }
}
