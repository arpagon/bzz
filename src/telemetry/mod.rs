pub mod config;
pub mod credential;
pub mod exporter;
pub mod otlp;

use crate::{
    config::Config,
    error::{Error, Result},
    paths::Paths,
};

pub use exporter::TelemetryHandle;

pub fn start_if_enabled(config: &Config, paths: &Paths) -> Result<Option<TelemetryHandle>> {
    if !config.telemetry.enabled {
        return Ok(None);
    }
    let endpoint = match self::config::endpoint(config) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            exporter::record_start_failure(
                &paths.telemetry_health_file(),
                crate::diagnostics::ErrorClass::from_error(&error),
            );
            return Ok(None);
        }
    };
    let installation_id = config
        .telemetry
        .installation_id
        .ok_or_else(|| Error::Config("telemetry installation identity is absent".into()))?;
    if !config.telemetry.credential_persisted && std::env::var_os("BZZ_OTEL_TOKEN").is_none() {
        exporter::record_start_failure(
            &paths.telemetry_health_file(),
            crate::diagnostics::ErrorClass::AccessDenied,
        );
        return Ok(None);
    }
    let token = match credential::load(installation_id) {
        Ok(token) => token,
        Err(error) => {
            exporter::record_start_failure(
                &paths.telemetry_health_file(),
                crate::diagnostics::ErrorClass::from_error(&error),
            );
            return Ok(None);
        }
    };
    match TelemetryHandle::start(endpoint, token, paths.telemetry_health_file()) {
        Ok(handle) => Ok(Some(handle)),
        Err(error) => {
            exporter::record_start_failure(
                &paths.telemetry_health_file(),
                crate::diagnostics::ErrorClass::from_error(&error),
            );
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn paths(temp: &TempDir) -> Paths {
        Paths {
            config_dir: temp.path().join("config"),
            data_dir: temp.path().join("data"),
            cache_dir: temp.path().join("cache"),
        }
    }

    #[test]
    fn default_off_creates_no_remote_exporter() {
        let temp = TempDir::new().unwrap();
        let paths = paths(&temp);
        paths.ensure().unwrap();
        assert!(
            start_if_enabled(&Config::default(), &paths)
                .unwrap()
                .is_none()
        );
        assert!(!paths.telemetry_health_file().exists());
    }

    #[test]
    fn a_tampered_endpoint_binding_fails_locally_without_blocking_bzz() {
        let temp = TempDir::new().unwrap();
        let paths = paths(&temp);
        paths.ensure().unwrap();
        let mut config = Config::default();
        crate::telemetry::config::configure(&mut config, "https://otel.example/v1/logs").unwrap();
        config.telemetry.enabled = true;
        config.telemetry.endpoint = Some("https://other.example/v1/logs".into());
        assert!(start_if_enabled(&config, &paths).unwrap().is_none());
        assert_eq!(
            exporter::read_health(&paths.telemetry_health_file()).last_error_class,
            Some(crate::diagnostics::ErrorClass::Unknown)
        );
    }
}
