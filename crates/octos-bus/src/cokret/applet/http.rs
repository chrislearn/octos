//! Applet-mode runtime: the inbound HTTP endpoints the Cokret server calls,
//! plus the outbound (agent-reply) path.
//!
//! Paths follow the Cokret spec `edge` trust segment (applet-integration.md
//! §6), versionless, under `/_cokret/edge/applet/...`:
//!
//! | path | method |
//! |---|---|
//! | `/_cokret/edge/applet/ping` | GET |
//! | `/_cokret/edge/applet/describe` | GET |
//! | `/_cokret/edge/applet/transactions` | POST |
//! | `/_cokret/edge/applet/actors/{actor_id}` | GET |
//! | `/_cokret/edge/applet/realms/{realm_id_or_alias}` | GET |
//! | `/_cokret/edge/applet/protocols/{protocol}` | GET |
//! | `/_cokret/edge/applet/third_party/users` | GET |
//! | `/_cokret/edge/applet/third_party/locations` | GET |
//!
//! Unlike the savfox gateway (which mounts these on a shared salvo server via a
//! global registry), each octos `CokretChannel` owns its own axum listener, so
//! the applet state is held directly rather than looked up by bearer.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::{Arc, atomic::AtomicBool, atomic::Ordering};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use chrono::Utc;
use cokret::http_signature::{
    Component, HttpMessageVerificationError, SignaturePolicyError, SignatureVerificationPolicy,
    parse_signature_input, public_key_from_bytes, verify_signed_http_message,
};
use cokret::{IdempotencyClaim, IdempotencyDirection, IdempotencyIdentity, IdempotencyWindow};
use cokret_core::{
    AppletDescription, AppletPingOutcome, AppletTransactionOutcome, AppletTransactionRequestBody,
    Did, Hash, canonical,
};
use cokret_identifiers::RealmId;
use eyre::{Result, WrapErr, bail, eyre};
use octos_core::{InboundMessage, MessageOrigin};
use serde_json::{Value, json};
use subtle::ConstantTimeEq;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::config::CokretAppletConfig;
use super::outbound::{build_applet_message_event, sign_outbound_event};
use super::transaction::{AppletDispatchSkip, AppletEventOutcome, classify_inbound_event};
use crate::cokret::client::CokretHttpClient;
use crate::cokret::crypto_state::{
    CokretDecryptOutcome, CokretEncryptOutcome, FileCokretCryptoStore,
    extract_encrypted_payload_from_message_content,
};
use crate::cokret::signer::load_ed25519_signer;

const TXN_DEDUPE_WINDOW: Duration = Duration::from_secs(300);
const MAX_APPLET_TRANSACTION_BODY_BYTES: usize = 65_536;
const EVENT_DEDUPE_MAX: usize = 4096;
const SOURCE_SERVICE_DID_HEADER: &str = "source-service-did";
const DESTINATION_SERVICE_DID_HEADER: &str = "destination-service-did";
const APPLET_TRANSACTION_SIGNATURE_MAX_LIFETIME_SECS: i64 = 300;
const APPLET_TRANSACTION_SIGNATURE_MAX_CLOCK_SKEW_SECS: i64 = 30;

/// Shared applet runtime state held by a `CokretChannel` in applet mode.
pub struct AppletState {
    config: CokretAppletConfig,
    idempotency: IdempotencyWindow<AppletTransactionOutcome>,
    seq: cokret_bridge_runtime::SeqAllocator,
    crypto_store: FileCokretCryptoStore,
    /// Bounded set of recently-dispatched `event_id`s (loop / retry guard).
    event_dedupe: Mutex<EventDedupe>,
}

impl AppletState {
    /// Build the runtime state with a restart-safe monotonic `actor_seq`
    /// allocator backed by `seq_path`.
    pub fn new(
        config: CokretAppletConfig,
        seq_path: std::path::PathBuf,
        data_dir: std::path::PathBuf,
    ) -> Result<Arc<Self>> {
        let store = crate::cokret::seq_store::FileSeqStore::shared(seq_path)
            .map_err(|e| eyre!("cokret applet seq store: {e}"))?;
        let key = format!("applet:{}:actor_seq", config.id);
        let seq = cokret_bridge_runtime::SeqAllocator::new(store, key);
        let crypto_store = FileCokretCryptoStore::for_applet(&data_dir, &config.id);
        if let Err(err) =
            FileCokretCryptoStore::feature_report().and_then(|_| crypto_store.ensure_created())
        {
            warn!(
                "cokret: applet '{}' crypto state unavailable at {}: {err:#}",
                config.id,
                crypto_store.path().display()
            );
        }
        Ok(Arc::new(Self {
            config,
            idempotency: IdempotencyWindow::new(TXN_DEDUPE_WINDOW),
            seq,
            crypto_store,
            event_dedupe: Mutex::new(EventDedupe::new(EVENT_DEDUPE_MAX)),
        }))
    }

    #[must_use]
    pub fn config(&self) -> &CokretAppletConfig {
        &self.config
    }

    fn alloc_seq(&self) -> Result<u64> {
        self.seq.alloc().map_err(|e| eyre!("seq alloc: {e}"))
    }

    /// Build the outbound HTTP client for the applet bot. Uses DID-proof login
    /// when `key_ref` is set; falls back to the static bearer otherwise.
    async fn construct_client(&self) -> Result<CokretHttpClient> {
        let cfg = &self.config;
        if let Some(key_ref) = &cfg.key_ref {
            let vm = cfg
                .verification_method
                .clone()
                .unwrap_or_else(|| format!("{}#key-1", cfg.bot_actor_id));
            let audience = cfg.cokret_server_did.as_deref().ok_or_else(|| {
                eyre!(
                    "applet '{}' has key_ref but no cokret_server_did for DID-proof audience",
                    cfg.id
                )
            })?;
            let challenge = cfg
                .login_challenge
                .as_deref()
                .ok_or_else(|| eyre!("applet '{}' has key_ref but no login_challenge", cfg.id))?;
            let signer = load_ed25519_signer(key_ref, &cfg.bot_actor_id, &vm)?;
            let principal = Did::new(cfg.bot_actor_id.clone())
                .map_err(|err| eyre!("invalid bot DID: {err}"))?;
            let device_id = cfg.device_id.clone().unwrap_or_else(|| {
                format!(
                    "ck:device:applet-{}",
                    cfg.applet_id.trim_start_matches("ck:applet:")
                )
            });
            let device = cokret_identifiers::DeviceId::new(device_id)
                .map_err(|err| eyre!("synth device_id: {err}"))?;
            let (client, _session) = CokretHttpClient::login(
                &cfg.cokret_server_url,
                &signer,
                principal,
                device,
                challenge,
                audience,
            )
            .await?;
            Ok(client)
        } else {
            let bearer = cfg.cokret_bearer_token.as_deref().ok_or_else(|| {
                eyre!(
                    "applet '{}' has neither key_ref nor cokret_bearer_token",
                    cfg.id
                )
            })?;
            CokretHttpClient::new(&cfg.cokret_server_url, bearer)
        }
    }

