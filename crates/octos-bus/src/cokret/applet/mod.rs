//! Cokret Applet + Ghost Actor mode (≈ Matrix AppService).
//!
//! When an octos `cokret` channel is configured with `mode = "applet"`, this
//! module's types take over: the channel registers itself as a Cokret Applet,
//! hosts the inbound `POST /_cokret/edge/applet/transactions` endpoint, and
//! writes replies back as the applet bot (or a ghost actor).

pub mod config;
pub mod ghost;
pub mod http;
pub mod namespace;
pub mod outbound;
pub mod transaction;

pub use config::CokretAppletConfig;
pub use ghost::{build_external_ref, build_ghost_profile_event, mint_ghost_did};
pub use http::AppletState;
pub use namespace::{AppletNamespaces, NamespacePattern, namespace_pattern_matches};
pub use outbound::{AppletMessageRequest, build_applet_message_event, sign_outbound_event};
pub use transaction::{
    AppletDispatchSkip, AppletEventOutcome, AppletInboundCommand, classify_inbound_event,
};
