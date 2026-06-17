//! Build outbound `ck.message.create` Event Envelopes attributed to a Ghost
//! Actor (or the applet bot).
//!
//! The current SDK envelope exposes no signed top-level `applet_id` or
//! `external_ref` slots, so this helper carries that bridge provenance in
//! `unsigned` transport metadata. The caller signs the envelope via
//! [`sign_outbound_event`] before submitting.

use cokret::Ed25519MoveSigner;
use cokret::signatures::{SignEventOptions, sign_event};
use cokret_core::Event;
use cokret_identifiers::{Did, Hlc, RealmId, new_prefixed_uuid7};
use eyre::{Result, WrapErr, bail, eyre};
use serde_json::{Value, json};

/// Inputs for an applet-attributed `ck.message.create` event.
#[derive(Debug, Clone)]
pub struct AppletMessageRequest {
    /// Stable applet id (`ck:applet:<uuidv7>`).
    pub applet_id: String,
    /// Target Realm where the message lands.
    pub realm_id: String,
    /// Target discussion Flow.
    pub flow_id: String,
    /// Ghost actor DID — the visible author of the message.
    pub ghost_actor_did: String,
    /// Message body (plain text).
    pub body: String,
    /// External-origin reference (protocol/network/external_id).
    pub external_ref: Value,
    /// `ck:grant:<uuidv7>` granting the ghost actor permission to write to
    /// this realm/flow. Set if available; omitted otherwise.
    pub authorization_ref: Option<String>,
    /// When the event is delegated (Applet acting *on behalf of* a native
    /// user), this is the executing agent / applet DID. For pure ghost writes
    /// (the ghost is the author), leave `None`.
    pub executed_by: Option<String>,
    /// Monotonic per-actor sequence number. Caller maintains this.
    pub actor_seq: u64,
    /// Optional thread root event id.
    pub thread_root_id: Option<String>,
}

/// Build an unsigned `ck.message.create` Event Envelope attributed to a Ghost
/// Actor.
pub fn build_applet_message_event(req: &AppletMessageRequest) -> Result<Event> {
    if req.applet_id.trim().is_empty() {
        bail!("AppletMessageRequest missing applet_id");
    }
    if req.realm_id.trim().is_empty() {
        bail!("AppletMessageRequest missing realm_id");
    }
    if req.flow_id.trim().is_empty() {
        bail!("AppletMessageRequest missing flow_id");
    }
    if req.body.trim().is_empty() {
        bail!("AppletMessageRequest has empty body");
    }
    if !req.ghost_actor_did.starts_with("did:") {
        bail!(
            "AppletMessageRequest ghost_actor_did must be a DID URI, got '{}'",
            req.ghost_actor_did
        );
    }
    let realm = RealmId::new(req.realm_id.clone())
        .wrap_err_with(|| format!("invalid realm_id: {}", req.realm_id))?;
    let actor = Did::new(req.ghost_actor_did.clone())
        .wrap_err_with(|| format!("invalid ghost actor DID: {}", req.ghost_actor_did))?;
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
        obj.insert("thread_root_id".into(), Value::String(thread_root.clone()));
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

    event
        .unsigned
        .insert("applet_id".to_owned(), Value::String(req.applet_id.clone()));
    event
        .unsigned
        .insert("external_ref".to_owned(), req.external_ref.clone());

    if let Some(grant) = &req.authorization_ref {
        event.authorization_ref = Some(grant.clone());
    }
    if let Some(exec) = &req.executed_by {
        event.executed_by = Some(
            Did::new(exec.clone()).wrap_err_with(|| format!("invalid executed_by DID: {exec}"))?,
        );
    }

    Ok(event)
}

/// Attach a detached-JWS [`cokret_core::Proof`] to an outbound event using the
/// supplied Ed25519 signer.
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

fn current_hlc() -> Hlc {
    let unix_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let value = format!("{unix_ms:012x}-0000-00000000");
    Hlc::new(value).expect("hlc shape validated")
}