    /// Send an agent reply into `realm_id` / `flow_id`, attributed to the
    /// applet bot actor. Builds a `ck.message.create`, signs it when a key is
    /// configured, and submits it.
    pub async fn send_reply(&self, realm_id: &str, flow_id: &str, body: &str) -> Result<()> {
        let cfg = &self.config;
        let actor_seq = self.alloc_seq()?;
        let external_ref = json!({
            "protocol": "octos",
            "network_id": cfg.id,
            "external_id": format!("{realm_id}:{flow_id}"),
            "kind": "agent_reply",
        });
        let authorization_ref = self
            .load_grant_event_id()
            .await
            .or_else(|| cfg.authorization_grant_id.clone());

        let req = super::outbound::AppletMessageRequest {
            applet_id: cfg.applet_id.clone(),
            realm_id: realm_id.to_owned(),
            flow_id: flow_id.to_owned(),
            ghost_actor_did: cfg.bot_actor_id.clone(),
            body: body.to_owned(),
            external_ref,
            authorization_ref,
            executed_by: None,
            actor_seq,
            thread_root_id: None,
        };
        let mut event = build_applet_message_event(&req)?;
        apply_applet_outbound_encryption(&self.crypto_store, realm_id, &mut event)?;

        if let Some(key_ref) = &cfg.key_ref {
            let vm = cfg
                .verification_method
                .clone()
                .unwrap_or_else(|| format!("{}#key-1", cfg.bot_actor_id));
            let signer = load_ed25519_signer(key_ref, &cfg.bot_actor_id, &vm)?;
            sign_outbound_event(&mut event, &signer, &vm)?;
        }

        let http = self.construct_client().await?;
        let resp = http.submit_event(&event).await?;
        if !resp.rejected.is_empty() {
            return Err(eyre!(
                "cokret applet: server rejected event for realm '{realm_id}': {:?}",
                resp.rejected
            ));
        }
        if resp.accepted.is_empty() && resp.duplicate.is_empty() {
            return Err(eyre!(
                "cokret applet: server accepted no events for realm '{realm_id}' (status={:?})",
                resp.status
            ));
        }
        Ok(())
    }

    /// If `grant_event_path` is set, load + verify the capability grant and
    /// return its `event_id`. Logs and returns `None` on load failure.
    async fn load_grant_event_id(&self) -> Option<String> {
        let cfg = &self.config;
        let path = cfg.grant_event_path.as_ref()?;
        match crate::cokret::grant::load_and_verify_grant(path, &cfg.bot_actor_id, None).await {
            Ok(grant) if grant.covers_action("ck.message.create") => Some(grant.event_id),
            Ok(_) => {
                warn!(
                    "cokret applet '{}': capability grant at {} does not cover ck.message.create",
                    cfg.id,
                    path.display()
                );
                None
            }
            Err(err) => {
                warn!(
                    "cokret applet '{}': capability grant load failed at {}: {err:#}",
                    cfg.id,
                    path.display()
                );
                None
            }
        }
    }
}

fn apply_applet_outbound_encryption(
    crypto_store: &FileCokretCryptoStore,
    realm_id: &str,
    event: &mut cokret_core::Event,
) -> Result<()> {
    let Some(content_block) = event.content.get("content").cloned() else {
        return Ok(());
    };
    match crypto_store.encrypt_content_block_for_realm(realm_id, &content_block)? {
        CokretEncryptOutcome::PlaintextAllowed => Ok(()),
        CokretEncryptOutcome::Encrypted(encrypted_content) => {
            let object = event
                .content
                .as_object_mut()
                .ok_or_else(|| eyre!("Cokret applet message content is not an object"))?;
            object.remove("content");
            object.insert("encrypted_content".to_owned(), encrypted_content);
            Ok(())
        }
        CokretEncryptOutcome::MissingRequiredGroupState { realm_id, group_id } => {
            bail!(
                "Cokret realm '{realm_id}' requires E2EE but no local applet MLS group state exists for group '{group_id}'"
            );
        }
    }
}

/// Axum router context — pairs the applet state with the inbound bus sender.
#[derive(Clone)]
struct HttpCtx {
    state: Arc<AppletState>,
    inbound_tx: mpsc::Sender<InboundMessage>,
}

/// Serve the applet HTTP endpoints on `bind_addr` until `shutdown` is set.
pub async fn serve(
    state: Arc<AppletState>,
    inbound_tx: mpsc::Sender<InboundMessage>,
    bind_addr: &str,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    let ctx = HttpCtx { state, inbound_tx };
    let app = Router::new()
        .route("/_cokret/edge/applet/ping", get(applet_ping))
        .route("/_cokret/edge/applet/describe", get(applet_describe))
        .route(
            "/_cokret/edge/applet/transactions",
            post(applet_transactions),
        )
        .route("/_cokret/edge/applet/actors/{actor_id}", get(applet_actor))
        .route(
            "/_cokret/edge/applet/realms/{realm_id_or_alias}",
            get(applet_realm),
        )
        .route(
            "/_cokret/edge/applet/protocols/{protocol}",
            get(applet_protocol),
        )
        .route(
            "/_cokret/edge/applet/third_party/users",
            get(applet_third_party_users),
        )
        .route(
            "/_cokret/edge/applet/third_party/locations",
            get(applet_third_party_locations),
        )
        .with_state(ctx);

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .wrap_err_with(|| format!("cokret applet: bind {bind_addr}"))?;
    info!(bind = bind_addr, "cokret applet HTTP server listening");
    let shutdown_signal = async move {
        loop {
            if shutdown.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    };
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await
        .wrap_err("cokret applet HTTP server error")?;
    Ok(())
}

// ─── Auth helpers ────────────────────────────────────────────────────────────

fn parse_bearer_header(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    let rest = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))?;
    let token = rest.trim();
    (!token.is_empty()).then_some(token)
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()
        .and_then(|v| parse_bearer_header(v).map(str::to_owned))
}

