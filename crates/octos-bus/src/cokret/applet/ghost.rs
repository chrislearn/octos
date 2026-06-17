//! Ghost actor helpers — DID minting, profile event content, external_ref.
//!
//! A Ghost Actor is the Cokret mirror of an external network user. The ghost
//! DID uses the colon path-segment form `{service_did}:{prefix}{slug}` (e.g.
//! `did:web:slack-bridge.example:ghost:u123`) so the controlling Applet's DID
//! namespace is visible; a `#fragment` MUST NOT appear in an `actor_id`.

use cokret::ProfileCreateBuilder;
use cokret_core::Event;
use cokret_identifiers::{AppletId, Did, Hlc, RealmId};
use eyre::{Result, WrapErr, eyre};
use serde_json::{Value, json};

/// Mint a stable ghost DID for an external user.
///
/// Format: `{service_did}:{ghost_did_prefix}{slug(external_user_id)}`. The slug
/// step lowercases the external id, replaces non-alphanumeric runs with `-`,
/// and trims surrounding hyphens. If slugging yields an empty string, the raw
/// `external_user_id` is used verbatim (URL-percent-encoded).
#[must_use]
pub fn mint_ghost_did(
    applet_service_did: &str,
    ghost_did_prefix: &str,
    external_user_id: &str,
) -> String {
    let slug = slugify(external_user_id);
    let suffix = if slug.is_empty() {
        percent_encode_minimal(external_user_id)
    } else {
        slug
    };
    format!("{applet_service_did}:{ghost_did_prefix}{suffix}")
}

/// Build a `ck.profile.create` Event Envelope for a Ghost Actor.
///
/// Uses the SDK's [`ProfileCreateBuilder`] to stamp `actor_kind =
/// "integration"`, `profile_fields.managed_by_applet`,
/// `profile_fields.external_ref`, and `accountable_principal_ids`. The returned
/// Event is unsigned (`proofs: []`) — the caller attaches a `Proof` via
/// `cokret::signatures::sign_event` when an Ed25519 signer is plumbed in.
pub fn build_ghost_profile_event(
    realm_id: &str,
    ghost_did: &str,
    display_name: &str,
    applet_id: &str,
    controller_did: &str,
    external_ref: Value,
    actor_seq: u64,
) -> Result<Event> {
    let realm =
        RealmId::new(realm_id.to_owned()).wrap_err_with(|| format!("invalid realm_id: {realm_id}"))?;
    let ghost = Did::new(ghost_did.to_owned())
        .wrap_err_with(|| format!("invalid ghost actor DID: {ghost_did}"))?;
    let controller = Did::new(controller_did.to_owned())
        .wrap_err_with(|| format!("invalid controller DID: {controller_did}"))?;
    let applet = AppletId::new(applet_id.to_owned())
        .wrap_err_with(|| format!("invalid applet_id: {applet_id}"))?;
    let hlc = current_hlc();
    ProfileCreateBuilder::new(realm, ghost)
        .with_display_name(display_name)
        .with_ghost_actor_profile(applet, vec![controller])
        .with_external_ref(external_ref)
        .build(actor_seq, hlc)
        .map_err(|err| eyre!("ProfileCreateBuilder build failed: {err}"))
}

/// Build an `external_ref` object for a Ghost Actor or bridged Realm.
#[must_use]
pub fn build_external_ref(protocol: &str, network_id: &str, external_id: &str) -> Value {
    json!({
        "protocol": protocol,
        "network_id": network_id,
        "external_id": external_id,
    })
}

fn current_hlc() -> Hlc {
    let unix_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let value = format!("{unix_ms:012x}-0000-00000000");
    Hlc::new(value).expect("hlc shape validated")
}

fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_was_sep = true; // collapse leading separators
    for c in input.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('-');
            last_was_sep = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn percent_encode_minimal(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            for b in c.to_string().bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}
