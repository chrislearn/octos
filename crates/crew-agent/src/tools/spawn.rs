//! Spawn tool for background subagent execution.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use crew_core::{AgentId, AgentRole, InboundMessage, Task, TaskContext, TaskKind};
use crew_llm::LlmProvider;
use crew_memory::EpisodeStore;
use eyre::{Result, WrapErr};
use serde::Deserialize;
use tracing::{info, warn};

use super::{Tool, ToolRegistry, ToolResult};
use crate::Agent;

/// Tool that spawns background worker agents for long-running tasks.
pub struct SpawnTool {
    llm: Arc<dyn LlmProvider>,
    memory: Arc<EpisodeStore>,
    working_dir: PathBuf,
    inbound_tx: tokio::sync::mpsc::Sender<InboundMessage>,
    origin: std::sync::Mutex<(String, String)>,
    worker_count: AtomicU32,
}

impl SpawnTool {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        memory: Arc<EpisodeStore>,
        working_dir: PathBuf,
        inbound_tx: tokio::sync::mpsc::Sender<InboundMessage>,
    ) -> Self {
        Self {
            llm,
            memory,
            working_dir,
            inbound_tx,
            origin: std::sync::Mutex::new(("cli".into(), "default".into())),
            worker_count: AtomicU32::new(0),
        }
    }

    /// Update the origin context for result delivery (called per inbound message).
    pub fn set_context(&self, channel: &str, chat_id: &str) {
        *self.origin.lock().unwrap() = (channel.to_string(), chat_id.to_string());
    }
}

#[derive(Deserialize)]
struct Input {
    task: String,
    #[serde(default)]
    label: Option<String>,
}

#[async_trait]
impl Tool for SpawnTool {
    fn name(&self) -> &str {
        "spawn"
    }

    fn description(&self) -> &str {
        "Spawn a background subagent to work on a task. Returns immediately while the task runs in the background. Results are announced when complete."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "The task for the background agent to complete"
                },
                "label": {
                    "type": "string",
                    "description": "Optional short label for display"
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> Result<ToolResult> {
        let input: Input =
            serde_json::from_value(args.clone()).wrap_err("invalid spawn tool input")?;

        let worker_num = self.worker_count.fetch_add(1, Ordering::SeqCst);
        let worker_id = AgentId::new(format!("subagent-{worker_num}"));
        let label = input.label.unwrap_or_else(|| input.task.chars().take(60).collect());

        let (origin_channel, origin_chat_id) = self.origin.lock().unwrap().clone();

        // Clone what the background task needs
        let llm = self.llm.clone();
        let memory = self.memory.clone();
        let working_dir = self.working_dir.clone();
        let inbound_tx = self.inbound_tx.clone();
        let task_desc = input.task.clone();
        let wid = worker_id.clone();

        info!(worker_id = %worker_id, task = %task_desc, "spawning background agent");

        tokio::spawn(async move {
            let tools = ToolRegistry::with_builtins(&working_dir);
            let worker = Agent::new(wid.clone(), AgentRole::Worker, llm, tools, memory);

            let subtask = Task::new(
                TaskKind::Code {
                    instruction: task_desc.clone(),
                    files: vec![],
                },
                TaskContext {
                    working_dir,
                    ..Default::default()
                },
            );

            let result = worker.run_task(&subtask).await;

            let content = match &result {
                Ok(r) => format!(
                    "[Subagent {} completed]\nTask: {}\nStatus: {}\n\nResult:\n{}\n\nPlease summarize this result naturally for the user.",
                    wid,
                    task_desc,
                    if r.success { "SUCCESS" } else { "FAILED" },
                    r.output
                ),
                Err(e) => format!(
                    "[Subagent {} failed]\nTask: {}\nError: {e}\n\nPlease inform the user about this failure.",
                    wid, task_desc
                ),
            };

            let announce = InboundMessage {
                channel: "system".into(),
                sender_id: "subagent".into(),
                chat_id: format!("{origin_channel}:{origin_chat_id}"),
                content,
                timestamp: chrono::Utc::now(),
                media: vec![],
                metadata: serde_json::json!({
                    "deliver_to_channel": origin_channel,
                    "deliver_to_chat_id": origin_chat_id,
                }),
            };

            if let Err(e) = inbound_tx.send(announce).await {
                warn!(error = %e, "failed to announce subagent result");
            }
        });

        Ok(ToolResult {
            output: format!("Spawned background task: {label}"),
            success: true,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_returns_immediately() {
        let (in_tx, _in_rx) = tokio::sync::mpsc::channel(16);

        // We can't easily create a real LLM + EpisodeStore for unit tests,
        // so just test the worker count and basic input parsing.
        let tool = SpawnTool {
            llm: Arc::new(MockProvider),
            memory: Arc::new(create_test_store().await),
            working_dir: PathBuf::from("/tmp"),
            inbound_tx: in_tx,
            origin: std::sync::Mutex::new(("cli".into(), "test".into())),
            worker_count: AtomicU32::new(0),
        };

        assert_eq!(tool.worker_count.load(Ordering::SeqCst), 0);

        // Invalid input test
        let result = tool.execute(&serde_json::json!({})).await;
        assert!(result.is_err());

        // Worker count should not increment on invalid input
        assert_eq!(tool.worker_count.load(Ordering::SeqCst), 0);
    }

    // Minimal mock provider for testing
    struct MockProvider;

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn chat(
            &self,
            _messages: &[crew_core::Message],
            _tools: &[crew_llm::ToolSpec],
            _config: &crew_llm::ChatConfig,
        ) -> Result<crew_llm::ChatResponse> {
            Ok(crew_llm::ChatResponse {
                content: Some("done".into()),
                tool_calls: vec![],
                stop_reason: crew_llm::StopReason::EndTurn,
                usage: crew_llm::TokenUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                },
            })
        }

        fn model_id(&self) -> &str {
            "mock"
        }

        fn provider_name(&self) -> &str {
            "mock"
        }
    }

    async fn create_test_store() -> EpisodeStore {
        let dir = tempfile::tempdir().unwrap();
        // Leak the dir so it stays alive for the test
        let dir = Box::leak(Box::new(dir));
        EpisodeStore::open(dir.path()).await.unwrap()
    }
}