fn token_matches(configured: Option<&str>, provided: &str) -> bool {
    let Some(configured) = configured.map(str::trim).filter(|v| !v.is_empty()) else {
        return false;
    };
    bool::from(configured.as_bytes().ct_eq(provided.trim().as_bytes()))
}

fn err_json(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({ "error": code, "message": message.into() })),
    )
        .into_response()
}

#[derive(Debug, Clone)]
struct VerifiedAppletHttpSignature {
    source_service_did: String,
    destination_service_did: String,
    key_id: String,
    source_key_state_digest: String,
    content_digest: Option<String>,
    covered_components: Vec<String>,
    created: i64,
    expires: i64,
    canonical_message_digest: String,
}

/// Bearer-authenticate the request against the applet's configured token.
fn check_auth(ctx: &HttpCtx, headers: &HeaderMap) -> Option<Response> {
    let Some(token) = bearer_token(headers) else {
        return Some(err_json(
            StatusCode::UNAUTHORIZED,
            "missing_bearer_token",
            "Cokret applet endpoint requires Authorization: Bearer <token>",
        ));
    };
    if token_matches(ctx.state.config.cokret_bearer_token.as_deref(), &token) {
        None
    } else {
        Some(err_json(
            StatusCode::UNAUTHORIZED,
            "invalid_bearer_token",
            "Authorization token does not match this Cokret applet channel",
        ))
    }
}

// ─── Handlers ────────────────────────────────────────────────────────────────

async fn applet_ping(State(ctx): State<HttpCtx>, headers: HeaderMap) -> Response {
    if let Some(resp) = check_auth(&ctx, &headers) {
        return resp;
    }
    let cfg = &ctx.state.config;
    let Ok(service_did) = Did::new(cfg.service_did.clone()) else {
        return err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_service_did",
            "applet service_did is not a valid DID",
        );
    };
    let body = AppletPingOutcome {
        ok: true,
        applet_id: cfg.applet_id.clone(),
        service_did,
        protocol_version: "1.0".to_owned(),
    };
    (StatusCode::OK, Json(body)).into_response()
}

async fn applet_describe(State(ctx): State<HttpCtx>, headers: HeaderMap) -> Response {
    if let Some(resp) = check_auth(&ctx, &headers) {
        return resp;
    }
    let cfg = &ctx.state.config;
    let Ok(service_did) = Did::new(cfg.service_did.clone()) else {
        return err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_service_did",
            "applet service_did is not a valid DID",
        );
    };
    let body = AppletDescription {
        applet_id: cfg.applet_id.clone(),
        service_did,
        protocols: cfg.protocols.clone(),
        namespaces: json!({
            "actors": cfg.namespaces.actors,
            "realms": cfg.namespaces.realms,
            "handles": cfg.namespaces.handles,
        }),
        limits: json!({
            "max_events_per_transaction": 100,
            "max_body_bytes": MAX_APPLET_TRANSACTION_BODY_BYTES,
            "e2ee": {
                "encrypted_content": "decrypt_when_local_group_state_exists",
                "outbound_policy": "encrypt_when_realm_requires_e2ee",
                "plaintext_fallback": "only_when_realm_policy_allows_plaintext",
                "device_id_configured": cfg.device_id.is_some(),
                "crypto_store": ctx.state.crypto_store.path().display().to_string(),
            },
        }),
        auth: json!({
            "type": "bearer",
            "controller_did": cfg.controller_did,
            "bot_actor_id": cfg.bot_actor_id,
            "bot_device_id": cfg.device_id.as_deref(),
            "http_message_signature": {
                "required_when_trusted_keys_configured": true,
                "trusted_verification_methods": cfg.trusted_verification_methods.len(),
            },
        }),
    };
    (StatusCode::OK, Json(body)).into_response()
}

