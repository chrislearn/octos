//! Account-mode channel configuration.
//!
//! A Cokret channel in `account` mode logs in as one or more already-existing
//! controlled accounts (DID + bearer / DID-proof key) and exchanges
//! `ck.message.create` events with the octos agent. The config is parsed from
//! the gateway `ChannelEntry.settings` JSON object.

use std::path::PathBuf;

use eyre::{Result, WrapErr, bail};
use serde_json::Value;

use super::signer::CokretKeyRef;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CokretAccountConfig {
    pub id: String,
    pub principal_id: String,
    pub device_id: String,
    /// Bearer-mode auth: pre-issued `ck.session.grant` access token. Either
    /// this OR `key_ref` MUST be set; if both are set, `key_ref` wins at
    /// startup (`access_token` becomes a runtime cache).
    pub access_token: String,
    /// ed25519 key location for DID-proof login + event signing. When set,
    /// octos calls `AuthManager::login_did_proof` at boot to obtain the
    /// session grant rather than using a static token.
    pub key_ref: Option<CokretKeyRef>,
    /// Verification method id used by `Ed25519MoveSigner`. Defaults to
    /// `{principal_id}#key-1` when missing.
    pub verification_method: Option<String>,
    /// Cokret server DID used as `audience` for `login_did_proof`. Defaults
    /// to the channel-level `service_did` when missing.
    pub cokret_server_did: Option<String>,
    /// Server-issued one-time challenge for DID-proof session grant issuance.
    pub login_challenge: Option<String>,
    /// Path to a pre-signed `ck.capability.grant` Event JSON. When set, the
    /// `event_id` is attached as `authorization_ref` on every outbound write.
    pub grant_event_path: Option<PathBuf>,
    pub default_realm_id: Option<String>,
    pub default_flow_id: Option<String>,
    pub agent_id: Option<String>,
    pub listen: bool,
    pub send: bool,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CokretChannelConfig {
    pub id: String,
    pub base_url: String,
    pub service_did: Option<String>,
    pub accounts: Vec<CokretAccountConfig>,
}

impl CokretChannelConfig {
    /// Parse an account-mode Cokret channel from a gateway settings object.
    ///
    /// Returns `None` when the settings carry an explicit `mode` other than
    /// `account` (the applet branch owns those), or when the required
    /// `baseUrl` is missing.
    #[must_use]
    pub fn from_settings(id: &str, settings: &Value) -> Option<Self> {
        let raw = settings.as_object()?;
        if let Some(mode) = raw.get("mode").and_then(Value::as_str)
            && !mode.eq_ignore_ascii_case("account")
        {
            return None;
        }
        let base_url = first_non_empty(raw, &["baseUrl", "base_url", "homeserver", "url"])?;
        let service_did = first_non_empty(raw, &["serviceDid", "service_did"]);

        let accounts = match raw.get("accounts") {
            Some(value) => parse_accounts(value, raw),
            None => parse_accounts(&Value::Null, raw),
        };

        Some(Self {
            id: id.to_owned(),
            base_url,
            service_did,
            accounts,
        })
    }

    /// Validate that the channel has at least one usable account.
    pub fn validate(&self) -> Result<()> {
        if self.base_url.trim().is_empty() {
            bail!("Cokret channel '{}' missing base_url", self.id);
        }
        if self.accounts.is_empty() {
            bail!(
                "Cokret channel '{}' has no accounts; configure at least one controlled account",
                self.id
            );
        }
        for account in &self.accounts {
            account.validate().wrap_err_with(|| {
                format!(
                    "Cokret channel '{}' account '{}' is invalid",
                    self.id, account.id
                )
            })?;
        }
        Ok(())
    }

    /// Find an account by id.
    #[must_use]
    pub fn account(&self, account_id: &str) -> Option<&CokretAccountConfig> {
        self.accounts.iter().find(|a| a.id == account_id)
    }

    /// Pick the account that should send to the given realm. Preference:
    /// 1. An account whose `default_realm_id == realm_id`.
    /// 2. The first account with `send == true`.
    #[must_use]
    pub fn select_send_account(&self, realm_id: &str) -> Option<&CokretAccountConfig> {
        if let Some(account) = self.accounts.iter().find(|a| {
            a.send
                && a.default_realm_id
                    .as_deref()
                    .is_some_and(|r| r.eq_ignore_ascii_case(realm_id))
        }) {
            return Some(account);
        }
        self.accounts.iter().find(|a| a.send)
    }
}

impl CokretAccountConfig {
    pub fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() {
            bail!("Cokret account missing id");
        }
        if self.principal_id.trim().is_empty() {
            bail!("Cokret account '{}' missing principal_id (DID)", self.id);
        }
        if self.device_id.trim().is_empty() {
            bail!("Cokret account '{}' missing device_id", self.id);
        }
        // Either a static access_token OR a key_ref MUST be set. Both is
        // allowed; key_ref takes precedence at startup.
        if self.access_token.trim().is_empty() && self.key_ref.is_none() {
            bail!(
                "Cokret account '{}' missing both access_token and key_ref — set one of them",
                self.id
            );
        }
        if self.key_ref.is_some() {
            let Some(challenge) = self.login_challenge.as_deref().map(str::trim) else {
                bail!(
                    "Cokret account '{}' has key_ref but no login_challenge / loginChallenge",
                    self.id
                );
            };
            if challenge.is_empty() {
                bail!(
                    "Cokret account '{}' has key_ref but no login_challenge / loginChallenge",
                    self.id
                );
            }
            if challenge.len() < 16 {
                bail!(
                    "Cokret account '{}' login_challenge must be at least 16 characters",
                    self.id
                );
            }
        }
        if self.send && self.default_realm_id.is_none() {
            bail!(
                "Cokret account '{}' has send=true but no default_realm_id",
                self.id
            );
        }
        Ok(())
    }
}

