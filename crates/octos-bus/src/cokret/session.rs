//! DID-proof login + session grant tracking.
//!
//! Wraps SDK `AuthManager::login_did_proof` so the gateway runtime can boot a
//! Cokret account / applet bot from a signer instead of a static
//! `access_token`. Returns a [`CokretSession`] that tracks `expires_at` so
//! callers can decide when to refresh.

use chrono::{DateTime, Utc};
use cokret::{AuthManager, Ed25519MoveSigner};
use cokret_http_client::Client;
use cokret_identifiers::{DeviceId, Did};
use eyre::{Result, WrapErr, eyre};

/// One-shot session state produced by [`login_with_signer`].
#[derive(Debug, Clone)]
pub struct CokretSession {
    pub session_grant: String,
    pub expires_at: DateTime<Utc>,
    pub principal_did: Did,
    pub device_id: Option<DeviceId>,
}

impl CokretSession {
    /// True if the session has expired (or is within `skew_secs` seconds of
    /// expiring). Callers MUST re-login before the window closes.
    #[must_use]
    pub fn is_near_expiry(&self, skew_secs: i64) -> bool {
        let now = Utc::now();
        let threshold = self.expires_at - chrono::Duration::seconds(skew_secs);
        now >= threshold
    }
}

/// Run `AuthManager::login_did_proof` against the given HTTP client.
///
/// `audience` is the Cokret server's service DID.
pub async fn login_with_signer(
    http: &Client,
    signer: &Ed25519MoveSigner,
    principal_did: Did,
    device_id: DeviceId,
    challenge: &str,
    audience: &str,
) -> Result<CokretSession> {
    let mut auth = AuthManager::default();
    let session = auth
        .login_did_proof(
            http,
            principal_did.clone(),
            device_id.clone(),
            signer,
            challenge,
            audience,
        )
        .await
        .map_err(|err| eyre!("login_did_proof failed: {err}"))
        .wrap_err_with(|| {
            format!(
                "cokret login_did_proof: principal={} audience={}",
                principal_did.as_str(),
                audience
            )
        })?;
    Ok(CokretSession {
        session_grant: session.session_grant,
        expires_at: session.expires_at,
        principal_did: session.principal_id,
        device_id: session.device_id,
    })
}