async fn applet_transactions(
    method: Method,
    uri: Uri,
    State(ctx): State<HttpCtx>,
    headers: HeaderMap,
    raw: Bytes,
) -> Response {
    if let Some(resp) = check_auth(&ctx, &headers) {
        return resp;
    }
    let Some(idempotency_key) = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .filter(|v| !v.trim().is_empty())
    else {
        return err_json(
            StatusCode::BAD_REQUEST,
            "missing_idempotency_key",
            "Cokret applet transactions require an Idempotency-Key header",
        );
    };

    if raw.len() > MAX_APPLET_TRANSACTION_BODY_BYTES {
        return err_json(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            format!("body exceeds {MAX_APPLET_TRANSACTION_BODY_BYTES} bytes"),
        );
    }
    let body: AppletTransactionRequestBody = match serde_json::from_slice(&raw) {
        Ok(body) => body,
        Err(err) => {
            return err_json(
                StatusCode::BAD_REQUEST,
                "invalid_payload",
                format!("invalid AppletTransactionRequestBody: {err}"),
            );
        }
    };

    let request_headers = collect_headers(&headers);
    let signature_path = uri
        .path_and_query()
        .map(|value| value.as_str().to_owned())
        .unwrap_or_else(|| "/".to_owned());
    let signature_authority = request_authority(&uri, &request_headers).ok();
    let signature_target_uri = signature_authority
        .as_deref()
        .map(|authority| request_target_uri(&uri, &request_headers, authority, &signature_path));

    let source_service_did = body.source_service_did.as_str().to_owned();
    if let Some(expected_source) = ctx.state.config.cokret_server_did.as_deref()
        && source_service_did != expected_source
    {
        return err_json(
            StatusCode::UNAUTHORIZED,
            "invalid_source_service_did",
            "Cokret applet transaction source_service_did does not match the trusted server DID",
        );
    }

    let verified_http_signature = match verify_applet_transaction_http_signature(
        &ctx.state,
        method.as_str(),
        signature_target_uri.as_deref(),
        signature_authority.as_deref(),
        &signature_path,
        &request_headers,
        raw.as_ref(),
    ) {
        Ok(verified) => verified,
        Err(err) => {
            warn!(
                config_id = %ctx.state.config.id,
                "applet: inbound HTTP message signature verification failed: {err:#}"
            );
            return err_json(
                StatusCode::UNAUTHORIZED,
                "invalid_signature",
                "Cokret applet transaction HTTP message signature verification failed",
            );
        }
    };
    if let Some(signature) = verified_http_signature.as_ref()
        && signature.source_service_did != source_service_did
    {
        return err_json(
            StatusCode::UNAUTHORIZED,
            "invalid_signature",
            "Cokret applet transaction source_service_did does not match signed source service DID",
        );
    }

    let body_hash = match canonical::canonical_sha256(&body).map(Hash::new) {
        Ok(Ok(h)) => h,
        Ok(Err(err)) => {
            warn!("applet: body hash construct failed: {err}");
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "hash_failed",
                "failed to compute body canonical hash",
            );
        }
        Err(err) => {
            warn!("applet: canonical hash failed: {err}");
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "hash_failed",
                "failed to compute body canonical hash",
            );
        }
    };

    let idempotency_key = idempotency_key.trim().to_owned();
    let identity = IdempotencyIdentity::applet_transaction(
        IdempotencyDirection::NodeToApplet,
        source_service_did.clone(),
        ctx.state.config.service_did.clone(),
        idempotency_key.clone(),
    );
    let source_signature_evidence = if let Some(signature) = verified_http_signature.as_ref() {
        json!({
            "operation_id": cokret::APPLET_TRANSACTION_OPERATION_ID,
            "direction": IdempotencyDirection::NodeToApplet.as_str(),
            "source_service_did": &source_service_did,
            "destination_service_did": &signature.destination_service_did,
            "idempotency_key": &idempotency_key,
            "auth_scheme": "http_message_signature+bearer",
            "canonical_body_digest": body_hash.clone(),
            "content_digest": &signature.content_digest,
            "covered_components": &signature.covered_components,
            "created": signature.created,
            "expires": signature.expires,
            "canonical_message_digest": &signature.canonical_message_digest,
            "source_key_state_digest": &signature.source_key_state_digest,
            "verification_method": &signature.key_id,
        })
    } else {
        json!({
            "operation_id": cokret::APPLET_TRANSACTION_OPERATION_ID,
            "direction": IdempotencyDirection::NodeToApplet.as_str(),
            "source_service_did": &source_service_did,
            "destination_service_did": ctx.state.config.service_did.clone(),
            "idempotency_key": &idempotency_key,
            "auth_scheme": "bearer",
            "content_digest": body_hash.clone(),
        })
    };
    let source_signature_anchor = match canonical::canonical_json_string(&source_signature_evidence)
    {
        Ok(anchor) => anchor,
        Err(err) => {
            warn!("applet: source signature anchor construct failed: {err}");
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "idempotency_anchor_failed",
                "failed to compute idempotency source signature anchor",
            );
        }
    };

    ctx.state.idempotency.gc();
    match ctx
        .state
        .idempotency
        .claim(&identity, &body_hash, &source_signature_anchor)
    {
        IdempotencyClaim::Fresh => {}
        IdempotencyClaim::Duplicate { outcome, .. } => {
            debug!(
                source_service_did,
                idempotency_key, "applet: duplicate transaction; returning cached outcome"
            );
            return (StatusCode::OK, Json(outcome)).into_response();
        }
        IdempotencyClaim::DuplicateConflict { .. } => {
            warn!(
                source_service_did,
                idempotency_key,
                "applet: idempotency conflict with different body hash or source signature anchor"
            );
            return err_json(
                StatusCode::CONFLICT,
                "duplicate_conflict",
                "Idempotency-Key already used for a different request body or signature anchor",
            );
        }
        IdempotencyClaim::InFlight { .. } => {
            return err_json(
                StatusCode::SERVICE_UNAVAILABLE,
                "duplicate_in_flight",
                "duplicate transaction is still being processed; retry to receive the outcome",
            );
        }
    }

    // Classify events and dispatch the accepted ones.
    let mut rejected: Vec<Value> = Vec::new();
    let mut dispatched_commands = Vec::new();
    let cfg = &ctx.state.config;
    for event in body.events.iter() {
        match classify_inbound_event(cfg, event) {
            AppletEventOutcome::Dispatch(cmd) => dispatched_commands.push(cmd),
            AppletEventOutcome::Skip(reason) => {
                if matches!(reason, AppletDispatchSkip::EncryptedContent)
                    && let Some(cmd) = try_decrypt_applet_event(&ctx.state, event)
                {
                    dispatched_commands.push(cmd);
                    continue;
                }
                if matches!(reason, AppletDispatchSkip::EncryptedContent) {
                    warn!(
                        config_id = %ctx.state.config.id,
                        event_id = event.event_id.as_str(),
                        realm_id = event.realm_id.as_str(),
                        "cokret applet: encrypted inbound event rejected; no usable local MLS state"
                    );
                }
                rejected.push(json!({
                    "event_id": event.event_id.as_str(),
                    "reason_code": format!("{reason:?}"),
                }));
            }
        }
    }

    for cmd in dispatched_commands {
        {
            let Ok(mut dedupe) = ctx.state.event_dedupe.lock() else {
                continue;
            };
            if !dedupe.insert(cmd.event_id.clone()) {
                continue;
            }
        }
        let chat_id = super::super::encode_chat_id(&cmd.realm_id, cmd.flow_id.as_deref());
        let inbound = InboundMessage {
            channel: super::super::CHANNEL_NAME.to_owned(),
            sender_id: cmd.sender_did.clone(),
            chat_id,
            content: cmd.body.clone(),
            timestamp: Utc::now(),
            media: vec![],
            metadata: json!({
                "cokret_mode": "applet",
                "realm_id": cmd.realm_id,
                "flow_id": cmd.flow_id,
                "thread_root_id": cmd.thread_root_id,
                "applet_id": cfg.applet_id.clone(),
            }),
            message_id: Some(cmd.event_id.clone()),
            origin: MessageOrigin::ExternalUser,
        };
        if ctx.inbound_tx.send(inbound).await.is_err() {
            warn!(
                "applet: inbound bus closed; dropping event {}",
                cmd.event_id
            );
        }
    }

    let outcome = AppletTransactionOutcome {
        ok: true,
        rejected,
        retry_after_ms: None,
    };
    if !ctx.state.idempotency.complete(&identity, outcome.clone()) {
        warn!("applet: idempotency claim was not in-flight when completing transaction");
    }

    (StatusCode::OK, Json(outcome)).into_response()
}

