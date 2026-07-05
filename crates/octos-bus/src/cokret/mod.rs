//! Cokret v1 channel adapter.
//!
//! Lets the octos agent participate in [Cokret](https://github.com/) realms.
//! Two modes, selected by the `mode` setting:
//!
//! * **account** — log in as one or more already-existing controlled accounts
//!   (DID + bearer / DID-proof key), subscribe to realm deltas, and exchange
//!   `ck.message.create` events with the agent.
//! * **applet** — register this node as a Cokret Applet (the Matrix-AppService
//!   equivalent): host the inbound `POST /_cokret/edge/applet/transactions`
//!   endpoint and write replies back as the applet bot.
//!
//! Layered like the other channel modules:
//!
//! * [`config`] / [`applet::config`] — typed wrappers for the channel settings.
//! * [`parse`] — convert one `ck.message.create` Event into an inbound event.
//! * [`client`] — thin wrapper around `cokret_http_client::Client`.
//! * [`outbound`] / [`applet::outbound`] — build outbound `ck.message.create`.
//! * [`signer`] — load an Ed25519 signing key and attach detached-JWS proofs.

pub mod applet;
pub mod client;
pub mod config;
pub mod crypto_state;
pub mod grant;
pub mod outbound;
pub mod parse;
pub mod seq_store;
pub mod session;
pub mod signer;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use cokret_core::AccountSubscribeFrameKind;
use cokret_identifiers::{DeviceId, Did};
use eyre::{Result, WrapErr, bail, eyre};
use octos_core::{InboundMessage, MessageOrigin, OutboundMessage};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::channel::{Channel, ChannelHealth};

pub use applet::{AppletState, CokretAppletConfig};
pub use client::{CokretFrameStream, CokretHttpClient};
pub use config::{CokretAccountConfig, CokretChannelConfig};
pub use crypto_state::{
    CokretDecryptOutcome, CokretEncryptOutcome, FileCokretCryptoStore,
    extract_encrypted_payload_from_message_content,
};
pub use grant::{CokretGrant, load_and_verify_grant};
pub use outbound::{MessageCreateRequest, build_message_create_event, sign_outbound_event};
pub use parse::{
    CokretInboundEvent, CokretInboundSkipReason, CokretInboundSkippedEvent,
    parse_delta_frame_for_account,
};
pub use signer::{CokretKeyRef, load_ed25519_signer};

/// Channel routing name.
pub const CHANNEL_NAME: &str = "cokret";

const ACCOUNT_EVENT_DEDUPE_MAX: usize = 4096;

/// Separator joining `realm_id` and `flow_id` inside a routing `chat_id`.
/// Cokret ids use `:` and never `|`, so it is collision-free.
const CHAT_ID_SEP: char = '|';

/// Encode a `(realm_id, flow_id)` pair into an octos `chat_id`, so the
/// outbound dispatcher can recover both for in-flow replies.
#[must_use]
pub fn encode_chat_id(realm_id: &str, flow_id: Option<&str>) -> String {
    match flow_id {
        Some(flow) if !flow.is_empty() => format!("{realm_id}{CHAT_ID_SEP}{flow}"),
        _ => realm_id.to_owned(),
    }
}

/// Split a routing `chat_id` back into `(realm_id, flow_id?)`.
#[must_use]
pub fn decode_chat_id(chat_id: &str) -> (String, Option<String>) {
    match chat_id.split_once(CHAT_ID_SEP) {
        Some((realm, flow)) => (realm.to_owned(), Some(flow.to_owned())),
        None => (chat_id.to_owned(), None),
    }
}

/// Operating mode of one `CokretChannel` instance.
enum CokretMode {
    Account {
        config: CokretChannelConfig,
        data_dir: PathBuf,
    },
    Applet {
        state: Arc<AppletState>,
        bind_addr: String,
    },
}

/// A Cokret v1 channel (account or applet mode).
pub struct CokretChannel {
    mode: CokretMode,
    shutdown: Arc<AtomicBool>,
}

impl CokretChannel {
    /// Build an account-mode channel from a validated [`CokretChannelConfig`].
    #[must_use]
    pub fn new_account(
        config: CokretChannelConfig,
        data_dir: PathBuf,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        Self {
            mode: CokretMode::Account { config, data_dir },
            shutdown,
        }
    }

