//! Capability grant loading + validation.
//!
//! Load a pre-signed `ck.capability.grant` Event JSON from disk, sanity-check
//! it, and surface the `event_id` for use as `authorization_ref` on outbound
//! writes. Grants are issued by Realm admins out-of-band; octos doesn't
//! request / renew them.

use std::path::Path;

use chrono::{DateTime, Utc};
use cokret_core::{CapabilityGrant, CapabilitySubject, Event};
use eyre::{Result, WrapErr, bail, eyre};
use serde_json::Value;

/// Loaded capability grant ready for use as `authorization_ref` on outbound
/// writes.
#[derive(Debug, Clone)]
pub struct CokretGrant {
    /// Event id of the `ck.capability.grant` Event — value goes into
    /// `Event.authorization_ref` on every outbound write.
    pub event_id: String,
    /// Grant `subject` field — must match the writer's `actor_id`.
    pub subject: String,
    /// Grant `issuer` field.
    pub issuer: String,
    /// Optional Realm scope.
    pub realm_id: Option<String>,
    /// Authorized actions (e.g. `["ck.message.create"]`).
    pub actions: Vec<String>,
    /// Expiry, if any.
    pub expires_at: Option<DateTime<Utc>>,
}

impl CokretGrant {
    /// True if the grant has not expired (or has no expiry).
    #[must_use]
    pub fn is_active(&self) -> bool {
        match &self.expires_at {
            Some(t) => Utc::now() < *t,
            None => true,
        }
    }

    /// True if the grant covers the given action.
    #[must_use]
    pub fn covers_action(&self, action: &str) -> bool {
        self.actions.iter().any(|a| a == action)
    }
}

/// Load + verify a `ck.capability.grant` Event JSON file.
pub async fn load_and_verify_grant(
    path: &Path,
    expected_subject: &str,
    expected_realm: Option<&str>,
) -> Result<CokretGrant> {
    let bytes = tokio::fs::read(path)
        .await
        .wrap_err_with(|| format!("read capability grant {}", path.display()))?;
    let event: Event = serde_json::from_slice(&bytes)
        .wrap_err_with(|| format!("parse capability grant {}", path.display()))?;

    if event.kind != "ck.capability.grant" {
        bail!(
            "capability grant {}: kind must be 'ck.capability.grant', got '{}'",
            path.display(),
            event.kind
        );
    }

    // Proof binding (digest-content tie). Real cryptographic signature
    // verification (issuer DID document lookup) is still out of scope here, but
    // unsigned or dev-proof grants must not be accepted.
    if event.proofs.is_empty() {
        bail!("capability grant {}: missing proofs", path.display());
    }
    for proof in &event.proofs {
        proof
            .validate_production()
            .map_err(|err| eyre!("grant proof is not production-grade: {err}"))?;
    }
    event
        .validate_proof_bindings()
        .map_err(|err| eyre!("grant proof binding invalid: {err}"))?;

    let grant: CapabilityGrant = decode_capability_grant(event.content.clone())
        .wrap_err_with(|| format!("decode CapabilityGrant content in {}", path.display()))?;

    if event.actor_id.as_str() != grant.issuer.as_str() {
        bail!(
            "capability grant {}: event actor '{}' does not match grant issuer '{}'",
            path.display(),
            event.actor_id.as_str(),
            grant.issuer.as_str()
        );
    }
    let issuer_vm_prefix = format!("{}#", grant.issuer.as_str());
    if !event.proofs.iter().any(|proof| {
        proof.verification_method == grant.issuer.as_str()
            || proof.verification_method.starts_with(&issuer_vm_prefix)
    }) {
        bail!(
            "capability grant {}: no proof verification_method belongs to issuer '{}'",
            path.display(),
            grant.issuer.as_str()
        );
    }

    let subject = capability_subject_did(&grant.subject).ok_or_else(|| {
        eyre!(
            "capability grant {}: subject must be a DID to match expected '{}'",
            path.display(),
            expected_subject
        )
    })?;

    if subject != expected_subject {
        bail!(
            "capability grant {}: subject '{}' does not match expected '{}'",
            path.display(),
            subject,
            expected_subject
        );
    }

    let realm_id = grant.realm_id.as_ref().map(|s| s.as_str().to_owned());
    if let Some(expected) = expected_realm {
        match &realm_id {
            Some(actual) if actual.eq_ignore_ascii_case(expected) => {}
            Some(actual) => bail!(
                "capability grant {}: realm '{}' does not match expected '{}'",
                path.display(),
                actual,
                expected
            ),
            None => bail!(
                "capability grant {}: no realm scope, expected '{}'",
                path.display(),
                expected
            ),
        }
    }

    if let Some(exp) = grant.expires_at
        && Utc::now() >= exp
    {
        bail!("capability grant {}: expired at {}", path.display(), exp);
    }

    Ok(CokretGrant {
        event_id: event.event_id.as_str().to_owned(),
        subject: subject.to_owned(),
        issuer: grant.issuer.as_str().to_owned(),
        realm_id,
        actions: grant.actions,
        expires_at: grant.expires_at,
    })
}

fn decode_capability_grant(content: Value) -> Result<CapabilityGrant> {
    if let Some(grant) = content.get("grant") {
        return serde_json::from_value(grant.clone())
            .wrap_err("decode wrapped capability_grant_payload.grant");
    }
    serde_json::from_value(content).wrap_err("decode legacy direct CapabilityGrant")
}

fn capability_subject_did(subject: &CapabilitySubject) -> Option<&str> {
    match subject {
        CapabilitySubject::Did(did) => Some(did.as_str()),
        CapabilitySubject::Selector(_) => None,
    }
}