fn verify_applet_transaction_http_signature(
    state: &AppletState,
    method: &str,
    target_uri: Option<&str>,
    authority: Option<&str>,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<Option<VerifiedAppletHttpSignature>> {
    let has_signature_headers = header_value_from(headers, "signature-input").is_some()
        || header_value_from(headers, "signature").is_some();
    if state.config.trusted_verification_methods.is_empty() {
        if has_signature_headers {
            bail!(
                "request carries HTTP Message Signature headers but no trusted verification methods are configured"
            );
        }
        return Ok(None);
    }

    let expected_source = state.config.cokret_server_did.as_deref().ok_or_else(|| {
        eyre!("trusted verification methods require cokret_server_did / trustedServerDid")
    })?;
    let signature_input_header = header_value_from(headers, "signature-input")
        .ok_or_else(|| eyre!("Signature-Input header is required"))?;
    let signature_input = parse_signature_input(&signature_input_header)
        .map_err(|err| eyre!("parse Signature-Input: {err}"))?;
    let trusted_method = state
        .config
        .trusted_verification_methods
        .iter()
        .find(|method| method.verification_method == signature_input.key_id)
        .ok_or_else(|| {
            eyre!(
                "verification method '{}' is not trusted for applet '{}'",
                signature_input.key_id,
                state.config.id
            )
        })?;
    let signer_did = verification_method_did(&signature_input.key_id)
        .ok_or_else(|| eyre!("HTTP signature keyid has no DID fragment"))?;
    if signer_did != expected_source {
        bail!(
            "HTTP signature keyid owner '{signer_did}' does not match trusted server DID '{expected_source}'"
        );
    }

    let source = header_value_from(headers, SOURCE_SERVICE_DID_HEADER)
        .ok_or_else(|| eyre!("{SOURCE_SERVICE_DID_HEADER} header is required"))?;
    if source != expected_source {
        bail!(
            "HTTP signature source service DID '{source}' does not match trusted server DID '{expected_source}'"
        );
    }
    let destination = header_value_from(headers, DESTINATION_SERVICE_DID_HEADER)
        .ok_or_else(|| eyre!("{DESTINATION_SERVICE_DID_HEADER} header is required"))?;
    if destination != state.config.service_did {
        bail!(
            "HTTP signature destination service DID '{destination}' does not match applet service DID '{}'",
            state.config.service_did
        );
    }

    let public_key_bytes = trusted_method
        .public_key
        .ed25519_bytes()
        .map_err(|err| eyre!("trusted HTTP signature public key: {err}"))?;
    let source_key_state_digest = canonical::canonical_digest(&public_key_bytes);
    let public_key = public_key_from_bytes(&public_key_bytes)
        .map_err(|err| eyre!("trusted HTTP signature public key: {err}"))?;
    let authority = authority.ok_or_else(|| eyre!("request authority/Host is required"))?;
    let target_uri =
        target_uri.ok_or_else(|| eyre!("request target URI could not be constructed"))?;
    let required_components = vec![
        Component::Method,
        Component::TargetUri,
        Component::Authority,
        Component::Header(SOURCE_SERVICE_DID_HEADER.to_owned()),
        Component::Header(DESTINATION_SERVICE_DID_HEADER.to_owned()),
        Component::Header("content-digest".to_owned()),
        Component::Header("idempotency-key".to_owned()),
    ];
    let policy = SignatureVerificationPolicy::new(required_components)
        .require_content_digest(true)
        .max_clock_skew_seconds(APPLET_TRANSACTION_SIGNATURE_MAX_CLOCK_SKEW_SECS)
        .max_validity_window_seconds(APPLET_TRANSACTION_SIGNATURE_MAX_LIFETIME_SECS);
    let verified = verify_signed_http_message(
        method,
        target_uri,
        authority,
        path,
        headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str())),
        body,
        &public_key,
        &policy,
        chrono::Utc::now().timestamp(),
    )
    .map_err(map_http_signature_error)?;
    let content_digest = verified
        .content_digest
        .as_ref()
        .map(|digest| digest.wire_value.clone());
    let covered_components = verified
        .signature_input
        .covered_components
        .iter()
        .map(Component::canonical_name)
        .collect();
    let canonical_message_digest = canonical::canonical_digest(&verified.canonical_message);
    Ok(Some(VerifiedAppletHttpSignature {
        source_service_did: source,
        destination_service_did: destination,
        key_id: verified.signature_input.key_id,
        source_key_state_digest,
        content_digest,
        covered_components,
        created: verified.signature_input.created,
        expires: verified.signature_input.expires,
        canonical_message_digest,
    }))
}

fn collect_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.trim().to_owned()))
        })
        .collect()
}

fn header_value_from(headers: &[(String, String)], name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let values: Vec<&str> = headers
        .iter()
        .filter(|(candidate, _)| candidate.eq_ignore_ascii_case(&lower))
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
        .collect();
    if values.is_empty() {
        None
    } else {
        Some(values.join(", "))
    }
}

fn request_authority(uri: &Uri, headers: &[(String, String)]) -> Result<String> {
    header_value_from(headers, "host")
        .or_else(|| {
            uri.authority()
                .map(|authority| authority.as_str().to_owned())
        })
        .ok_or_else(|| eyre!("request authority/Host is required"))
}