    /// Build an applet-mode channel. `seq_path` backs the restart-safe
    /// monotonic `actor_seq` allocator; `bind_addr` is the `host:port` the
    /// inbound HTTP endpoints listen on.
    pub fn new_applet(
        config: CokretAppletConfig,
        seq_path: PathBuf,
        data_dir: PathBuf,
        bind_addr: String,
        shutdown: Arc<AtomicBool>,
    ) -> Result<Self> {
        let state = AppletState::new(config, seq_path, data_dir)?;
        Ok(Self {
            mode: CokretMode::Applet { state, bind_addr },
            shutdown,
        })
    }

    async fn start_account(
        &self,
        config: &CokretChannelConfig,
        data_dir: &Path,
        inbound_tx: mpsc::Sender<InboundMessage>,
    ) -> Result<()> {
        let mut handles: Vec<JoinHandle<()>> = Vec::new();
        for account in &config.accounts {
            if !account.listen {
                continue;
            }
            let channel = config.clone();
            let account = account.clone();
            let data_dir = data_dir.to_path_buf();
            let tx = inbound_tx.clone();
            let shutdown = Arc::clone(&self.shutdown);
            handles.push(tokio::spawn(async move {
                run_account_subscribe_loop(channel, account, data_dir, tx, shutdown).await;
            }));
        }
        if handles.is_empty() {
            warn!("cokret: account channel has no listening accounts");
        }
        // Park until shutdown, then abort the per-account listeners.
        while !self.shutdown.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        for handle in handles {
            handle.abort();
        }
        Ok(())
    }
}

#[async_trait]
impl Channel for CokretChannel {
    fn name(&self) -> &str {
        CHANNEL_NAME
    }

    async fn start(&self, inbound_tx: mpsc::Sender<InboundMessage>) -> Result<()> {
        match &self.mode {
            CokretMode::Account { config, data_dir } => {
                self.start_account(config, data_dir, inbound_tx).await
            }
            CokretMode::Applet { state, bind_addr } => {
                applet::http::serve(
                    Arc::clone(state),
                    inbound_tx,
                    bind_addr,
                    Arc::clone(&self.shutdown),
                )
                .await
            }
        }
    }

    async fn send(&self, msg: &OutboundMessage) -> Result<()> {
        let (realm_id, flow_from_chat) = decode_chat_id(&msg.chat_id);
        match &self.mode {
            CokretMode::Account { config, data_dir } => {
                send_account_message(
                    config,
                    data_dir,
                    &realm_id,
                    flow_from_chat.as_deref(),
                    &msg.content,
                )
                .await
            }
            CokretMode::Applet { state, .. } => {
                let flow = flow_from_chat.ok_or_else(|| {
                    eyre!("cokret applet reply to realm '{realm_id}' requires a flow id in chat_id")
                })?;
                state.send_reply(&realm_id, &flow, &msg.content).await
            }
        }
    }

    async fn stop(&self) -> Result<()> {
        self.shutdown.store(true, Ordering::Release);
        Ok(())
    }

    async fn health_check(&self) -> Result<ChannelHealth> {
        Ok(ChannelHealth::Unknown)
    }
}

/// Build an authenticated [`CokretHttpClient`] for one account. Runs DID-proof
/// login when `key_ref` is set; otherwise a bare-bearer client.
async fn construct_account_client(
    channel: &CokretChannelConfig,
    account: &CokretAccountConfig,
) -> Result<CokretHttpClient> {
    if let Some(key_ref) = &account.key_ref {
        let vm = account
            .verification_method
            .clone()
            .unwrap_or_else(|| format!("{}#key-1", account.principal_id));
        let audience = account
            .cokret_server_did
            .clone()
            .or_else(|| channel.service_did.clone())
            .ok_or_else(|| {
                eyre!(
                    "cokret account '{}' has key_ref but no cokret_server_did or \
                     channel.service_did for login audience",
                    account.id
                )
            })?;
        let challenge = account.login_challenge.as_deref().ok_or_else(|| {
            eyre!(
                "cokret account '{}' has key_ref but no login_challenge",
                account.id
            )
        })?;
        let signer = load_ed25519_signer(key_ref, &account.principal_id, &vm)?;
        let principal = Did::new(account.principal_id.clone())
            .map_err(|err| eyre!("invalid principal_id: {err}"))?;
        let device = DeviceId::new(account.device_id.clone())
            .map_err(|err| eyre!("invalid device_id: {err}"))?;
        let (client, _session) = CokretHttpClient::login(
            &channel.base_url,
            &signer,
            principal,
            device,
            challenge,
            &audience,
        )
        .await?;
        info!("cokret: account '{}' logged in via DID-proof", account.id);
        Ok(client)
    } else {
        CokretHttpClient::new(&channel.base_url, &account.access_token)
    }
}

