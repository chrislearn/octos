//! Parse Cokret `ck.message.create` Event Envelopes into octos inbound events.
//!
//! Frame-level parsing (NDJSON `Delta` / `CatchupComplete` / etc.) is provided
//! by the Cokret SDK (`cokret_core::AccountSubscribeFrame`). This module owns
//! the "given an Event payload, decide whether to dispatch and how" logic.

use serde_json::Value;

use super::config::CokretAccountConfig;
use super::crypto_state::{
    extract_encrypted_payload_from_message_content, message_content_has_encrypted_carrier,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CokretInboundEvent {
    pub account_id: String,
    pub event_id: String,
    pub realm_id: String,
    pub flow_id: Option<String>,
    pub sender_did: String,
    pub body: String,
    pub thread_root_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CokretInboundParseResult {
    pub events: Vec<CokretInboundEvent>,
    pub skipped: Vec<CokretInboundSkippedEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CokretInboundSkippedEvent {
    pub account_id: String,
    pub event_id: Option<String>,
    pub realm_id: Option<String>,
    pub sender_did: Option<String>,
    pub encrypted_payload: Option<cokret_core::EncryptedPayload>,
    pub reason: CokretInboundSkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CokretInboundSkipReason {
    KindNotMessageCreate,
    MissingRequiredField(&'static str),
    EncryptedContent,
    UnsupportedContentKind(String),
    LoopbackFromAccount,
    RealmNotAllowed,
    EmptyBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CokretInboundEventOutcome {
    Dispatchable(CokretInboundEvent),
    Skip(CokretInboundSkippedEvent),
}

/// Extract a `ck.message.create` Event payload into an octos inbound event.
///
/// Returns `None` for non-message events, redacted/missing content, or
/// unsupported content kinds (anything other than `ck.content.text`).
#[must_use]
pub fn extract_message_event(event: &Value, account_id: &str) -> Option<CokretInboundEvent> {
    match classify_message_event(event, account_id) {
        CokretInboundEventOutcome::Dispatchable(event) => Some(event),
        CokretInboundEventOutcome::Skip(_) => None,
    }
}

/// Classify one Cokret event for the account-mode inbound path.
///
/// Encrypted payload carriers are detected explicitly and reported as
/// [`CokretInboundSkipReason::EncryptedContent`] so callers can fail closed
/// instead of silently dropping an encrypted message as an unsupported kind.
#[must_use]
pub fn classify_message_event(event: &Value, account_id: &str) -> CokretInboundEventOutcome {
    let Some(kind) = event.get("kind").and_then(Value::as_str) else {
        return skip_event(
            event,
            account_id,
            CokretInboundSkipReason::MissingRequiredField("kind"),
        );
    };
    if kind != "ck.message.create" {
        return skip_event(
            event,
            account_id,
            CokretInboundSkipReason::KindNotMessageCreate,
        );
    }
    let Some(event_id) = event.get("event_id").and_then(Value::as_str) else {
        return skip_event(
            event,
            account_id,
            CokretInboundSkipReason::MissingRequiredField("event_id"),
        );
    };
    let Some(realm_id) = event.get("realm_id").and_then(Value::as_str) else {
        return skip_event(
            event,
            account_id,
            CokretInboundSkipReason::MissingRequiredField("realm_id"),
        );
    };
    let Some(sender_did) = event.get("actor_id").and_then(Value::as_str) else {
        return skip_event(
            event,
            account_id,
            CokretInboundSkipReason::MissingRequiredField("actor_id"),
        );
    };

    let Some(content) = event.get("content") else {
        return skip_event(
            event,
            account_id,
            CokretInboundSkipReason::MissingRequiredField("content"),
        );
    };
    // `content` is the operation payload; in v1 a `ck.message.create` payload
    // wraps `{ message_id, flow_id, track, content: { kind, body, ... } }`.
    let flow_id = content
        .get("flow_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if message_content_has_encrypted_carrier(content) {
        return skip_encrypted_event(event, account_id, content);
    }
    let inner = content.get("content").unwrap_or(content);
    let Some(content_kind) = inner.get("kind").and_then(Value::as_str) else {
        return skip_event(
            event,
            account_id,
            CokretInboundSkipReason::MissingRequiredField("content.kind"),
        );
    };
    if content_kind == "ck.content.encrypted" {
        return skip_encrypted_event(event, account_id, content);
    }
    if content_kind != "ck.content.text" {
        return skip_event(
            event,
            account_id,
            CokretInboundSkipReason::UnsupportedContentKind(content_kind.to_owned()),
        );
    }
    let Some(body) = inner.get("body").and_then(Value::as_str) else {
        return skip_event(
            event,
            account_id,
            CokretInboundSkipReason::MissingRequiredField("content.body"),
        );
    };
    let thread_root_id = inner
        .get("thread_root_id")
        .and_then(Value::as_str)
        .or_else(|| content.get("thread_root_id").and_then(Value::as_str))
        .map(str::to_owned);

    CokretInboundEventOutcome::Dispatchable(CokretInboundEvent {
        account_id: account_id.to_owned(),
        event_id: event_id.to_owned(),
        realm_id: realm_id.to_owned(),
        flow_id,
        sender_did: sender_did.to_owned(),
        body: body.to_owned(),
        thread_root_id,
    })
}

/// Decide whether the parsed event should be dispatched to the agent pipeline.
///
/// Rules:
/// * Drop messages sent by the listening account itself (loop guard).
/// * Drop messages whose realm is not in the account's allow set (when the
///   account specifies one via `default_realm_id`); accounts without a
///   `default_realm_id` accept any realm.
#[must_use]
pub fn should_dispatch_event(event: &CokretInboundEvent, account: &CokretAccountConfig) -> bool {
    dispatch_skip_reason(event, account).is_none()
}

fn dispatch_skip_reason(
    event: &CokretInboundEvent,
    account: &CokretAccountConfig,
) -> Option<CokretInboundSkipReason> {
    if event.sender_did.eq_ignore_ascii_case(&account.principal_id) {
        return Some(CokretInboundSkipReason::LoopbackFromAccount);
    }
    if let Some(allowed) = account.default_realm_id.as_deref()
        && !event.realm_id.eq_ignore_ascii_case(allowed)
    {
        return Some(CokretInboundSkipReason::RealmNotAllowed);
    }
    if event.body.trim().is_empty() {
        return Some(CokretInboundSkipReason::EmptyBody);
    }
    None
}

/// Walk a `Delta` frame body's `realms` object and extract every dispatchable
/// `ck.message.create` event for the given account.
///
/// The shape expected is the v1 account subscribe `delta` shape:
/// `realms.<realm_id>.timeline.events[] : Event`. Tolerates missing nested
/// fields by returning an empty list rather than failing.
#[must_use]
pub fn parse_delta_frame_for_account(
    realms_value: &Value,
    account: &CokretAccountConfig,
) -> CokretInboundParseResult {
    let mut events = Vec::new();
    let mut skipped = Vec::new();
    let Some(realms) = realms_value.as_object() else {
        return CokretInboundParseResult { events, skipped };
    };
    for (_realm_id, realm_body) in realms {
        let timeline = realm_body
            .get("timeline")
            .and_then(|t| t.get("events"))
            .and_then(Value::as_array);
        let Some(timeline) = timeline else { continue };
        for raw_event in timeline {
            match classify_message_event(raw_event, &account.id) {
                CokretInboundEventOutcome::Dispatchable(parsed) => {
                    if let Some(reason) = dispatch_skip_reason(&parsed, account) {
                        skipped.push(CokretInboundSkippedEvent {
                            account_id: parsed.account_id.clone(),
                            event_id: Some(parsed.event_id.clone()),
                            realm_id: Some(parsed.realm_id.clone()),
                            sender_did: Some(parsed.sender_did.clone()),
                            encrypted_payload: None,
                            reason,
                        });
                    } else {
                        events.push(parsed);
                    }
                }
                CokretInboundEventOutcome::Skip(event) => skipped.push(event),
            }
        }
    }
    CokretInboundParseResult { events, skipped }
}

fn skip_event(
    event: &Value,
    account_id: &str,
    reason: CokretInboundSkipReason,
) -> CokretInboundEventOutcome {
    CokretInboundEventOutcome::Skip(CokretInboundSkippedEvent {
        account_id: account_id.to_owned(),
        event_id: event
            .get("event_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        realm_id: event
            .get("realm_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        sender_did: event
            .get("actor_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        encrypted_payload: None,
        reason,
    })
}

fn skip_encrypted_event(
    event: &Value,
    account_id: &str,
    content: &Value,
) -> CokretInboundEventOutcome {
    CokretInboundEventOutcome::Skip(CokretInboundSkippedEvent {
        account_id: account_id.to_owned(),
        event_id: event
            .get("event_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        realm_id: event
            .get("realm_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        sender_did: event
            .get("actor_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        encrypted_payload: extract_encrypted_payload_from_message_content(content),
        reason: CokretInboundSkipReason::EncryptedContent,
    })
}