fn request_target_uri(
    uri: &Uri,
    headers: &[(String, String)],
    authority: &str,
    path: &str,
) -> String {
    if uri.scheme().is_some() && uri.authority().is_some() {
        return uri.to_string();
    }
    let scheme = header_value_from(headers, "x-forwarded-proto")
        .and_then(|value| value.split(',').next().map(str::trim).map(str::to_owned))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "http".to_owned());
    format!("{scheme}://{authority}{path}")
}

fn verification_method_did(verification_method: &str) -> Option<&str> {
    verification_method
        .rsplit_once('#')
        .map(|(did, _)| did)
        .filter(|did| !did.is_empty())
}

fn map_http_signature_error(err: HttpMessageVerificationError) -> eyre::Report {
    match err {
        HttpMessageVerificationError::MissingHeader(header) => {
            eyre!("required HTTP signature header '{header}' is missing")
        }
        HttpMessageVerificationError::Policy(
            SignaturePolicyError::MissingContentDigest
            | SignaturePolicyError::MissingRequiredCoveredComponent,
        ) => eyre!("HTTP signature does not cover required applet transaction fields"),
        HttpMessageVerificationError::Policy(SignaturePolicyError::InvalidValidityWindow) => {
            eyre!("HTTP signature validity window is invalid")
        }
        HttpMessageVerificationError::Policy(
            SignaturePolicyError::CreatedInFuture | SignaturePolicyError::Expired,
        ) => eyre!("HTTP signature timestamp is outside the accepted window"),
        HttpMessageVerificationError::Signature(err) => eyre!("{err}"),
    }
}

