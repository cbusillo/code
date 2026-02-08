use anyhow::Result;
use code_core::config::ConfigBuilder;
use code_core::config_types::OtelExporterKind;
use code_core::config_types::OtelHttpProtocol;
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use tempfile::TempDir;

const SERVICE_VERSION: &str = "0.0.0-test";

fn set_metrics_exporter(config: &mut code_core::config::Config) {
    config.otel.exporter = OtelExporterKind::OtlpHttp {
        endpoint: "http://localhost:4318".to_string(),
        headers: HashMap::new(),
        protocol: OtelHttpProtocol::Json,
    };
}

#[tokio::test]
async fn app_server_default_analytics_disabled_without_flag() -> Result<()> {
    let codex_home = TempDir::new()?;
    let config = ConfigBuilder::new()
        .with_code_home(codex_home.path().to_path_buf())
        .load()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let provider = code_core::otel_init::build_provider(&config, SERVICE_VERSION)
    .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    // With exporter unset in the config, metrics are disabled.
    assert_eq!(provider.is_none(), true);
    Ok(())
}

#[tokio::test]
async fn app_server_default_analytics_enabled_with_flag() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mut config = ConfigBuilder::new()
        .with_code_home(codex_home.path().to_path_buf())
        .load()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    set_metrics_exporter(&mut config);

    let provider = code_core::otel_init::build_provider(&config, SERVICE_VERSION)
    .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    // With exporter configured, metrics are enabled.
    assert_eq!(provider.is_some(), true);
    Ok(())
}