enum StreamOutcome {
    Reconnect,
    ResetCursor,
    Unauthorized,
    Backoff,
}

struct AccountStreamContext<'a> {
    channel: &'a CokretChannelConfig,
    account: &'a CokretAccountConfig,
    crypto_store: &'a FileCokretCryptoStore,
    inbound_tx: &'a mpsc::Sender<InboundMessage>,
    shutdown: &'a Arc<AtomicBool>,
}

async fn run_account_subscribe_loop(
    channel: CokretChannelConfig,
    account: CokretAccountConfig,
    data_dir: PathBuf,
    inbound_tx: mpsc::Sender<InboundMessage>,
    shutdown: Arc<AtomicBool>,
) {
    let mut backoff = Duration::from_secs(1);
    let mut cursor: Option<String> = None;
    let mut dedupe = EventDedupe::new(ACCOUNT_EVENT_DEDUPE_MAX);
    let crypto_store = FileCokretCryptoStore::for_account(&data_dir, &channel.id, &account.id);
    if let Err(err) =
        FileCokretCryptoStore::feature_report().and_then(|_| crypto_store.ensure_created())
    {
        warn!(
            "cokret: account '{}' crypto state unavailable at {}: {err:#}",
            account.id,
            crypto_store.path().display()
        );
    }

    let client = match construct_account_client(&channel, &account).await {
        Ok(client) => client,
        Err(err) => {
            warn!(
                "cokret: account '{}' on channel '{}' failed to construct HTTP client: {err:#}",
                account.id, channel.id
            );
            return;
        }
    };

    loop {
        if shutdown.load(Ordering::Acquire) {
            return;
        }
        let initial = cursor.is_none();
        match client
            .account_subscribe_stream(cursor.as_deref(), initial)
            .await
        {
            Ok(stream) => {
                let context = AccountStreamContext {
                    channel: &channel,
                    account: &account,
                    crypto_store: &crypto_store,
                    inbound_tx: &inbound_tx,
                    shutdown: &shutdown,
                };
                let outcome = consume_stream(stream, context, &mut cursor, &mut dedupe).await;
                match outcome {
                    StreamOutcome::Reconnect => backoff = Duration::from_secs(1),
                    StreamOutcome::ResetCursor => {
                        cursor = None;
                        dedupe.clear();
                        backoff = Duration::from_secs(1);
                    }
                    StreamOutcome::Unauthorized => {
                        warn!(
                            "cokret: account '{}' became unauthorized mid-stream — stopping",
                            account.id
                        );
                        return;
                    }
                    StreamOutcome::Backoff => sleep_with_backoff(&mut backoff).await,
                }
            }
            Err(err) => {
                warn!(
                    "cokret: subscribe call for '{}/{}' failed: {err}",
                    channel.id, account.id
                );
                sleep_with_backoff(&mut backoff).await;
            }
        }
    }
}

