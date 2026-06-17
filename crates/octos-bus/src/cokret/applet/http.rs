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
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use chrono::Utc;
use cokret::{IdempotencyDecision, IdempotencyWindow};
use cokret_core::{
    AppletDescription, AppletPingOutcome, AppletTransactionOutcome, AppletTransactionRequestBody,
    Did, Hash, canonical,
};
use cokret_identifiers::RealmId;
use eyre::{Result, WrapErr, eyre};
use octos_core::{InboundMessage, MessageOrigin};
use serde_json::{Value, json};
use subtle::ConstantTimeEq;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::config::CokretAppletConfig;
use super::outbound::{build_applet_message_event, sign_outbound_event};
use super::transaction::{AppletEventOutcome, classify_inbound_event};
use crate::cokret::client::CokretHttpClient;
use crate::cokret::signer::load_ed25519_signer;

const TXN_DEDUPE_WINDOW: Duration = Duration::from_secs(300);
const MAX_APPLET_TRANSACTION_BODY_BYTES: usize = 65_536;
const EVENT_DEDUPE_MAX: usize = 4096;

/// Shared applet runtime state held by a `CokretChannel` in applet mode.
pub struct AppletState {
    config: CokretAppletConfig,
    idempotency: Mutex<IdempotencyWindow>,
    seq: cokret_bridge_runtime::SeqAllocator,
    /// Bounded set of recently-dispatched `event_id`s (loop / retry guard).
    event_dedupe: Mutex<EventDedupe>,
}

impl AppletState {
    /// Build the runtime state with a restart-safe monotonic `actor_seq`
    /// allocator backed by `seq_path`.
    pub fn new(config: CokretAppletConfig, seq_path: std::path::PathBuf) -> Result<Arc<Self>> {
        let store = crate::cokret::seq_store::FileSeqStore::shared(seq_path)
            .map_err(|e| eyre!("cokret applet seq store: {e}"))?;
        let key = format!("applet:{}:actor_seq", config.id);
        let seq = cokret_bridge_runtime::SeqAllocator::new(store, key);
        Ok(Arc::new(Self {
            config,
            idempotency: Mutex::new(IdempotencyWindow::new(TXN_DEDUPE_WINDOW)),
            seq,
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
            let challenge = cfg.login_challenge.as_deref().ok_or_else(|| {
                eyre!("applet '{}' has key_ref but no login_challenge", cfg.id)
            })?;
            let signer = load_ed25519_signer(key_ref, &cfg.bot_actor_id, &vm)?;
            let principal = Did::new(cfg.bot_actor_id.clone())
                .map_err(|err| eyre!("invalid bot DID: {err}"))?;
            let device = cokret_identifiers::DeviceId::new(format!(
                "ck:device:applet-{}",
                cfg.applet_id.trim_start_matches("ck:applet:")
            ))
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
                eyre!("applet '{}' has neither key_ref nor cokret_bearer_token", cfg.id)
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
        .route("/_cokret/edge/applet/transactions", post(applet_transactions))
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

/// Bearer-authenticate the request against the applet's configured token.
fn check_auth(ctx: &HttpCtx, headers: &HeaderMap) -> std::result::Result<(), Response> {
    let Some(token) = bearer_token(headers) else {
        return Err(err_json(
            StatusCode::UNAUTHORIZED,
            "missing_bearer_token",
            "Cokret applet endpoint requires Authorization: Bearer <token>",
        ));
    };
    if token_matches(ctx.state.config.cokret_bearer_token.as_deref(), &token) {
        Ok(())
    } else {
        Err(err_json(
            StatusCode::UNAUTHORIZED,
            "invalid_bearer_token",
            "Authorization token does not match this Cokret applet channel",
        ))
    }
}

// ─── Handlers ────────────────────────────────────────────────────────────────

async fn applet_ping(State(ctx): State<HttpCtx>, headers: HeaderMap) -> Response {
    if let Err(resp) = check_auth(&ctx, &headers) {
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
    if let Err(resp) = check_auth(&ctx, &headers) {
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
        }),
        auth: json!({
            "type": "bearer",
            "controller_did": cfg.controller_did,
            "bot_actor_id": cfg.bot_actor_id,
        }),
    };
    (StatusCode::OK, Json(body)).into_response()
}

async fn applet_transactions(
    State(ctx): State<HttpCtx>,
    headers: HeaderMap,
    raw: Bytes,
) -> Response {
    if let Err(resp) = check_auth(&ctx, &headers) {
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

    let source_service_did = body.source_service_did.as_str().to_owned();
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

    // Idempotency check (SDK IdempotencyWindow).
    {
        let Ok(window) = ctx.state.idempotency.lock() else {
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "state_unavailable",
                "Cokret applet runtime state unavailable",
            );
        };
        window.gc();
        match window.check(&source_service_did, &idempotency_key, &body_hash) {
            IdempotencyDecision::Fresh => {
                window.record(&source_service_did, &idempotency_key, body_hash.clone());
            }
            IdempotencyDecision::Duplicate { .. } => {
                debug!(source_service_did, idempotency_key, "applet: duplicate txn");
                return (
                    StatusCode::OK,
                    Json(AppletTransactionOutcome {
                        ok: true,
                        rejected: vec![],
                        retry_after_ms: None,
                    }),
                )
                    .into_response();
            }
            IdempotencyDecision::Conflict { .. } => {
                warn!(source_service_did, idempotency_key, "applet: idempotency conflict");
                return err_json(
                    StatusCode::CONFLICT,
                    "duplicate_conflict",
                    "Idempotency-Key already used for a different request body",
                );
            }
        }
    }

    // Classify events and dispatch the accepted ones.
    let mut rejected: Vec<Value> = Vec::new();
    let cfg = &ctx.state.config;
    for event in body.events.iter() {
        match classify_inbound_event(cfg, event) {
            AppletEventOutcome::Dispatch(cmd) => {
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
                        "applet_id": cfg.applet_id,
                    }),
                    message_id: Some(cmd.event_id.clone()),
                    origin: MessageOrigin::ExternalUser,
                };
                if ctx.inbound_tx.send(inbound).await.is_err() {
                    warn!("applet: inbound bus closed; dropping event {}", cmd.event_id);
                }
            }
            AppletEventOutcome::Skip(reason) => {
                rejected.push(json!({
                    "event_id": event.event_id.as_str(),
                    "reason_code": format!("{reason:?}"),
                }));
            }
        }
    }

    (
        StatusCode::OK,
        Json(AppletTransactionOutcome {
            ok: true,
            rejected,
            retry_after_ms: None,
        }),
    )
        .into_response()
}

async fn applet_actor(
    State(ctx): State<HttpCtx>,
    headers: HeaderMap,
    Path(actor_id): Path<String>,
) -> Response {
    if let Err(resp) = check_auth(&ctx, &headers) {
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
    if let Err(resp) = check_auth(&ctx, &headers) {
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
    if let Err(resp) = check_auth(&ctx, &headers) {
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
    if let Err(resp) = check_auth(&ctx, &headers) {
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
    let actor_id = field_first(&fields, &["actor_id", "user", "user_id", "external_id", "id"])
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
    if let Err(resp) = check_auth(&ctx, &headers) {
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

fn supported_protocol(cfg: &CokretAppletConfig, fields: &HashMap<String, String>) -> Option<String> {
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
        &["location", "location_id", "external_id", "id", "conversation"],
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
