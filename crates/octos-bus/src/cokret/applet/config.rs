//! Applet mode configuration.
//!
//! A Cokret channel saved with `mode = "applet"` declares this octos node as a
//! registered Applet (the Matrix-AppService equivalent). The config carries
//! applet identity, the service URL where this node receives
//! `POST /_cokret/edge/applet/transactions`, the Cokret server we write events
//! back to, namespace declarations, and a ghost-DID generation rule.

use std::path::PathBuf;

use cokret::signatures::PublicKeyMaterial;
use cokret_identifiers::{DeviceId, Did};
use eyre::{Result, bail};
use serde_json::Value;

use super::namespace::{AppletNamespaces, NamespacePattern};
use crate::cokret::config::{first_non_empty, parse_string_list};
use crate::cokret::signer::CokretKeyRef;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CokretAppletTrustedVerificationMethod {
    pub verification_method: String,
    pub public_key: PublicKeyMaterial,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CokretAppletConfig {
    /// Stable identifier in octos's local channel store.
    pub id: String,
    /// `ck:applet:<uuidv7>` — stable across registrations.
    pub applet_id: String,
    /// Applet service DID (e.g. `did:web:slack-bridge.example`).
    pub service_did: String,
    /// Controller DID that signs the registration (typically `did:webvh:...`).
    pub controller_did: String,
    /// Public URL where this octos node accepts inbound transactions.
    pub base_url: String,
    /// Bot actor DID — the visible identity of the applet in Realms it joins
    /// (usually `<service_did>:bot`).
    pub bot_actor_id: String,
    /// Optional Cokret device id for the bot/applet local MLS member.
    pub device_id: Option<String>,
    /// Cokret server base URL where outbound events are POSTed.
    pub cokret_server_url: String,
    /// Cokret server service DID used as DID-proof login audience.
    pub cokret_server_did: Option<String>,
    /// Static server verification methods accepted for inbound applet HTTP
    /// Message Signatures and event pushes.
    pub trusted_verification_methods: Vec<CokretAppletTrustedVerificationMethod>,
    /// Server-issued one-time challenge for DID-proof session grant issuance.
    pub login_challenge: Option<String>,
    /// Bearer for inbound transaction authentication (and, when `key_ref` is
    /// absent, outbound `events_submit` calls).
    pub cokret_bearer_token: Option<String>,
    /// Namespaces declared in the registration. Used for inbound transaction
    /// filtering and for actor / realm lookup endpoints.
    pub namespaces: AppletNamespaces,
    /// External protocols this Applet bridges (`["slack"]`, `["discord"]`, ...).
    pub protocols: Vec<String>,
    /// Prefix to prepend when minting ghost DIDs (colon path-segment form).
    pub ghost_did_prefix: String,
    /// `requested_scopes[]` — informational.
    pub requested_scopes: Vec<String>,
    /// Whether the Cokret server is expected to push event transactions.
    pub receive_events: bool,
    /// Whether to receive ephemeral (typing/presence) events.
    pub receive_ephemeral: bool,
    /// Whether the server is permitted to rate-limit transaction pushes.
    pub rate_limited: bool,
    /// Optional `ck.capability.grant` event id this applet currently holds.
    pub authorization_grant_id: Option<String>,
    /// Operator-supplied security epoch hash over the registration evidence.
    pub registration_epoch: Option<String>,
    /// ed25519 key for DID-proof login + event signing.
    pub key_ref: Option<CokretKeyRef>,
    /// Verification method id used by the signer. Defaults to
    /// `{bot_actor_id}#key-1` when missing.
    pub verification_method: Option<String>,
    /// Path to a pre-signed `ck.capability.grant` Event JSON.
    pub grant_event_path: Option<PathBuf>,
}

impl CokretAppletConfig {
    /// Parse an octos channel settings object as an Applet-mode Cokret channel.
    /// Returns `None` if the settings are missing the `mode == "applet"`
    /// discriminator or a required identity field.
    #[must_use]
    pub fn from_settings(id: &str, settings: &Value) -> Option<Self> {
        let raw = settings.as_object()?;
        let mode = raw
            .get("mode")
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if mode != "applet" {
            return None;
        }

        let applet_id = first_non_empty(raw, &["appletId", "applet_id"])?;
        let service_did = first_non_empty(raw, &["serviceDid", "service_did"])?;
        let controller_did = first_non_empty(raw, &["controllerDid", "controller_did"])?;
        let base_url = first_non_empty(raw, &["baseUrl", "base_url"])?;
        let bot_actor_id = first_non_empty(raw, &["botActorId", "bot_actor_id"])
            .unwrap_or_else(|| format!("{service_did}:bot"));
        let device_id = first_non_empty(raw, &["deviceId", "device_id", "botDeviceId"]);
        let cokret_server_url =
            first_non_empty(raw, &["cokretServerUrl", "cokret_server_url", "homeserver"])
                .unwrap_or_else(|| base_url.clone());
        let cokret_server_did = first_non_empty(
            raw,
            &[
                "cokretServerDid",
                "cokret_server_did",
                "trustedServerDid",
                "trusted_server_did",
            ],
        );
        let trusted_verification_methods = parse_trusted_verification_methods(
            raw.get("trustedVerificationMethods")
                .or_else(|| raw.get("trusted_verification_methods")),
        )?;
        let login_challenge = first_non_empty(raw, &["loginChallenge", "login_challenge"]);
        let cokret_bearer_token =
            first_non_empty(raw, &["accessToken", "access_token", "cokretBearerToken"]);

        let namespaces = parse_namespaces(raw.get("namespaces"));
        let protocols = parse_string_list(raw.get("protocols"));
        let requested_scopes =
            parse_string_list(raw.get("requestedScopes").or(raw.get("requested_scopes")));
        let ghost_did_prefix = first_non_empty(raw, &["ghostDidPrefix", "ghost_did_prefix"])
            .unwrap_or_else(|| "ghost:".to_owned());

        let receive_events = raw
            .get("receiveEvents")
            .or(raw.get("receive_events"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let receive_ephemeral = raw
            .get("receiveEphemeral")
            .or(raw.get("receive_ephemeral"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let rate_limited = raw
            .get("rateLimited")
            .or(raw.get("rate_limited"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let authorization_grant_id = first_non_empty(
            raw,
            &["authorizationGrantId", "authorization_grant_id", "grantId"],
        );
        let registration_epoch = first_non_empty(raw, &["registrationEpoch", "registration_epoch"]);
        let key_ref = raw
            .get("keyRef")
            .or_else(|| raw.get("key_ref"))
            .and_then(CokretKeyRef::from_value);
        let verification_method = first_non_empty(
            raw,
            &[
                "verificationMethod",
                "verification_method",
                "verificationMethodId",
            ],
        );
        let grant_event_path =
            first_non_empty(raw, &["grantEventPath", "grant_event_path"]).map(PathBuf::from);

        Some(Self {
            id: id.to_owned(),
            applet_id,
            service_did,
            controller_did,
            base_url,
            bot_actor_id,
            device_id,
            cokret_server_url,
            cokret_server_did,
            trusted_verification_methods,
            login_challenge,
            cokret_bearer_token,
            namespaces,
            protocols,
            ghost_did_prefix,
            requested_scopes,
            receive_events,
            receive_ephemeral,
            rate_limited,
            authorization_grant_id,
            registration_epoch,
            key_ref,
            verification_method,
            grant_event_path,
        })
    }

    /// Validate required fields are non-empty and DID-typed fields parse.
    pub fn validate(&self) -> Result<()> {
        for (label, value) in [
            ("applet_id", &self.applet_id),
            ("service_did", &self.service_did),
            ("controller_did", &self.controller_did),
            ("base_url", &self.base_url),
            ("bot_actor_id", &self.bot_actor_id),
            ("cokret_server_url", &self.cokret_server_url),
        ] {
            if value.trim().is_empty() {
                bail!("Cokret applet channel '{}' missing {label}", self.id);
            }
        }
        for (label, value) in [
            ("service_did", &self.service_did),
            ("controller_did", &self.controller_did),
            ("bot_actor_id", &self.bot_actor_id),
        ] {
            Did::new(value.clone()).map_err(|err| {
                eyre::eyre!(
                    "Cokret applet channel '{}' {label} must be a valid DID URI, got '{}': {err}",
                    self.id,
                    value
                )
            })?;
        }
        if let Some(device_id) = self.device_id.as_deref() {
            DeviceId::new(device_id.to_owned()).map_err(|err| {
                eyre::eyre!(
                    "Cokret applet channel '{}' device_id must be a valid Cokret device id, got '{}': {err}",
                    self.id,
                    device_id
                )
            })?;
        }
        if let Some(value) = self.cokret_server_did.as_deref() {
            Did::new(value.to_owned()).map_err(|err| {
                eyre::eyre!(
                    "Cokret applet channel '{}' cokret_server_did must be a valid DID URI, got '{}': {err}",
                    self.id,
                    value
                )
            })?;
        } else if self.key_ref.is_some() {
            bail!(
                "Cokret applet channel '{}' has key_ref but no cokret_server_did / cokretServerDid for DID-proof audience",
                self.id
            );
        }
        for method in &self.trusted_verification_methods {
            if method.verification_method.trim().is_empty() {
                bail!(
                    "Cokret applet channel '{}' has an empty trusted verification method id",
                    self.id
                );
            }
            let owner_did = verification_method_did(&method.verification_method).ok_or_else(|| {
                eyre::eyre!(
                    "Cokret applet channel '{}' trusted verification method '{}' must include a DID fragment",
                    self.id,
                    method.verification_method
                )
            })?;
            if let Some(server_did) = self.cokret_server_did.as_deref()
                && owner_did != server_did
            {
                bail!(
                    "Cokret applet channel '{}' trusted verification method '{}' is owned by '{}', not trusted server DID '{}'",
                    self.id,
                    method.verification_method,
                    owner_did,
                    server_did
                );
            }
            method.public_key.ed25519_bytes().map_err(|err| {
                eyre::eyre!(
                    "Cokret applet channel '{}' trusted verification method '{}' public key is not valid Ed25519 material: {err}",
                    self.id,
                    method.verification_method
                )
            })?;
        }
        if self.key_ref.is_some() {
            let Some(challenge) = self.login_challenge.as_deref().map(str::trim) else {
                bail!(
                    "Cokret applet channel '{}' has key_ref but no login_challenge / loginChallenge",
                    self.id
                );
            };
            if challenge.is_empty() {
                bail!(
                    "Cokret applet channel '{}' has key_ref but no login_challenge / loginChallenge",
                    self.id
                );
            }
            if challenge.len() < 16 {
                bail!(
                    "Cokret applet channel '{}' login_challenge must be at least 16 characters",
                    self.id
                );
            }
        }
        if self.namespaces.actors.is_empty()
            && self.namespaces.realms.is_empty()
            && self.namespaces.handles.is_empty()
        {
            bail!(
                "Cokret applet channel '{}' declares no namespaces; at least one of \
                 actors/realms/handles is required",
                self.id
            );
        }
        if self.protocols.is_empty() {
            bail!(
                "Cokret applet channel '{}' declares no protocols (e.g. [\"slack\"])",
                self.id
            );
        }
        if self
            .cokret_bearer_token
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
        {
            bail!(
                "Cokret applet channel '{}' missing access_token / cokretBearerToken for inbound applet authentication",
                self.id
            );
        }
        Ok(())
    }
}

fn parse_namespaces(value: Option<&Value>) -> AppletNamespaces {
    let Some(Value::Object(obj)) = value else {
        return AppletNamespaces::default();
    };
    AppletNamespaces {
        actors: parse_pattern_list(obj.get("actors")),
        realms: parse_pattern_list(obj.get("realms")),
        handles: parse_pattern_list(obj.get("handles")),
    }
}

fn parse_pattern_list(value: Option<&Value>) -> Vec<NamespacePattern> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let obj = item.as_object()?;
            let pattern = obj.get("pattern").and_then(Value::as_str)?.trim();
            if pattern.is_empty() {
                return None;
            }
            let exclusive = obj
                .get("exclusive")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Some(NamespacePattern::new(pattern.to_owned(), exclusive))
        })
        .collect()
}

fn parse_trusted_verification_methods(
    value: Option<&Value>,
) -> Option<Vec<CokretAppletTrustedVerificationMethod>> {
    let Some(value) = value else {
        return Some(Vec::new());
    };
    let items = value.as_array()?;
    items
        .iter()
        .map(parse_trusted_verification_method)
        .collect()
}

fn parse_trusted_verification_method(
    value: &Value,
) -> Option<CokretAppletTrustedVerificationMethod> {
    let obj = value.as_object()?;
    let verification_method = first_non_empty(
        obj,
        &[
            "verificationMethod",
            "verification_method",
            "verificationMethodId",
        ],
    )?;
    let public_key = if let Some(value) = obj.get("publicKey").or_else(|| obj.get("public_key")) {
        serde_json::from_value(value.clone()).ok()?
    } else if let Some(value) = obj.get("publicKeyJwk") {
        PublicKeyMaterial::Jwk {
            value: value.clone(),
        }
    } else {
        PublicKeyMaterial::Ed25519Multibase {
            value: obj.get("publicKeyMultibase")?.as_str()?.to_owned(),
        }
    };
    Some(CokretAppletTrustedVerificationMethod {
        verification_method,
        public_key,
    })
}

fn verification_method_did(verification_method: &str) -> Option<&str> {
    verification_method
        .rsplit_once('#')
        .map(|(did, _)| did)
        .filter(|did| !did.is_empty())
}