async fn consume_stream(
    mut stream: CokretFrameStream,
    context: AccountStreamContext<'_>,
    cursor: &mut Option<String>,
    dedupe: &mut EventDedupe,
) -> StreamOutcome {
    use futures::StreamExt;
    loop {
        if context.shutdown.load(Ordering::Acquire) {
            return StreamOutcome::Reconnect;
        }
        let frame = match stream.next().await {
            Some(Ok(frame)) => frame,
            None => return StreamOutcome::Reconnect,
            Some(Err(err)) => {
                debug!(
                    "cokret: stream read error on '{}/{}': {err}",
                    context.channel.id, context.account.id
                );
                return StreamOutcome::Backoff;
            }
        };
        if let Some(new_cursor) = frame.cursor.clone() {
            *cursor = Some(new_cursor);
        }
        match frame.kind {
            AccountSubscribeFrameKind::Delta => {
                if let Some(realms) = &frame.realms {
                    let realms_value = serde_json::to_value(realms).unwrap_or_default();
                    match context
                        .crypto_store
                        .update_realm_policies_from_sync(&realms_value)
                    {
                        Ok(updated) if updated > 0 => {
                            debug!(
                                "cokret: '{}/{}' updated {updated} realm crypto policy record(s)",
                                context.channel.id, context.account.id
                            );
                        }
                        Ok(_) => {}
                        Err(err) => warn!(
                            "cokret: '{}/{}' failed to update realm crypto policy: {err:#}",
                            context.channel.id, context.account.id
                        ),
                    }
                    let parsed = parse_delta_frame_for_account(&realms_value, context.account);
                    for skipped in parsed.skipped {
                        match skipped.reason {
                            CokretInboundSkipReason::EncryptedContent => {
                                let decrypted = try_handle_encrypted_account_skip(
                                    &skipped,
                                    context.crypto_store,
                                    context.account,
                                    context.inbound_tx,
                                )
                                .await;
                                if decrypted {
                                    continue;
                                }
                                warn!(
                                    account_id = %skipped.account_id,
                                    event_id = skipped.event_id.as_deref().unwrap_or("<unknown>"),
                                    realm_id = skipped.realm_id.as_deref().unwrap_or("<unknown>"),
                                    "cokret: encrypted account message skipped; no usable local MLS state"
                                );
                            }
                            reason => {
                                debug!(
                                    account_id = %skipped.account_id,
                                    event_id = skipped.event_id.as_deref().unwrap_or("<unknown>"),
                                    realm_id = skipped.realm_id.as_deref().unwrap_or("<unknown>"),
                                    ?reason,
                                    "cokret: account event skipped"
                                );
                            }
                        }
                    }
                    for event in parsed.events {
                        if !dedupe.insert(event.event_id.clone()) {
                            continue;
                        }
                        if dispatch_account_event(context.account, event, context.inbound_tx)
                            .await
                            .is_err()
                        {
                            return StreamOutcome::Reconnect;
                        }
                    }
                }
            }
            AccountSubscribeFrameKind::CatchupComplete => {
                debug!(
                    "cokret: '{}/{}' catchup complete",
                    context.channel.id, context.account.id
                );
            }
            AccountSubscribeFrameKind::Frontier | AccountSubscribeFrameKind::Heartbeat => {}
            AccountSubscribeFrameKind::Dropped | AccountSubscribeFrameKind::ResyncRequired => {
                warn!(
                    "cokret: '{}/{}' stream dropped/resync — resyncing",
                    context.channel.id, context.account.id
                );
                return StreamOutcome::ResetCursor;
            }
            AccountSubscribeFrameKind::Unauthorized => return StreamOutcome::Unauthorized,
            _ => {
                debug!(
                    "cokret: '{}/{}' ignored unknown account subscribe frame kind",
                    context.channel.id, context.account.id
                );
            }
        }
    }
}

/// Push one parsed inbound event onto the octos bus. Returns `Err(())` only
/// when the bus is closed (so the caller can stop the stream).
async fn dispatch_account_event(
    account: &CokretAccountConfig,
    event: CokretInboundEvent,
    inbound_tx: &mpsc::Sender<InboundMessage>,
) -> std::result::Result<(), ()> {
    let chat_id = encode_chat_id(&event.realm_id, event.flow_id.as_deref());
    let inbound = InboundMessage {
        channel: CHANNEL_NAME.to_owned(),
        sender_id: event.sender_did.clone(),
        chat_id,
        content: event.body.clone(),
        timestamp: Utc::now(),
        media: vec![],
        metadata: json!({
            "cokret_mode": "account",
            "account_id": account.id,
            "agent_id": account.agent_id,
            "realm_id": event.realm_id,
            "flow_id": event.flow_id,
            "thread_root_id": event.thread_root_id,
        }),
        message_id: Some(event.event_id),
        origin: MessageOrigin::ExternalUser,
    };
    inbound_tx.send(inbound).await.map_err(|_| {
        info!("cokret: inbound bus closed, stopping account listener");
    })
}

