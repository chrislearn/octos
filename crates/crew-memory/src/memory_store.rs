//! Markdown-based persistent memory store.
//!
//! Stores long-term memory in `MEMORY.md` and daily notes in `YYYY-MM-DD.md`
//! under a `.crew/memory/` directory.

use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr};

/// Persistent memory store backed by markdown files.
pub struct MemoryStore {
    memory_dir: PathBuf,
}

impl MemoryStore {
    /// Open (or create) the memory directory under `data_dir`.
    pub async fn open(data_dir: impl AsRef<Path>) -> Result<Self> {
        let memory_dir = data_dir.as_ref().join("memory");
        tokio::fs::create_dir_all(&memory_dir)
            .await
            .wrap_err("failed to create memory directory")?;
        Ok(Self { memory_dir })
    }

    /// Read long-term memory (`MEMORY.md`). Returns empty string if missing.
    pub async fn read_long_term(&self) -> Result<String> {
        let path = self.memory_dir.join("MEMORY.md");
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => Ok(content),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e).wrap_err("failed to read MEMORY.md"),
        }
    }

    /// Write long-term memory (`MEMORY.md`), replacing previous content.
    pub async fn write_long_term(&self, content: &str) -> Result<()> {
        let path = self.memory_dir.join("MEMORY.md");
        tokio::fs::write(&path, content)
            .await
            .wrap_err("failed to write MEMORY.md")
    }

    /// Read today's daily notes. Returns empty string if missing.
    pub async fn read_today(&self) -> Result<String> {
        let path = self.today_path();
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => Ok(content),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e).wrap_err("failed to read today's notes"),
        }
    }

    /// Append to today's daily notes. Creates file with date header if new.
    pub async fn append_today(&self, content: &str) -> Result<()> {
        let path = self.today_path();
        let existing = self.read_today().await?;

        let new_content = if existing.is_empty() {
            let date = chrono::Local::now().format("%Y-%m-%d");
            format!("# {date}\n\n{content}\n")
        } else {
            format!("{existing}{content}\n")
        };

        tokio::fs::write(&path, new_content)
            .await
            .wrap_err("failed to write today's notes")
    }

    /// Build a formatted context string for injection into the system prompt.
    pub async fn get_memory_context(&self) -> String {
        let long_term = self.read_long_term().await.unwrap_or_default();
        let today = self.read_today().await.unwrap_or_default();

        let mut ctx = String::new();

        if !long_term.is_empty() {
            ctx.push_str("## Long-term Memory\n\n");
            ctx.push_str(&long_term);
            ctx.push_str("\n\n");
        }

        if !today.is_empty() {
            ctx.push_str("## Today's Notes\n\n");
            ctx.push_str(&today);
            ctx.push('\n');
        }

        ctx
    }

    fn today_path(&self) -> PathBuf {
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        self.memory_dir.join(format!("{date}.md"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).await.unwrap();

        assert_eq!(store.read_long_term().await.unwrap(), "");
        assert_eq!(store.read_today().await.unwrap(), "");
        assert_eq!(store.get_memory_context().await, "");
    }

    #[tokio::test]
    async fn test_long_term_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).await.unwrap();

        store.write_long_term("remember this").await.unwrap();
        assert_eq!(store.read_long_term().await.unwrap(), "remember this");

        store.write_long_term("updated").await.unwrap();
        assert_eq!(store.read_long_term().await.unwrap(), "updated");
    }

    #[tokio::test]
    async fn test_append_today_creates_header() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).await.unwrap();

        store.append_today("first note").await.unwrap();
        let content = store.read_today().await.unwrap();
        assert!(content.starts_with("# "));
        assert!(content.contains("first note"));
    }

    #[tokio::test]
    async fn test_append_today_appends() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).await.unwrap();

        store.append_today("note 1").await.unwrap();
        store.append_today("note 2").await.unwrap();

        let content = store.read_today().await.unwrap();
        assert!(content.contains("note 1"));
        assert!(content.contains("note 2"));
        // Only one header
        assert_eq!(content.matches("# ").count(), 1);
    }

    #[tokio::test]
    async fn test_get_memory_context_formatting() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).await.unwrap();

        store.write_long_term("I am a bot").await.unwrap();
        store.append_today("did something").await.unwrap();

        let ctx = store.get_memory_context().await;
        assert!(ctx.contains("## Long-term Memory"));
        assert!(ctx.contains("I am a bot"));
        assert!(ctx.contains("## Today's Notes"));
        assert!(ctx.contains("did something"));
    }
}
