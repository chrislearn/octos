//! Webhook reverse proxy for Feishu and Twilio.
//!
//! Routes incoming webhook requests from a single public URL to the correct
//! profile's gateway process based on the profile ID in the URL path.
//!
//! ```text
//! POST /webhook/feishu/{profile_id}  →  127.0.0.1:{port}/webhook/event
//! POST /webhook/twilio/{profile_id}  →  127.0.0.1:{port}/twilio/webhook
//! ```

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use super::AppState;

/// Proxy Feishu/Lark webhook events to the gateway's local webhook server.
pub async fn feishu_webhook_proxy(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    proxy_to_gateway(state, profile_id, "/webhook/event", headers, body).await
}

/// Proxy Twilio webhook events to the gateway's local webhook server.
pub async fn twilio_webhook_proxy(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    proxy_to_gateway(state, profile_id, "/twilio/webhook", headers, body).await
}

/// Forward a request to the gateway's local webhook server.
async fn proxy_to_gateway(
    state: Arc<AppState>,
    profile_id: String,
    upstream_path: &str,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let pm = match state.process_manager.as_ref() {
        Some(pm) => pm,
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    let port = match pm.webhook_port(&profile_id).await {
        Some(port) => port,
        None => return StatusCode::BAD_GATEWAY.into_response(),
    };

    let url = format!("http://127.0.0.1:{port}{upstream_path}");

    // Convert axum body to reqwest body
    let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    // Build upstream request preserving headers
    let mut req = state.http_client.post(&url).body(body_bytes.to_vec());

    // Forward relevant headers
    for (name, value) in &headers {
        // Skip hop-by-hop headers
        let n = name.as_str();
        if matches!(
            n,
            "host" | "connection" | "transfer-encoding" | "keep-alive"
        ) {
            continue;
        }
        if let Ok(v) = value.to_str() {
            req = req.header(name.clone(), v);
        }
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(
                profile = %profile_id,
                url = %url,
                error = %e,
                "webhook proxy: upstream request failed"
            );
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };

    // Convert upstream response back to axum response
    let status =
        StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let resp_headers = resp.headers().clone();
    let resp_body = match resp.bytes().await {
        Ok(b) => b,
        Err(_) => return StatusCode::BAD_GATEWAY.into_response(),
    };

    let mut response = (status, resp_body.to_vec()).into_response();
    // Copy content-type from upstream
    if let Some(ct) = resp_headers.get("content-type") {
        response.headers_mut().insert("content-type", ct.clone());
    }

    response
}