async fn try_handle_encrypted_account_skip(
    skipped: &CokretInboundSkippedEvent,
    crypto_store: &FileCokretCryptoStore,
    account: &CokretAccountConfig,
    inbound_tx: &mpsc::Sender<InboundMessage>,
) -> bool {
    let Some(payload) = skipped.encrypted_payload.as_ref() else {
        return false;
    };
    match crypto_store.plan_bootstrap_for_payload(
        &account.principal_id,
        &account.device_id,
        payload,
    ) {
        Ok(plan) => debug!(
            account_id = %account.id,
            group_id = %plan.group_id,
            required_epoch = plan.required_epoch,
            local_epoch = ?plan.local_epoch,
            action = ?plan.action,
            "cokret: planned crypto bootstrap for encrypted account event"
        ),
        Err(err) => warn!(
            account_id = %account.id,
            "cokret: failed to plan crypto bootstrap for encrypted account event: {err:#}"
        ),
    }

    match crypto_store.try_decrypt_content_block(payload) {
        Ok(CokretDecryptOutcome::Decrypted(content)) => {
            let Some(body) = decrypted_text_body(&content) else {
                warn!(
                    account_id = %account.id,
                    event_id = skipped.event_id.as_deref().unwrap_or("<unknown>"),
                    "cokret: decrypted encrypted account event but content is not displayable text"
                );
                return false;
            };
            let Some(event_id) = skipped.event_id.clone() else {
                return false;
            };
            let Some(realm_id) = skipped.realm_id.clone() else {
                return false;
            };
            let Some(sender_did) = skipped.sender_did.clone() else {
                return false;
            };
            dispatch_account_event(
                account,
                CokretInboundEvent {
                    account_id: skipped.account_id.clone(),
                    event_id,
                    realm_id,
                    flow_id: None,
                    sender_did,
                    body,
                    thread_root_id: None,
                },
                inbound_tx,
            )
            .await
            .is_ok()
        }
        Ok(CokretDecryptOutcome::MissingGroupState) => {
            record_account_unable_to_decrypt(
                crypto_store,
                skipped,
                payload.clone(),
                cokret::crypto_protocol::UnableToDecryptReason::NoSession,
            );
            false
        }
        Ok(CokretDecryptOutcome::UnsupportedScheme(scheme)) => {
            warn!(
                account_id = %account.id,
                event_id = skipped.event_id.as_deref().unwrap_or("<unknown>"),
                scheme,
                "cokret: encrypted account event uses unsupported encrypted payload scheme"
            );
            record_account_unable_to_decrypt(
                crypto_store,
                skipped,
                payload.clone(),
                cokret::crypto_protocol::UnableToDecryptReason::BadCiphertext,
            );
            false
        }
        Err(err) => {
            warn!(
                account_id = %account.id,
                event_id = skipped.event_id.as_deref().unwrap_or("<unknown>"),
                "cokret: failed to decrypt encrypted account event: {err:#}"
            );
            record_account_unable_to_decrypt(
                crypto_store,
                skipped,
                payload.clone(),
                cokret::crypto_protocol::UnableToDecryptReason::BadCiphertext,
            );
            false
        }
    }
}

fn record_account_unable_to_decrypt(
    crypto_store: &FileCokretCryptoStore,
    skipped: &CokretInboundSkippedEvent,
    payload: cokret_core::EncryptedPayload,
    reason: cokret::crypto_protocol::UnableToDecryptReason,
) {
    let (Some(event_id), Some(realm_id), Some(sender)) = (
        skipped.event_id.as_deref(),
        skipped.realm_id.as_deref(),
        skipped.sender_did.as_deref(),
    ) else {
        return;
    };
    if let Err(err) =
        crypto_store.record_unable_to_decrypt(event_id, realm_id, sender, payload, reason)
    {
        warn!(
            event_id,
            realm_id, "cokret: failed to persist unable-to-decrypt record: {err:#}"
        );
    }
}

