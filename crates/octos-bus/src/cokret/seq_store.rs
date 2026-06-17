//! File-backed [`SeqStore`] for the cokret bridge runtime.
//!
//! The `cokret-bridge-runtime` crate drives outbound applet event minting
//! through a restart-safe monotonic sequence allocator. The runtime's
//! `SeqAllocator` reserves blocks from a pluggable [`SeqStore`]; the default
//! upstream implementation is diesel-backed. octos does not embed diesel, so
//! we provide a small file-backed store instead.
//!
//! [`FileSeqStore`] persists per-key high-water marks as a flat JSON object
//! (`{"<key>": <i64>, ...}`). Each [`reserve_block`](FileSeqStore::reserve_block)
//! call loads-or-initializes the value for the key, advances it by `block`,
//! persists atomically (temp file + rename), and returns the new high-water
//! mark.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cokret_bridge_runtime::{BridgeError, Result as BridgeResult, SeqStore};

/// On-disk JSON shape: `{ "<seq-key>": <high-water-mark>, ... }`.
type SeqMap = HashMap<String, i64>;

/// A file-backed, restart-safe [`SeqStore`].
#[derive(Debug)]
pub struct FileSeqStore {
    path: PathBuf,
    state: Mutex<SeqMap>,
}

impl FileSeqStore {
    /// Open (or lazily create) a store backed by `path`.
    pub fn new(path: PathBuf) -> BridgeResult<Self> {
        let state = load_map(&path)?;
        Ok(Self {
            path,
            state: Mutex::new(state),
        })
    }

    /// Convenience constructor returning a trait object ready to hand to the
    /// runtime's `SeqAllocator::new`.
    pub fn shared(path: PathBuf) -> BridgeResult<Arc<dyn SeqStore>> {
        Ok(Arc::new(Self::new(path)?))
    }

    /// Persist the current in-memory map atomically: write a sibling temp file
    /// then rename it over the target so a crash mid-write never leaves a
    /// truncated/partial JSON file.
    fn persist(&self, map: &SeqMap) -> BridgeResult<()> {
        let bytes = serde_json::to_vec_pretty(map)
            .map_err(|e| BridgeError::App(format!("seq_store: serialize: {e}")))?;
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                BridgeError::App(format!("seq_store: create dir {}: {e}", parent.display()))
            })?;
        }
        let tmp = self.tmp_path();
        std::fs::write(&tmp, &bytes)
            .map_err(|e| BridgeError::App(format!("seq_store: write {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            BridgeError::App(format!(
                "seq_store: rename {} -> {}: {e}",
                tmp.display(),
                self.path.display()
            ))
        })?;
        Ok(())
    }

    fn tmp_path(&self) -> PathBuf {
        let mut name = self
            .path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_else(|| "seq_store.json".into());
        name.push(".tmp");
        self.path.with_file_name(name)
    }
}

impl SeqStore for FileSeqStore {
    fn reserve_block(&self, key: &str, block: i64) -> BridgeResult<i64> {
        let mut map = self
            .state
            .lock()
            .map_err(|_| BridgeError::App("seq_store: mutex poisoned".to_owned()))?;
        let current = map.get(key).copied().unwrap_or(0);
        let reserved = current.saturating_add(block);
        map.insert(key.to_owned(), reserved);
        // Persist while holding the lock so the on-disk high-water mark is
        // never behind a value already handed back to a caller.
        self.persist(&map)?;
        Ok(reserved)
    }
}

/// Read and parse the backing file. A missing file yields an empty map.
fn load_map(path: &PathBuf) -> BridgeResult<SeqMap> {
    match std::fs::read(path) {
        Ok(bytes) => {
            if bytes.is_empty() {
                return Ok(SeqMap::new());
            }
            serde_json::from_slice(&bytes)
                .map_err(|e| BridgeError::App(format!("seq_store: parse {}: {e}", path.display())))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SeqMap::new()),
        Err(e) => Err(BridgeError::App(format!(
            "seq_store: read {}: {e}",
            path.display()
        ))),
    }
}
