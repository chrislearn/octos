//! Load an Ed25519 signer for a Cokret principal / applet bot.
//!
//! The 32-byte ed25519 seed is loaded from a [`CokretKeyRef`] location (env
//! var, file, or — debug only — inline base64). The resulting
//! [`cokret::Ed25519MoveSigner`] then drives both DID-proof login
//! (`AuthManager::login_did_proof`) and event signing
//! (`cokret::signatures::sign_event`).
//!
//! Security: seed material is wiped with `zeroize` after the signer is
//! constructed. Callers MUST NOT log [`CokretKeyRef`] variants directly —
//! those contain or point at secret material.

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use cokret::Ed25519MoveSigner;
use cokret_identifiers::Did;
use eyre::{Result, WrapErr, bail};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// How to find the ed25519 seed for a Cokret principal / applet bot.
///
/// Tagged on `kind` so the JSON config form is:
/// ```jsonc
/// { "kind": "env", "var": "OCTOS_COKRET_BOT_KEY" }
/// { "kind": "file", "path": "/var/secrets/octos/cokret.seed" }
/// { "kind": "inline_seed_base64", "value": "..." }   // debug only
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CokretKeyRef {
    /// Read base64-no-pad-encoded 32-byte ed25519 seed from named env var.
    Env { var: String },
    /// Read 32-byte ed25519 seed from file. Accepts:
    /// * raw 32 bytes (binary, file len == 32), or
    /// * UTF-8 text holding base64-no-pad of the 32-byte seed.
    File { path: PathBuf },
    /// **TEST ONLY** — inline base64-no-pad seed in the config JSON. Refused
    /// at runtime in release builds.
    InlineSeedBase64 { value: String },
}

impl CokretKeyRef {
    /// Parse from JSON value (used by config parsers).
    #[must_use]
    pub fn from_value(value: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }
}

/// Load a [`Ed25519MoveSigner`] from a [`CokretKeyRef`].
///
/// Returns `Err` on missing env var, unreadable file, decoded seed length
/// != 32, invalid base64, invalid DID URI, or a release build + an
/// `InlineSeedBase64` ref.
pub fn load_ed25519_signer(
    key_ref: &CokretKeyRef,
    did: &str,
    verification_method: &str,
) -> Result<Ed25519MoveSigner> {
    let mut seed_arr = load_seed_array(key_ref)?;

    let did =
        Did::new(did.to_owned()).wrap_err_with(|| format!("cokret signer: invalid DID '{did}'"))?;
    let signer = Ed25519MoveSigner::from_did_key_seed(seed_arr, did, verification_method);
    // `from_did_key_seed` copies the seed into a SigningKey; wipe ours.
    seed_arr.zeroize();
    Ok(signer)
}

fn load_seed_array(key_ref: &CokretKeyRef) -> Result<[u8; 32]> {
    let mut seed_bytes = load_seed_bytes(key_ref)?;
    if seed_bytes.len() != 32 {
        let len = seed_bytes.len();
        seed_bytes.zeroize();
        bail!("cokret signer: seed must be 32 bytes, got {len}");
    }
    let mut seed_arr = [0u8; 32];
    seed_arr.copy_from_slice(&seed_bytes);
    seed_bytes.zeroize();
    Ok(seed_arr)
}

fn load_seed_bytes(key_ref: &CokretKeyRef) -> Result<Vec<u8>> {
    match key_ref {
        CokretKeyRef::Env { var } => {
            let value = std::env::var(var)
                .wrap_err_with(|| format!("cokret signer: env var {var} not set"))?;
            decode_base64_no_pad(&value, "env value")
        }
        CokretKeyRef::File { path } => load_file_seed(path),
        CokretKeyRef::InlineSeedBase64 { value } => {
            #[cfg(not(debug_assertions))]
            {
                let _ = value;
                bail!("cokret signer: inline_seed_base64 is not permitted in release builds");
            }
            #[cfg(debug_assertions)]
            decode_base64_no_pad(value, "inline_seed_base64")
        }
    }
}

fn load_file_seed(path: &Path) -> Result<Vec<u8>> {
    let raw = fs::read(path)
        .wrap_err_with(|| format!("cokret signer: read seed file {}", path.display()))?;
    if raw.len() == 32 {
        // Binary seed, exactly 32 bytes.
        return Ok(raw);
    }
    // Otherwise treat as text -> base64-no-pad, trimming whitespace.
    let text = std::str::from_utf8(&raw).wrap_err_with(|| {
        format!(
            "cokret signer: seed file {} is neither 32 raw bytes nor UTF-8 base64",
            path.display()
        )
    })?;
    decode_base64_no_pad(text.trim(), "file content")
}

fn decode_base64_no_pad(text: &str, source: &str) -> Result<Vec<u8>> {
    // Trim surrounding whitespace/newlines first (e.g. an env var set via
    // `export KEY=$(cat seed.b64)` carries a trailing newline), then strip any
    // base64 padding before decoding with the no-pad engine.
    let cleaned = text.trim().trim_end_matches('=');
    STANDARD_NO_PAD
        .decode(cleaned)
        .wrap_err_with(|| format!("cokret signer: base64 decode failed ({source})"))
}
