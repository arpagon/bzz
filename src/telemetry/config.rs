use sha2::{Digest as _, Sha256};
use url::Url;

use crate::{
    config::{Config, validate_telemetry_endpoint},
    error::{Error, Result},
};

pub fn configure(config: &mut Config, endpoint: &str) -> Result<uuid::Uuid> {
    let endpoint = validate_telemetry_endpoint(endpoint)?;
    let canonical = endpoint.to_string();
    let installation_id = config
        .telemetry
        .installation_id
        .unwrap_or_else(uuid::Uuid::new_v4);
    config.telemetry.enabled = false;
    config.telemetry.credential_persisted = false;
    config.telemetry.endpoint_digest = Some(endpoint_digest(&canonical));
    config.telemetry.endpoint = Some(canonical);
    config.telemetry.installation_id = Some(installation_id);
    config.validate()?;
    Ok(installation_id)
}

pub fn endpoint(config: &Config) -> Result<Url> {
    let configured = config
        .telemetry
        .endpoint
        .as_deref()
        .ok_or_else(|| Error::Config("telemetry is not configured".into()))?;
    let endpoint = validate_telemetry_endpoint(configured)?;
    let expected = config
        .telemetry
        .endpoint_digest
        .as_deref()
        .ok_or_else(|| Error::Config("telemetry endpoint binding is absent".into()))?;
    if !constant_time_eq(expected.as_bytes(), endpoint_digest(configured).as_bytes()) {
        return Err(Error::Config(
            "telemetry endpoint binding changed; configure telemetry again".into(),
        ));
    }
    Ok(endpoint)
}

pub fn endpoint_origin(endpoint: &Url) -> String {
    let host = endpoint.host_str().unwrap_or("invalid");
    match endpoint.port() {
        Some(port) => format!("{}://{host}:{port}", endpoint.scheme()),
        None => format!("{}://{host}", endpoint.scheme()),
    }
}

pub fn endpoint_digest(endpoint: &str) -> String {
    hex::encode(Sha256::digest(endpoint.as_bytes()))
}

pub fn forget(config: &mut Config) {
    config.telemetry = crate::config::TelemetryConfig::default();
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_bound_and_sanitized() {
        let mut config = Config::default();
        configure(&mut config, "https://otel.example/v1/logs").unwrap();
        let configured_endpoint = endpoint(&config).unwrap();
        assert_eq!(
            endpoint_origin(&configured_endpoint),
            "https://otel.example"
        );
        config.telemetry.endpoint = Some("https://other.example/v1/logs".into());
        assert!(endpoint(&config).is_err());
    }

    #[test]
    fn endpoint_validation_forbids_redirectable_or_credential_urls() {
        for endpoint in [
            "http://otel.example/v1/logs",
            "https://user@otel.example/v1/logs",
            "https://otel.example/other",
            "https://otel.example/v1/logs?token=bad",
        ] {
            assert!(configure(&mut Config::default(), endpoint).is_err());
        }
    }
}
