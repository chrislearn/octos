//! Parse Cokret `ck.message.create` Event Envelopes into octos inbound events.
//!
//! Frame-level parsing (NDJSON `Delta` / `CatchupComplete` / etc.) is provided
//! by the Cokret SDK (`cokret_core::AccountSubscribeFrame`). This module owns
//! the "given an Event payload, decide whether to dispatch and how" logic.

use serde_json::Value;

use super::config::CokretAccountConfig;

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
}

/// Extract a `ck.message.create` Event payload into an octos inbound event.
///
/// Returns `None` for non-message events, redacted/missing content, or
/// unsupported content kinds (anything other than `ck.content.text`).
#[must_use]
pub fn extract_message_event(event: &Value, account_id: &str) -> Option<CokretInboundEvent> {
    let kind = event.get("kind").and_then(Value::as_str)?;
    if kind != "ck.message.create" {
        return None;
    }
    let event_id = event.get("event_id").and_then(Value::as_str)?.to_owned();
    let realm_id = event.get("realm_id").and_then(Value::as_str)?.to_owned();
    let sender_did = event.get("actor_id").and_then(Value::as_str)?.to_owned();

    let content = event.get("content")?;
    // `content` is the operation payload; in v1 a `ck.message.create` payload
    // wraps `{ message_id, flow_id, track, content: { kind, body, ... } }`.
    let flow_id = content
        .get("flow_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let inner = content.get("content").unwrap_or(content);
    let content_kind = inner.get("kind").and_then(Value::as_str)?;
    if content_kind != "ck.content.text" {
        return None;
    }
    let body = inner.get("body").and_then(Value::as_str)?.to_owned();
    let thread_root_id = inner
        .get("thread_root_id")
        .and_then(Value::as_str)
        .or_else(|| content.get("thread_root_id").and_then(Value::as_str))
        .map(str::to_owned);

    Some(CokretInboundEvent {
        account_id: account_id.to_owned(),
        event_id,
        realm_id,
        flow_id,
        sender_did,
        body,
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
    if event.sender_did.eq_ignore_ascii_case(&account.principal_id) {
        return false;
    }
    if let Some(allowed) = account.default_realm_id.as_deref()
        && !event.realm_id.eq_ignore_ascii_case(allowed)
    {
        return false;
    }
    !event.body.trim().is_empty()
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
    let Some(realms) = realms_value.as_object() else {
        return CokretInboundParseResult { events };
    };
    for (_realm_id, realm_body) in realms {
        let timeline = realm_body
            .get("timeline")
            .and_then(|t| t.get("events"))
            .and_then(Value::as_array);
        let Some(timeline) = timeline else { continue };
        for raw_event in timeline {
            let Some(parsed) = extract_message_event(raw_event, &account.id) else {
                continue;
            };
            if should_dispatch_event(&parsed, account) {
                events.push(parsed);
            }
        }
    }
    CokretInboundParseResult { events }
}