fn decrypted_text_body(content: &Value) -> Option<String> {
    let block = content
        .get("content")
        .filter(|inner| inner.get("kind").is_some())
        .unwrap_or(content);
    let kind = block.get("kind").and_then(Value::as_str)?;
    if kind != "ck.content.text" {
        return None;
    }
    block
        .get("body")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|body| !body.is_empty())
        .map(str::to_owned)
}

/// Build, sign, and submit a `ck.message.create` reply as the account that
/// owns `realm_id`.
async fn send_account_message(
    config: &CokretChannelConfig,
    data_dir: &Path,
    realm_id: &str,
    flow_id: Option<&str>,
    body: &str,
) -> Result<()> {
    let Some(account) = config.select_send_account(realm_id) else {
        bail!("cokret: no send-enabled account for realm '{realm_id}'");
    };
    let flow = flow_id
        .map(str::to_owned)
        .or_else(|| account.default_flow_id.clone())
        .ok_or_else(|| {
            eyre!(
                "cokret account '{}' has no default_flow_id and chat_id carried none",
                account.id
            )
        })?;

    let client = construct_account_client(config, account).await?;
    let request = MessageCreateRequest {
        realm_id: realm_id.to_owned(),
        flow_id: flow,
        body: body.to_owned(),
        principal_id: account.principal_id.clone(),
        actor_seq: Utc::now().timestamp_millis().max(0) as u64,
        thread_root_id: None,
    };
    let mut event = build_message_create_event(&request)?;
    let crypto_store = FileCokretCryptoStore::for_account(data_dir, &config.id, &account.id);
    apply_account_outbound_encryption(&crypto_store, realm_id, &mut event)?;

    if let Some(grant_path) = &account.grant_event_path {
        let grant = load_and_verify_grant(
            grant_path,
            &account.principal_id,
            account.default_realm_id.as_deref(),
        )
        .await
        .wrap_err_with(|| {
            format!(
                "cokret account '{}' failed to load capability grant {}",
                account.id,
                grant_path.display()
            )
        })?;
        if !grant.covers_action("ck.message.create") {
            bail!(
                "cokret account '{}' capability grant {} does not cover ck.message.create",
                account.id,
                grant_path.display()
            );
        }
        event.authorization_ref = Some(grant.event_id);
    }

    if let Some(key_ref) = &account.key_ref {
        let vm = account
            .verification_method
            .clone()
            .unwrap_or_else(|| format!("{}#key-1", account.principal_id));
        let signer = load_ed25519_signer(key_ref, &account.principal_id, &vm)?;
        sign_outbound_event(&mut event, &signer, &vm)?;
    }

    let response = client.submit_event(&event).await?;
    if !response.rejected.is_empty() {
        bail!(
            "cokret: server rejected event for realm '{realm_id}': {:?}",
            response.rejected
        );
    }
    if response.accepted.is_empty() && response.duplicate.is_empty() {
        bail!(
            "cokret: server accepted no events for realm '{realm_id}' (status={:?})",
            response.status
        );
    }
    debug!(
        "cokret: submitted event to '{realm_id}': accepted={} duplicate={}",
        response.accepted.len(),
        response.duplicate.len()
    );
    Ok(())
}

fn apply_account_outbound_encryption(
    crypto_store: &FileCokretCryptoStore,
    realm_id: &str,
    event: &mut cokret_core::Event,
) -> Result<()> {
    let Some(content_block) = event.content.get("content").cloned() else {
        return Ok(());
    };
    match crypto_store.encrypt_content_block_for_realm(realm_id, &content_block)? {
        CokretEncryptOutcome::PlaintextAllowed => Ok(()),
        CokretEncryptOutcome::Encrypted(encrypted_content) => {
            let object = event
                .content
                .as_object_mut()
                .ok_or_else(|| eyre!("Cokret message content is not an object"))?;
            object.remove("content");
            object.insert("encrypted_content".to_owned(), encrypted_content);
            Ok(())
        }
        CokretEncryptOutcome::MissingRequiredGroupState { realm_id, group_id } => {
            bail!(
                "Cokret realm '{realm_id}' requires E2EE but no local MLS group state exists for group '{group_id}'"
            );
        }
    }
}

