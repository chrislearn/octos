//! Applet namespace pattern matcher.
//!
//! Delegates to the upstream SDK matcher (`cokret::namespace_pattern_matches`).
//! The local pub fn name is preserved so other code in this crate doesn't have
//! to reach across the workspace boundary.
//!
//! Grammar (Cokret spec `applet-schema.md` §2):
//! * `*` matches one segment (one or more non-separator chars).
//! * `**` matches one or more `/`-separated segments but never crosses a `:`.
//! * Literal `*` is escaped as `\*`.
//! * Separators are domain-specific: actors split on `:` only; realms and
//!   handles split on `:` and `/`.

use cokret::AppletNamespaceDomain;
use serde::{Deserialize, Serialize};

/// True iff `candidate` matches `pattern` under the applet namespace grammar
/// for `domain`.
#[must_use]
pub fn namespace_pattern_matches(
    domain: AppletNamespaceDomain,
    pattern: &str,
    candidate: &str,
) -> bool {
    cokret::namespace_pattern_matches(domain, pattern, candidate)
}

/// Pattern + exclusivity flag pair (mirrors spec `namespaces.*[]` entries).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NamespacePattern {
    pub pattern: String,
    #[serde(default)]
    pub exclusive: bool,
}

impl NamespacePattern {
    #[must_use]
    pub fn new(pattern: impl Into<String>, exclusive: bool) -> Self {
        Self {
            pattern: pattern.into(),
            exclusive,
        }
    }

    #[must_use]
    pub fn matches(&self, domain: AppletNamespaceDomain, candidate: &str) -> bool {
        namespace_pattern_matches(domain, &self.pattern, candidate)
    }
}

/// Three-axis namespace declaration block for an applet.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppletNamespaces {
    #[serde(default)]
    pub actors: Vec<NamespacePattern>,
    #[serde(default)]
    pub realms: Vec<NamespacePattern>,
    #[serde(default)]
    pub handles: Vec<NamespacePattern>,
}

impl AppletNamespaces {
    #[must_use]
    pub fn actor_matches(&self, did: &str) -> bool {
        self.actors
            .iter()
            .any(|p| p.matches(AppletNamespaceDomain::Actors, did))
    }

    #[must_use]
    pub fn realm_matches(&self, realm_id_or_alias: &str) -> bool {
        self.realms
            .iter()
            .any(|p| p.matches(AppletNamespaceDomain::Realms, realm_id_or_alias))
    }

    #[must_use]
    pub fn handle_matches(&self, handle: &str) -> bool {
        self.handles
            .iter()
            .any(|p| p.matches(AppletNamespaceDomain::Handles, handle))
    }
}
