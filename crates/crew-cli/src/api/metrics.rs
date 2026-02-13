//! Prometheus metrics endpoint and helpers.

use std::sync::Arc;

use axum::extract::State;
use metrics::{counter, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

use super::AppState;

/// Initialize the Prometheus metrics recorder and return a handle for rendering.
pub fn init_metrics() -> PrometheusHandle {
    PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install Prometheus recorder")
}

/// GET /metrics -- render Prometheus text exposition format.
pub async fn metrics_handler(State(state): State<Arc<AppState>>) -> String {
    match state.metrics_handle {
        Some(ref handle) => handle.render(),
        None => String::new(),
    }
}

/// Record a tool call metric.
pub fn record_tool_call(name: &str, success: bool, duration_secs: f64) {
    let labels = [
        ("tool", name.to_string()),
        ("success", success.to_string()),
    ];
    counter!("crew_tool_calls_total", &labels).increment(1);
    histogram!("crew_tool_call_duration_seconds", "tool" => name.to_string())
        .record(duration_secs);
}

/// Record LLM token usage.
pub fn record_llm_tokens(direction: &str, count: u32) {
    counter!("crew_llm_tokens_total", "direction" => direction.to_string())
        .increment(u64::from(count));
}
