//! REST API and SSE streaming for crew-rs.
//!
//! Feature-gated behind `api`. Start with `crew serve [--port 8080]`.

mod handlers;
pub mod metrics;
mod router;
mod sse;
mod static_files;

pub use metrics::init_metrics;
pub use router::build_router;
pub use sse::SseBroadcaster;

use std::sync::Arc;

/// Shared application state for API handlers.
pub struct AppState {
    /// Agent for processing messages.
    pub agent: Arc<crew_agent::Agent>,
    /// Session manager for history.
    pub sessions: Arc<tokio::sync::Mutex<crew_bus::SessionManager>>,
    /// SSE broadcaster for streaming events.
    pub broadcaster: Arc<SseBroadcaster>,
    /// Server start time.
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Auth token (if configured).
    pub auth_token: Option<String>,
    /// Prometheus metrics handle.
    pub metrics_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
}