fn parse_accounts(
    accounts_value: &Value,
    parent_raw: &serde_json::Map<String, Value>,
) -> Vec<CokretAccountConfig> {
    match accounts_value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| parse_account_entry(item.as_object()?))
            .collect(),
        Value::Object(_) | Value::Null => {
            // Allow single-account flat form where principal_id / access_token
            // live at the top of the channel config object.
            if let Some(account) = parse_account_entry(parent_raw) {
                vec![account]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

fn parse_account_entry(map: &serde_json::Map<String, Value>) -> Option<CokretAccountConfig> {
    let principal_id = first_non_empty(map, &["principalId", "principal_id", "did", "user_id"])?;
    let access_token = first_non_empty(
        map,
        &["accessToken", "access_token", "token", "sessionGrant"],
    )
    .unwrap_or_default();
    let key_ref = map
        .get("keyRef")
        .or_else(|| map.get("key_ref"))
        .and_then(CokretKeyRef::from_value);
    // Caller must have at least one auth path. Reject parse for accounts with
    // neither token nor key_ref; `validate()` reports the precise error.
    if access_token.is_empty() && key_ref.is_none() {
        return None;
    }
    let id = first_non_empty(map, &["id", "accountId", "account_id"])
        .unwrap_or_else(|| principal_id.clone());
    let device_id = first_non_empty(map, &["deviceId", "device_id"]).unwrap_or_default();
    let default_realm_id = first_non_empty(map, &["defaultRealmId", "default_realm_id", "realmId"]);
    let default_flow_id = first_non_empty(map, &["defaultFlowId", "default_flow_id", "flowId"]);
    let agent_id = first_non_empty(map, &["agentId", "agent_id", "agent"]);
    let verification_method = first_non_empty(
        map,
        &[
            "verificationMethod",
            "verification_method",
            "verificationMethodId",
        ],
    );
    let cokret_server_did = first_non_empty(map, &["cokretServerDid", "cokret_server_did"]);
    let login_challenge = first_non_empty(map, &["loginChallenge", "login_challenge"]);
    let grant_event_path =
        first_non_empty(map, &["grantEventPath", "grant_event_path"]).map(PathBuf::from);

    let listen = map.get("listen").and_then(Value::as_bool).unwrap_or(true);
    let send = map.get("send").and_then(Value::as_bool).unwrap_or(true);
    let scopes = parse_string_list(map.get("scopes"));

    Some(CokretAccountConfig {
        id,
        principal_id,
        device_id,
        access_token,
        key_ref,
        verification_method,
        cokret_server_did,
        login_challenge,
        grant_event_path,
        default_realm_id,
        default_flow_id,
        agent_id,
        listen,
        send,
        scopes,
    })
}

pub(super) fn first_non_empty(
    map: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<String> {
    keys.iter().find_map(|key| {
        map.get(*key).and_then(|value| {
            let text = value.as_str()?.trim();
            if text.is_empty() {
                None
            } else {
                Some(text.to_owned())
            }
        })
    })
}

pub(super) fn parse_string_list(value: Option<&Value>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    match value {
        Value::String(text) => text
            .split([',', '\n'])
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect(),
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}
