//! Inbound transaction parsing.
//!
//! When the Cokret server pushes events via
//! `POST /_cokret/edge/applet/transactions`, the body is an
//! [`cokret_core::AppletTransactionRequestBody`]. This module converts each
//! contained Event into an octos-side [`AppletInboundCommand`] when it matches
//! the configured namespaces and looks dispatchable.

use cokret_core::Event;

use super::super::crypto_state::message_content_has_encrypted_carrier;
use super::config::CokretAppletConfig;

/// One dispatchable command extracted from an inbound applet transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppletInboundCommand {
    /// Wire `event_id` of the source event — used for dedupe + tracing.
    pub event_id: String,
    /// Realm the event was emitted in.
    pub realm_id: String,
    /// Discussion flow id, extracted from `content.flow_id` when present.
    pub flow_id: Option<String>,
    /// Sender DID (usually a native user, since the Cokret server pushes
    /// traffic destined for the applet's namespaces).
    pub sender_did: String,
    /// Extracted text body (currently only `ck.content.text` is handled).
    pub body: String,
    /// Optional thread root.
    pub thread_root_id: Option<String>,
}

/// Reason a given event was filtered out of the dispatch path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppletDispatchSkip {
    /// `actor_id` did not match the configured `namespaces.actors`.
    ActorNotInNamespace,
    /// `realm_id` did not match the configured `namespaces.realms`.
    RealmNotInNamespace,
    /// The event's `kind` is not `ck.message.create`.
    KindNotMessageCreate,
    /// The event carries encrypted content. The HTTP runtime must attempt MLS
    /// decrypt or fail closed and record unable-to-decrypt state.
    EncryptedContent,
    /// `content.kind` is not `ck.content.text`.
    ContentKindUnsupported,
    /// `content.body` is missing or empty.
    EmptyBody,
    /// Event came from the applet's own bot or one of its ghost actors —
    /// don't loop back into the agent pipeline.
    LoopbackFromApplet,
}

/// Outcome of parsing a single event from an applet transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppletEventOutcome {
    Dispatch(AppletInboundCommand),
    Skip(AppletDispatchSkip),
}

/// Decide what to do with one Event from an inbound applet transaction.
#[must_use]
pub fn classify_inbound_event(cfg: &CokretAppletConfig, event: &Event) -> AppletEventOutcome {
    // Loopback: an event signed by our own bot or one of our ghost actors
    // should not be dispatched back to the agent pipeline.
    let actor = event.actor_id.as_str();
    if actor == cfg.bot_actor_id || cfg.namespaces.actor_matches(actor) {
        return AppletEventOutcome::Skip(AppletDispatchSkip::LoopbackFromApplet);
    }

    if event.kind != "ck.message.create" {
        return AppletEventOutcome::Skip(AppletDispatchSkip::KindNotMessageCreate);
    }

    // Realm namespace filter (primary filter for portal-Realm inbound).
    let realm = event.realm_id.as_str();
    if !cfg.namespaces.realm_matches(realm) {
        return AppletEventOutcome::Skip(AppletDispatchSkip::RealmNotInNamespace);
    }

    let content = &event.content;
    if message_content_has_encrypted_carrier(content) {
        return AppletEventOutcome::Skip(AppletDispatchSkip::EncryptedContent);
    }
    let content_kind = content
        .get("content")
        .and_then(|c| c.get("kind"))
        .and_then(|k| k.as_str());
    if content_kind == Some("ck.content.encrypted") {
        return AppletEventOutcome::Skip(AppletDispatchSkip::EncryptedContent);
    }
    if content_kind != Some("ck.content.text") {
        return AppletEventOutcome::Skip(AppletDispatchSkip::ContentKindUnsupported);
    }
    let body = content
        .get("content")
        .and_then(|c| c.get("body"))
        .and_then(|b| b.as_str())
        .unwrap_or("")
        .trim()
        .to_owned();
    if body.is_empty() {
        return AppletEventOutcome::Skip(AppletDispatchSkip::EmptyBody);
    }

    let flow_id = content
        .get("flow_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let thread_root_id = content
        .get("thread_root_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);

    AppletEventOutcome::Dispatch(AppletInboundCommand {
        event_id: event.event_id.as_str().to_owned(),
        realm_id: realm.to_owned(),
        flow_id,
        sender_did: actor.to_owned(),
        body,
        thread_root_id,
    })
}
