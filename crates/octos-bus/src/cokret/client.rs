//! Thin wrapper around [`cokret_http_client::Client`] that pre-attaches a
//! bearer `ck.session.grant` access token for one Cokret account / applet bot.
//!
//! All HTTP / retry / canonical-bytes / NDJSON line splitting logic lives in
//! the upstream SDK; this type only exists so the gateway runtime never has to
//! think about constructing the underlying client.

use std::pin::Pin;

use cokret::Ed25519MoveSigner;
use cokret_core::{
    AccountSubscribeFrame, Event, EventsSubmitOutcome, ServerDescription, SyncRequestBody,
};
use cokret_http_client::{Auth, Client, ClientBuilder};
use cokret_identifiers::{DeviceId, Did};
use eyre::{Result, WrapErr, eyre};
use futures::Stream;
use url::Url;

use super::session::{CokretSession, login_with_signer};

#[derive(Clone)]
pub struct CokretHttpClient {
    inner: Client,
}

/// Stream of [`AccountSubscribeFrame`] yielded by
/// [`CokretHttpClient::account_subscribe_stream`].
pub type CokretFrameStream = Pin<Box<dyn Stream<Item = Result<AccountSubscribeFrame>> + Send>>;

impl CokretHttpClient {
    /// Build a new HTTP client bound to `base_url`, authenticated via the
    /// given bearer access token (typically a `ck.session.grant`).
    pub fn new(base_url: &str, access_token: &str) -> Result<Self> {
        let url = Url::parse(base_url)
            .wrap_err_with(|| format!("invalid Cokret base_url: {base_url}"))?;
        let inner = ClientBuilder::new(url)
            .auth(Auth::Bearer(access_token.to_owned()))
            .build()
            .map_err(|err| eyre!("failed to build Cokret HTTP client: {err}"))?;
        Ok(Self { inner })
    }

    #[must_use]
    pub fn inner(&self) -> &Client {
        &self.inner
    }

    /// Construct a client by running DID-proof login.
    ///
    /// Builds an unauthenticated underlying `Client`, runs
    /// `AuthManager::login_did_proof` to obtain a session grant, then rebuilds
    /// the authenticated `Client` carrying the `Authorization: Bearer <grant>`
    /// header. Returns both the wrapped client and the [`CokretSession`].
    pub async fn login(
        base_url: &str,
        signer: &Ed25519MoveSigner,
        principal_did: Did,
        device_id: DeviceId,
        challenge: &str,
        audience: &str,
    ) -> Result<(Self, CokretSession)> {
        let url = Url::parse(base_url)
            .wrap_err_with(|| format!("invalid Cokret base_url: {base_url}"))?;
        let bootstrap = ClientBuilder::new(url.clone())
            .build()
            .map_err(|err| eyre!("bootstrap HTTP client: {err}"))?;
        let session = login_with_signer(
            &bootstrap,
            signer,
            principal_did,
            device_id,
            challenge,
            audience,
        )
        .await?;
        let inner = ClientBuilder::new(url)
            .auth(Auth::Bearer(session.session_grant.clone()))
            .build()
            .map_err(|err| eyre!("authenticated HTTP client: {err}"))?;
        Ok((Self { inner }, session))
    }

    /// Re-bind the bearer token in-place (after a re-login).
    pub fn refresh_bearer(&mut self, base_url: &str, access_token: &str) -> Result<()> {
        let url = Url::parse(base_url)
            .wrap_err_with(|| format!("invalid Cokret base_url: {base_url}"))?;
        self.inner = ClientBuilder::new(url)
            .auth(Auth::Bearer(access_token.to_owned()))
            .build()
            .map_err(|err| eyre!("re-authenticated HTTP client: {err}"))?;
        Ok(())
    }

    /// `GET /api/v1/server/describe` — used at startup to verify the target
    /// server and pin the service DID.
    pub async fn server_describe(&self) -> Result<ServerDescription> {
        self.inner
            .describe()
            .await
            .map_err(|err| eyre!(err.to_string()))
    }

    /// `GET /api/v1/account/subscribe` — returns a [`CokretFrameStream`] that
    /// yields fully-parsed `AccountSubscribeFrame` items.
    pub async fn account_subscribe_stream(
        &self,
        after: Option<&str>,
        catchup: bool,
    ) -> Result<CokretFrameStream> {
        use futures::StreamExt;

        let req = SyncRequestBody {
            after: after.map(str::to_owned),
            catchup: Some(catchup),
            filter: None,
            set_presence: None,
            subscriptions: None,
            wait_for: None,
        };
        let stream = self
            .inner
            .account_subscribe_frames(&req)
            .await
            .map_err(|err| eyre!("cokret account_subscribe_frames: {err}"))?;
        let mapped = stream.map(|item| item.map_err(|err| eyre!("frame: {err}")));
        Ok(Box::pin(mapped))
    }

    /// `POST /api/v1/events` — submit one signed Event Envelope.
    pub async fn submit_event(&self, event: &Event) -> Result<EventsSubmitOutcome> {
        self.inner
            .events_submit(event)
            .await
            .map_err(|err| eyre!(err.to_string()))
    }
}
