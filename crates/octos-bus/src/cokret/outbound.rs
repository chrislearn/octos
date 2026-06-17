//! Build a `ck.message.create` Event Envelope for an account-mode actor.
//!
//! [`build_message_create_event`] leaves `proofs[]` empty; [`sign_outbound_event`]
//! wraps `cokret::signatures::sign_event`. Callers with a signer plumbed in
//! should call it after `build_message_create_event` and before
//! `CokretHttpClient::submit_event`; bearer-only callers can still submit
//! unsigned at their own risk (production servers will reject).

use cokret::Ed25519MoveSigner;
use cokret::signatures::{SignEventOptions, sign_event};
use cokret_core::{Event, EventRequirements};
use cokret_identifiers::{Did, Hlc, RealmId, new_prefixed_uuid7};
use eyre::{Result, WrapErr, bail, eyre};
use serde_json::json;

#[derive(Debug, Clone)]
pub struct MessageCreateRequest {
    pub realm_id: String,
    pub flow_id: String,
    pub body: String,
    pub principal_id: String,
    pub actor_seq: u64,
    pub thread_root_id: Option<String>,
}

/// Build an unsigned `ck.message.create` Event Envelope ready to be POSTed to
/// `/api/v1/events`.
///
/// **Caveat:** this returns the envelope with `proofs[]` empty. The server
/// will reject submission if it enforces per-event detached-JWS signing.
pub fn build_message_create_event(req: &MessageCreateRequest) -> Result<Event> {
    if req.realm_id.trim().is_empty() {
        bail!("MessageCreateRequest missing realm_id");
    }
    if req.flow_id.trim().is_empty() {
        bail!("MessageCreateRequest missing flow_id");
    }
    if req.body.trim().is_empty() {
        bail!("MessageCreateRequest has empty body");
    }
    let realm = RealmId::new(req.realm_id.clone())
        .wrap_err_with(|| format!("invalid realm_id: {}", req.realm_id))?;
    let actor = Did::new(req.principal_id.clone())
        .wrap_err_with(|| format!("invalid principal DID: {}", req.principal_id))?;
    let hlc = current_hlc();

    let mut content = json!({
        "message_id": new_prefixed_uuid7("ck:message:"),
        "flow_id": req.flow_id,
        "track": "discussion",
        "content": {
            "kind": "ck.content.text",
            "body": req.body,
        }
    });
    if let Some(thread_root) = &req.thread_root_id
        && let Some(obj) = content.as_object_mut()
    {
        obj.insert(
            "thread_root_id".into(),
            serde_json::Value::String(thread_root.clone()),
        );
    }

    let mut event = Event::new(
        "ck.message.create",
        realm,
        actor,
        req.actor_seq,
        hlc,
        content,
    )
    .map_err(|err| eyre!("failed to build event envelope: {err}"))?;

    event.requirements = EventRequirements::default();
    Ok(event)
}

/// Attach a detached-JWS [`cokret_core::Proof`] to an outbound event.
pub fn sign_outbound_event(
    event: &mut Event,
    signer: &Ed25519MoveSigner,
    verification_method: &str,
) -> Result<()> {
    sign_event(
        event,
        signer,
        verification_method,
        SignEventOptions::default(),
    )
    .map_err(|err| eyre!("sign_event failed: {err}"))?;
    Ok(())
}

pub(super) fn current_hlc() -> Hlc {
    // HLC format: `unix_ms_hex(12) - logical_hex(4) - node_hex(8)`. We don't
    // own a logical clock here, so emit `(now, 0, 00000000)` — Cokret v1
    // tolerates monotonic-by-time stamps from a single emitter.
    let unix_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let value = format!("{unix_ms:012x}-0000-00000000");
    Hlc::new(value).expect("hlc shape validated")
}
