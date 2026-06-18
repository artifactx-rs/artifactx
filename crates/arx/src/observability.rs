//! tracing + metrics initialization.

use anyhow::{Context, Result};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tracing_subscriber::{prelude::*, EnvFilter};

/// Log output format selectable on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

/// Initialize the global tracing subscriber. Honors `RUST_LOG`, defaulting to `info`.
pub fn init_tracing(format: LogFormat) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::registry().with(filter);
    match format {
        LogFormat::Json => {
            registry
                .with(tracing_subscriber::fmt::layer().json())
                .init();
        }
        LogFormat::Text => {
            registry.with(tracing_subscriber::fmt::layer()).init();
        }
    }
}

/// Install the Prometheus recorder and return a handle used to render `/metrics`.
pub fn init_metrics() -> Result<PrometheusHandle> {
    PrometheusBuilder::new()
        .install_recorder()
        .context("installing Prometheus metrics recorder")
}