fn try_decrypt_applet_event(
    state: &AppletState,
    event: &cokret_core::Event,
) -> Option<super::transaction::AppletInboundCommand> {
    let payload = extract_encrypted_payload_from_message_content(&event.content)?;
    if let Some(device_id) = state.config.device_id.as_deref() {
        match state.crypto_store.plan_bootstrap_for_payload(
            &state.config.bot_actor_id,
            device_id,
            &payload,
        ) {
            Ok(plan) => debug!(
                config_id = %state.config.id,
                group_id = %plan.group_id,
                required_epoch = plan.required_epoch,
                local_epoch = ?plan.local_epoch,
                action = ?plan.action,
                "cokret applet: planned crypto bootstrap for encrypted event"
            ),
            Err(err) => warn!(
                config_id = %state.config.id,
                "cokret applet: failed to plan crypto bootstrap for encrypted event: {err:#}"
            ),
        }
    }

    match state.crypto_store.try_decrypt_content_block(&payload) {
        Ok(CokretDecryptOutcome::Decrypted(content)) => {
            let Some(body) = decrypted_text_body(&content) else {
                warn!(
                    config_id = %state.config.id,
                    event_id = event.event_id.as_str(),
                    "cokret applet: decrypted encrypted event but content is not displayable text"
                );
                return None;
            };
            Some(super::transaction::AppletInboundCommand {
                event_id: event.event_id.as_str().to_owned(),
                realm_id: event.realm_id.as_str().to_owned(),
                flow_id: event
                    .content
                    .get("flow_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                sender_did: event.actor_id.as_str().to_owned(),
                body,
                thread_root_id: event
                    .content
                    .get("thread_root_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        }
        Ok(CokretDecryptOutcome::MissingGroupState) => {
            record_applet_unable_to_decrypt(
                state,
                event,
                payload,
                cokret::crypto_protocol::UnableToDecryptReason::NoSession,
            );
            None
        }
        Ok(CokretDecryptOutcome::UnsupportedScheme(scheme)) => {
            warn!(
                config_id = %state.config.id,
                event_id = event.event_id.as_str(),
                scheme,
                "cokret applet: unsupported encrypted payload scheme"
            );
            record_applet_unable_to_decrypt(
                state,
                event,
                payload,
                cokret::crypto_protocol::UnableToDecryptReason::BadCiphertext,
            );
            None
        }
        Err(err) => {
            warn!(
                config_id = %state.config.id,
                event_id = event.event_id.as_str(),
                "cokret applet: encrypted event decrypt failed: {err:#}"
            );
            record_applet_unable_to_decrypt(
                state,
                event,
                payload,
                cokret::crypto_protocol::UnableToDecryptReason::BadCiphertext,
            );
            None
        }
    }
}

fn record_applet_unable_to_decrypt(
    state: &AppletState,
    event: &cokret_core::Event,
    payload: cokret_core::EncryptedPayload,
    reason: cokret::crypto_protocol::UnableToDecryptReason,
) {
    if let Err(err) = state.crypto_store.record_unable_to_decrypt(
        event.event_id.as_str(),
        event.realm_id.as_str(),
        event.actor_id.as_str(),
        payload,
        reason,
    ) {
        warn!(
            config_id = %state.config.id,
            event_id = event.event_id.as_str(),
            "cokret applet: failed to persist unable-to-decrypt record: {err:#}"
        );
    }
}

fn decrypted_text_body(content: &Value) -> Option<String> {
    let block = content
        .get("content")
        .filter(|inner| inner.get("kind").is_some())
        .unwrap_or(content);
    let kind = block.get("kind").and_then(Value::as_str)?;
    if kind != "ck.content.text" {
        return None;
    }
    block
        .get("body")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|body| !body.is_empty())
        .map(str::to_owned)
}

async fn applet_actor(
    State(ctx): State<HttpCtx>,
    headers: HeaderMap,
    Path(actor_id): Path<String>,
) -> Response {
    if let Some(resp) = check_auth(&ctx, &headers) {
        return resp;
    }
    if !ctx.state.config.namespaces.actor_matches(&actor_id) {
        return err_json(
            StatusCode::NOT_FOUND,
            "actor_not_in_namespace",
            format!("actor '{actor_id}' is not in this applet's namespace"),
        );
    }
    (
        StatusCode::OK,
        Json(json!({
            "exists": true,
            "actor_id": Did::new(actor_id).ok().map(|d| d.as_str().to_owned()),
            "display_name": Value::Null,
            "external_ref": Value::Null,
        })),
    )
        .into_response()
}

async fn applet_realm(
    State(ctx): State<HttpCtx>,
    headers: HeaderMap,
    Path(realm): Path<String>,
) -> Response {
    if let Some(resp) = check_auth(&ctx, &headers) {
        return resp;
    }
    if !ctx.state.config.namespaces.realm_matches(&realm) {
        return err_json(
            StatusCode::NOT_FOUND,
            "realm_not_in_namespace",
            format!("realm '{realm}' is not in this applet's namespace"),
        );
    }
    (
        StatusCode::OK,
        Json(json!({
            "exists": true,
            "realm_id": RealmId::new(realm).ok().map(|r| r.as_str().to_owned()),
            "title": Value::Null,
            "external_ref": Value::Null,
        })),
    )
        .into_response()
}

async fn applet_protocol(
    State(ctx): State<HttpCtx>,
    headers: HeaderMap,
    Path(protocol): Path<String>,
) -> Response {
    if let Some(resp) = check_auth(&ctx, &headers) {
        return resp;
    }
    if !ctx.state.config.protocols.iter().any(|p| p == &protocol) {
        return err_json(
            StatusCode::NOT_FOUND,
            "protocol_not_supported",
            format!("protocol '{protocol}' is not registered with this applet"),
        );
    }
    (
        StatusCode::OK,
        Json(json!({
            "protocol": protocol,
            "display_name": protocol,
            "icon_blob_ref": Value::Null,
            "field_types": {},
            "instances": [],
        })),
    )
        .into_response()
}

async fn applet_third_party_users(
    State(ctx): State<HttpCtx>,
    headers: HeaderMap,
    Query(fields): Query<HashMap<String, String>>,
) -> Response {
    if let Some(resp) = check_auth(&ctx, &headers) {
        return resp;
    }
    let cfg = &ctx.state.config;
    let Some(protocol) = supported_protocol(cfg, &fields) else {
        return err_json(
            StatusCode::NOT_FOUND,
            "protocol_not_supported",
            "third_party lookup requires a supported protocol query parameter",
        );
    };
    let actor_id = field_first(
        &fields,
        &["actor_id", "user", "user_id", "external_id", "id"],
    )
    .map(|external_id| {
        super::ghost::mint_ghost_did(&cfg.service_did, &cfg.ghost_did_prefix, external_id)
    })
    .filter(|actor_id| cfg.namespaces.actor_matches(actor_id));
    let exists = actor_id.is_some();
    (
        StatusCode::OK,
        Json(json!({
            "actor_id": actor_id,
            "exists": exists,
            "external_ref": external_ref_with_protocol(&protocol, &fields),
        })),
    )
        .into_response()
}

async fn applet_third_party_locations(
    State(ctx): State<HttpCtx>,
    headers: HeaderMap,
    Query(fields): Query<HashMap<String, String>>,
) -> Response {
    if let Some(resp) = check_auth(&ctx, &headers) {
        return resp;
    }
    let cfg = &ctx.state.config;
    let Some(protocol) = supported_protocol(cfg, &fields) else {
        return err_json(
            StatusCode::NOT_FOUND,
            "protocol_not_supported",
            "third_party lookup requires a supported protocol query parameter",
        );
    };
    let realm_id = location_candidates(&protocol, &fields)
        .into_iter()
        .find(|candidate| cfg.namespaces.realm_matches(candidate));
    let exists = realm_id.is_some();
    (
        StatusCode::OK,
        Json(json!({
            "realm_id": realm_id,
            "exists": exists,
            "external_ref": external_ref_with_protocol(&protocol, &fields),
        })),
    )
        .into_response()
}

fn field_first<'a>(fields: &'a HashMap<String, String>, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        fields.get(*key).map(String::as_str).and_then(|v| {
            let t = v.trim();
            (!t.is_empty()).then_some(t)
        })
    })
}

fn supported_protocol(
    cfg: &CokretAppletConfig,
    fields: &HashMap<String, String>,
) -> Option<String> {
    let protocol = field_first(fields, &["protocol"])?;
    cfg.protocols
        .iter()
        .any(|p| p == protocol)
        .then(|| protocol.to_owned())
}

fn external_ref_with_protocol(protocol: &str, fields: &HashMap<String, String>) -> Value {
    let mut map = serde_json::Map::new();
    for (k, v) in fields {
        map.insert(k.clone(), Value::String(v.clone()));
    }
    map.entry("protocol".to_owned())
        .or_insert_with(|| Value::String(protocol.to_owned()));
    Value::Object(map)
}

fn location_candidates(protocol: &str, fields: &HashMap<String, String>) -> Vec<String> {
    let mut candidates = Vec::new();
    for key in ["realm_id", "space_id"] {
        if let Some(value) = field_first(fields, &[key]) {
            candidates.push(value.to_owned());
        }
    }
    let team = field_first(fields, &["team", "team_id", "workspace", "workspace_id"]);
    let channel = field_first(fields, &["channel", "channel_id", "room", "room_id"]);
    if let (Some(team), Some(channel)) = (team, channel) {
        candidates.push(format!("{protocol}:team:{team}:channel:{channel}"));
    }
    if let Some(channel) = channel {
        candidates.push(format!("{protocol}:channel:{channel}"));
    }
    if let Some(location) = field_first(
        fields,
        &[
            "location",
            "location_id",
            "external_id",
            "id",
            "conversation",
        ],
    ) {
        candidates.push(format!("{protocol}:location:{location}"));
    }
    candidates
}

/// Bounded FIFO event-id dedupe set (loop / retry guard).
struct EventDedupe {
    seen: HashSet<String>,
    order: std::collections::VecDeque<String>,
    max_len: usize,
}

impl EventDedupe {
    fn new(max_len: usize) -> Self {
        Self {
            seen: HashSet::new(),
            order: std::collections::VecDeque::new(),
            max_len,
        }
    }

    fn insert(&mut self, event_id: String) -> bool {
        if self.seen.contains(&event_id) {
            return false;
        }
        self.seen.insert(event_id.clone());
        self.order.push_back(event_id);
        while self.order.len() > self.max_len {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use cokret::http_signature::{
        Component, ContentDigest, ContentDigestAlgorithm, SignedRequestParts, canonical_message,
        parse_signature_input, sign_message, signing_key_from_seed,
    };
    use cokret::signatures::PublicKeyMaterial;
    use serde_json::json;

    use super::*;

    fn valid_config_with_trusted_key(public_key: Vec<u8>) -> CokretAppletConfig {
        let public_key_value =
            serde_json::to_value(PublicKeyMaterial::Ed25519Raw { bytes: public_key })
                .expect("public key should serialize");
        let settings = json!({
            "mode": "applet",
            "appletId": "ck:applet:21532600-0000-7000-8000-000000000000",
            "serviceDid": "did:web:bridge.example",
            "controllerDid": "did:webvh:example.com:admin",
            "baseUrl": "https://octos.example/applet-test",
            "botActorId": "did:web:bridge.example:bot",
            "cokretServerUrl": "https://cokret.example.org",
            "cokretServerDid": "did:webvh:cokret.example.org",
            "accessToken": "test-bearer",
            "protocols": ["slack"],
            "namespaces": {
                "actors": [{"pattern": "did:web:bridge.example:ghost:*", "exclusive": true}],
                "realms": [{"pattern": "ck:realm:*", "exclusive": true}],
                "handles": []
            },
            "trustedVerificationMethods": [{
                "verificationMethod": "did:webvh:cokret.example.org#key-1",
                "publicKey": public_key_value,
            }]
        });
        let cfg = CokretAppletConfig::from_settings("applet-test", &settings).expect("parse");
        cfg.validate().expect("validate");
        cfg
    }

    fn state_with_trusted_http_signature_key(public_key: Vec<u8>) -> Arc<AppletState> {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg = valid_config_with_trusted_key(public_key);
        AppletState::new(
            cfg,
            tmp.path().join("seq").join("applet.seq"),
            tmp.path().to_path_buf(),
        )
        .expect("state")
    }

    fn signed_transaction_headers(body: &[u8], seed: [u8; 32]) -> (Vec<(String, String)>, Vec<u8>) {
        let signing_key = signing_key_from_seed(&seed);
        let public_key = signing_key.verifying_key().to_bytes().to_vec();
        let now = chrono::Utc::now().timestamp();
        let content_digest = ContentDigest::compute(body, ContentDigestAlgorithm::Sha256);
        let signature_input = format!(
            "sig1=(\"@method\" \"@target-uri\" \"@authority\" \
             \"source-service-did\" \"destination-service-did\" \
             \"content-digest\" \"idempotency-key\");created={now};expires={};\
             keyid=\"did:webvh:cokret.example.org#key-1\";alg=\"ed25519\"",
            now + 300
        );
        let mut headers = vec![
            ("host".to_owned(), "octos.example".to_owned()),
            (
                SOURCE_SERVICE_DID_HEADER.to_owned(),
                "did:webvh:cokret.example.org".to_owned(),
            ),
            (
                DESTINATION_SERVICE_DID_HEADER.to_owned(),
                "did:web:bridge.example".to_owned(),
            ),
            (
                "content-digest".to_owned(),
                content_digest.wire_value.clone(),
            ),
            ("idempotency-key".to_owned(), "txn-1".to_owned()),
            ("signature-input".to_owned(), signature_input.clone()),
        ];
        let parsed = parse_signature_input(&signature_input).expect("signature input should parse");
        assert!(parsed.covers_all(&[
            Component::Method,
            Component::TargetUri,
            Component::Authority,
            Component::Header(SOURCE_SERVICE_DID_HEADER.to_owned()),
            Component::Header(DESTINATION_SERVICE_DID_HEADER.to_owned()),
            Component::Header("content-digest".to_owned()),
            Component::Header("idempotency-key".to_owned()),
        ]));
        let request = SignedRequestParts {
            method: "POST".to_owned(),
            target_uri: "http://octos.example/_cokret/edge/applet/transactions".to_owned(),
            authority: "octos.example".to_owned(),
            path: "/_cokret/edge/applet/transactions".to_owned(),
            headers: headers.clone(),
            body_digest: Some(content_digest.wire_value),
        };
        let message = canonical_message(&request, &parsed).expect("canonical message");
        let signature = sign_message(&message, &signing_key);
        headers.push(("signature".to_owned(), format!("sig1=:{signature}:")));
        (headers, public_key)
    }

    #[test]
    fn verifies_trusted_http_message_signature() {
        let body = serde_json::to_vec(&json!({
            "transaction_id": "txn-1",
            "source_service_did": "did:webvh:cokret.example.org",
            "events": []
        }))
        .expect("body should serialize");
        let (headers, public_key) = signed_transaction_headers(&body, [9u8; 32]);
        let state = state_with_trusted_http_signature_key(public_key);
        let verified = verify_applet_transaction_http_signature(
            &state,
            "POST",
            Some("http://octos.example/_cokret/edge/applet/transactions"),
            Some("octos.example"),
            "/_cokret/edge/applet/transactions",
            &headers,
            &body,
        )
        .expect("signature should verify")
        .expect("signature should be required");
        assert_eq!(verified.source_service_did, "did:webvh:cokret.example.org");
        assert_eq!(verified.destination_service_did, "did:web:bridge.example");
        assert_eq!(verified.key_id, "did:webvh:cokret.example.org#key-1");
        assert!(verified.content_digest.is_some());
    }

    #[test]
    fn rejects_tampered_http_message_signature_body() {
        let body = serde_json::to_vec(&json!({
            "transaction_id": "txn-1",
            "source_service_did": "did:webvh:cokret.example.org",
            "events": []
        }))
        .expect("body should serialize");
        let (headers, public_key) = signed_transaction_headers(&body, [9u8; 32]);
        let state = state_with_trusted_http_signature_key(public_key);
        let tampered = serde_json::to_vec(&json!({
            "transaction_id": "txn-1",
            "source_service_did": "did:webvh:cokret.example.org",
            "events": [{"kind":"ck.message.create"}]
        }))
        .expect("tampered body should serialize");
        let err = verify_applet_transaction_http_signature(
            &state,
            "POST",
            Some("http://octos.example/_cokret/edge/applet/transactions"),
            Some("octos.example"),
            "/_cokret/edge/applet/transactions",
            &headers,
            &tampered,
        )
        .expect_err("tampered body must fail signature verification");
        assert!(
            err.to_string().contains("content-digest") || err.to_string().contains("signature")
        );
    }
}