async fn sleep_with_backoff(backoff: &mut Duration) {
    tokio::time::sleep(*backoff).await;
    let next = (backoff.as_secs() * 2).min(60);
    *backoff = Duration::from_secs(next.max(1));
}

/// Bounded FIFO `event_id` dedupe set for account-mode delta replay.
struct EventDedupe {
    seen: HashSet<String>,
    order: std::collections::VecDeque<String>,
    max_len: usize,
}

impl EventDedupe {
    fn new(max_len: usize) -> Self {
        Self {
            seen: HashSet::new(),
            order: std::collections::VecDeque::new(),
            max_len,
        }
    }

    fn insert(&mut self, event_id: String) -> bool {
        if self.seen.contains(&event_id) {
            return false;
        }
        self.seen.insert(event_id.clone());
        self.order.push_back(event_id);
        while self.order.len() > self.max_len {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }
        true
    }

    fn clear(&mut self) {
        self.seen.clear();
        self.order.clear();
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn chat_id_roundtrips_with_flow() {
        let chat = encode_chat_id("ck:realm:abc", Some("ck:flow:def"));
        assert_eq!(chat, "ck:realm:abc|ck:flow:def");
        let (realm, flow) = decode_chat_id(&chat);
        assert_eq!(realm, "ck:realm:abc");
        assert_eq!(flow.as_deref(), Some("ck:flow:def"));
    }

    #[test]
    fn chat_id_roundtrips_without_flow() {
        let chat = encode_chat_id("ck:realm:abc", None);
        assert_eq!(chat, "ck:realm:abc");
        let (realm, flow) = decode_chat_id(&chat);
        assert_eq!(realm, "ck:realm:abc");
        assert_eq!(flow, None);
    }

    #[test]
    fn account_config_parses_flat_form() {
        let settings = json!({
            "mode": "account",
            "baseUrl": "http://127.0.0.1:8008",
            "principalId": "did:webvh:127.0.0.1%3A8008:agents:support",
            "deviceId": "ck:device:01904100-0000-7000-8000-000000000001",
            "accessToken": "ck.session.grant:tok",
            "defaultRealmId": "ck:realm:r1"
        });
        let cfg = CokretChannelConfig::from_settings("cokret-account", &settings).expect("parse");
        assert_eq!(cfg.accounts.len(), 1);
        assert_eq!(
            cfg.accounts[0].principal_id,
            "did:webvh:127.0.0.1%3A8008:agents:support"
        );
        cfg.validate().expect("validate");
    }

    #[test]
    fn account_config_skips_applet_mode() {
        let settings = json!({ "mode": "applet", "baseUrl": "http://x" });
        assert!(CokretChannelConfig::from_settings("x", &settings).is_none());
    }

    #[test]
    fn applet_config_parses_and_validates() {
        let settings = json!({
            "mode": "applet",
            "appletId": "ck:applet:21532600-0000-7000-8000-000000000000",
            "serviceDid": "did:web:octos-bridge.example",
            "controllerDid": "did:webvh:example.com:admin",
            "baseUrl": "https://octos.example/applet",
            "botActorId": "did:web:octos-bridge.example:bot",
            "cokretServerUrl": "http://127.0.0.1:8008",
            "accessToken": "inbound-bearer",
            "protocols": ["octos"],
            "namespaces": {
                "realms": [{ "pattern": "ck:realm:*", "exclusive": false }]
            }
        });
        let cfg = CokretAppletConfig::from_settings("cokret-applet", &settings).expect("parse");
        assert_eq!(cfg.protocols, vec!["octos"]);
        cfg.validate().expect("validate");
    }

    #[test]
    fn parse_extracts_text_message() {
        let event = json!({
            "event_id": "ck:event:1",
            "kind": "ck.message.create",
            "realm_id": "ck:realm:r1",
            "actor_id": "did:webvh:user-alice",
            "content": {
                "flow_id": "ck:flow:f1",
                "content": { "kind": "ck.content.text", "body": "hello" }
            }
        });
        let parsed = parse::extract_message_event(&event, "support").expect("parse");
        assert_eq!(parsed.body, "hello");
        assert_eq!(parsed.flow_id.as_deref(), Some("ck:flow:f1"));
    }
}
