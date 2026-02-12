//! Agent implementation.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crew_core::{AgentId, Message, MessageRole, Task, TaskResult, TokenUsage};
use crew_llm::{ChatConfig, ChatResponse, LlmProvider, StopReason};
use crew_memory::{Episode, EpisodeOutcome, EpisodeStore};
use eyre::Result;
use tracing::{Instrument, debug, info, info_span, warn};

use crate::progress::{ProgressEvent, ProgressReporter, SilentReporter};
use crate::tools::ToolRegistry;

/// Configuration for agent execution.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Maximum number of iterations before stopping.
    pub max_iterations: u32,
    /// Maximum total tokens (input + output) before stopping. None = unlimited.
    pub max_tokens: Option<u32>,
    /// Whether to save episodes to memory.
    pub save_episodes: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: 50,
            max_tokens: None,
            save_episodes: true,
        }
    }
}

/// Response from conversation mode (process_message).
#[derive(Debug, Clone)]
pub struct ConversationResponse {
    pub content: String,
    pub token_usage: TokenUsage,
    pub files_modified: Vec<PathBuf>,
}

/// An agent that can execute tasks.
pub struct Agent {
    /// Unique identifier for this agent.
    pub id: AgentId,
    /// LLM provider for generating responses.
    llm: Arc<dyn LlmProvider>,
    /// Tool registry for executing tool calls.
    tools: ToolRegistry,
    /// Episode store for memory.
    memory: Arc<EpisodeStore>,
    /// System prompt for this agent.
    system_prompt: String,
    /// Agent configuration.
    config: AgentConfig,
    /// Progress reporter.
    reporter: Arc<dyn ProgressReporter>,
    /// Shutdown signal.
    shutdown: Arc<AtomicBool>,
}

impl Agent {
    /// Create a new agent.
    pub fn new(
        id: AgentId,
        llm: Arc<dyn LlmProvider>,
        tools: ToolRegistry,
        memory: Arc<EpisodeStore>,
    ) -> Self {
        let system_prompt = include_str!("prompts/worker.txt").to_string();

        Self {
            id,
            llm,
            tools,
            memory,
            system_prompt,
            config: AgentConfig::default(),
            reporter: Arc::new(SilentReporter),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Set the agent configuration.
    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    /// Set the progress reporter.
    pub fn with_reporter(mut self, reporter: Arc<dyn ProgressReporter>) -> Self {
        self.reporter = reporter;
        self
    }

    /// Set the shutdown signal.
    pub fn with_shutdown(mut self, shutdown: Arc<AtomicBool>) -> Self {
        self.shutdown = shutdown;
        self
    }

    /// Override the system prompt (e.g. for gateway mode).
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = prompt;
        self
    }

    /// Process a single message in conversation mode (chat/gateway).
    /// Takes the user's message, conversation history, and optional media paths.
    pub async fn process_message(
        &self,
        user_content: &str,
        history: &[Message],
        media: Vec<String>,
    ) -> Result<ConversationResponse> {
        let mut messages = vec![Message {
            role: MessageRole::System,
            content: self.system_prompt.clone(),
            media: vec![],
            tool_calls: None,
            tool_call_id: None,
            timestamp: chrono::Utc::now(),
        }];

        messages.extend_from_slice(history);

        messages.push(Message {
            role: MessageRole::User,
            content: user_content.to_string(),
            media,
            tool_calls: None,
            tool_call_id: None,
            timestamp: chrono::Utc::now(),
        });

        let config = ChatConfig::default();
        let mut total_usage = TokenUsage::default();
        let mut files_modified = Vec::new();
        let mut iteration = 0u32;

        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                return Ok(ConversationResponse {
                    content: "Interrupted.".into(),
                    token_usage: total_usage,
                    files_modified,
                });
            }

            if iteration >= self.config.max_iterations {
                return Ok(ConversationResponse {
                    content: "Reached max iterations.".into(),
                    token_usage: total_usage,
                    files_modified,
                });
            }

            if let Some(max_tokens) = self.config.max_tokens {
                let used = total_usage.input_tokens + total_usage.output_tokens;
                if used >= max_tokens {
                    return Ok(ConversationResponse {
                        content: "Token budget exceeded.".into(),
                        token_usage: total_usage,
                        files_modified,
                    });
                }
            }

            iteration += 1;
            let tools_spec = self.tools.specs();
            let response = self.llm.chat(&messages, &tools_spec, &config).await?;
            total_usage.input_tokens += response.usage.input_tokens;
            total_usage.output_tokens += response.usage.output_tokens;

            match response.stop_reason {
                StopReason::EndTurn | StopReason::StopSequence => {
                    return Ok(ConversationResponse {
                        content: response.content.unwrap_or_default(),
                        token_usage: total_usage,
                        files_modified,
                    });
                }
                StopReason::ToolUse => {
                    messages.push(self.response_to_message(&response));
                    let (tool_messages, tool_files, tool_tokens) =
                        self.execute_tools(&response).await?;
                    messages.extend(tool_messages);
                    files_modified.extend(tool_files);
                    total_usage.input_tokens += tool_tokens.input_tokens;
                    total_usage.output_tokens += tool_tokens.output_tokens;
                }
                StopReason::MaxTokens => {
                    return Ok(ConversationResponse {
                        content: response.content.unwrap_or_default(),
                        token_usage: total_usage,
                        files_modified,
                    });
                }
            }
        }
    }

    /// Run a task to completion (used by spawn tool).
    pub async fn run_task(&self, task: &Task) -> Result<TaskResult> {
        let task_start = Instant::now();
        let span = info_span!(
            "task",
            task_id = %task.id,
            agent_id = %self.id,
        );

        async {
            info!("starting task");
            self.reporter.report(ProgressEvent::TaskStarted {
                task_id: task.id.to_string(),
            });

            let mut iteration = 0u32;
            let mut messages = self.build_initial_messages(task).await;
            let mut total_usage = TokenUsage::default();
            let mut files_modified = Vec::new();
            let config = ChatConfig::default();

            loop {
                if self.shutdown.load(Ordering::Relaxed) {
                    info!(iteration, "shutdown signal received");
                    self.reporter
                        .report(ProgressEvent::TaskInterrupted { iterations: iteration });
                    return Ok(TaskResult {
                        success: false,
                        output: "Task interrupted.".to_string(),
                        files_modified,
                        subtasks: Vec::new(),
                        token_usage: total_usage,
                    });
                }

                if iteration >= self.config.max_iterations {
                    warn!(iteration, max = self.config.max_iterations, "hit max iterations limit");
                    self.reporter.report(ProgressEvent::MaxIterationsReached {
                        limit: self.config.max_iterations,
                    });
                    return Ok(TaskResult {
                        success: false,
                        output: format!("Task stopped after {} iterations (limit).", iteration),
                        files_modified,
                        subtasks: Vec::new(),
                        token_usage: total_usage,
                    });
                }

                if let Some(max_tokens) = self.config.max_tokens {
                    let used = total_usage.input_tokens + total_usage.output_tokens;
                    if used >= max_tokens {
                        warn!(used, max = max_tokens, "hit token budget limit");
                        self.reporter.report(ProgressEvent::TokenBudgetExceeded {
                            used,
                            limit: max_tokens,
                        });
                        return Ok(TaskResult {
                            success: false,
                            output: format!("Task stopped after {} tokens (budget: {}).", used, max_tokens),
                            files_modified,
                            subtasks: Vec::new(),
                            token_usage: total_usage,
                        });
                    }
                }

                iteration += 1;
                let iter_start = Instant::now();
                self.reporter.report(ProgressEvent::Thinking { iteration });

                let tools_spec = self.tools.specs();
                let response = self.llm.chat(&messages, &tools_spec, &config).await?;
                total_usage.input_tokens += response.usage.input_tokens;
                total_usage.output_tokens += response.usage.output_tokens;

                debug!(
                    iteration,
                    input_tokens = response.usage.input_tokens,
                    output_tokens = response.usage.output_tokens,
                    stop_reason = ?response.stop_reason,
                    duration_ms = iter_start.elapsed().as_millis() as u64,
                    "llm response"
                );

                if let Some(ref content) = response.content {
                    self.reporter.report(ProgressEvent::Response {
                        content: content.clone(),
                        iteration,
                    });
                }

                match response.stop_reason {
                    StopReason::EndTurn | StopReason::StopSequence => {
                        if self.config.save_episodes {
                            let summary = response.content.clone().unwrap_or_default();
                            let summary_truncated = if summary.len() > 500 {
                                format!("{}...", &summary[..500])
                            } else {
                                summary
                            };

                            let mut episode = Episode::new(
                                task.id.clone(),
                                self.id.clone(),
                                task.context.working_dir.clone(),
                                summary_truncated,
                                EpisodeOutcome::Success,
                            );
                            episode.files_modified = files_modified.clone();

                            if let Err(e) = self.memory.store(episode).await {
                                warn!(error = %e, "failed to save episode to memory");
                            }
                        }

                        self.reporter.report(ProgressEvent::TaskCompleted {
                            success: true,
                            iterations: iteration,
                            duration: task_start.elapsed(),
                        });

                        info!(
                            total_input_tokens = total_usage.input_tokens,
                            total_output_tokens = total_usage.output_tokens,
                            iterations = iteration,
                            files_modified = files_modified.len(),
                            duration_ms = task_start.elapsed().as_millis() as u64,
                            "task completed"
                        );
                        return Ok(self.build_result(&response, total_usage, files_modified));
                    }
                    StopReason::ToolUse => {
                        messages.push(self.response_to_message(&response));
                        let (tool_messages, tool_files, tool_tokens) =
                            self.execute_tools(&response).await?;
                        for msg in tool_messages {
                            messages.push(msg);
                        }
                        files_modified.extend(tool_files);
                        total_usage.input_tokens += tool_tokens.input_tokens;
                        total_usage.output_tokens += tool_tokens.output_tokens;
                    }
                    StopReason::MaxTokens => {
                        self.reporter.report(ProgressEvent::TaskCompleted {
                            success: false,
                            iterations: iteration,
                            duration: task_start.elapsed(),
                        });
                        return Ok(self.build_result(&response, total_usage, files_modified));
                    }
                }
            }
        }
        .instrument(span)
        .await
    }

    async fn build_initial_messages(&self, task: &Task) -> Vec<Message> {
        let mut messages = vec![Message {
            role: MessageRole::System,
            content: self.system_prompt.clone(),
            media: vec![],
            tool_calls: None,
            tool_call_id: None,
            timestamp: chrono::Utc::now(),
        }];

        // Add working memory from context
        messages.extend(task.context.working_memory.clone());

        // Query episodic memory for relevant past experiences
        let query = match &task.kind {
            crew_core::TaskKind::Plan { goal } => goal.clone(),
            crew_core::TaskKind::Code { instruction, .. } => instruction.clone(),
            crew_core::TaskKind::Review { .. } => "code review".to_string(),
            crew_core::TaskKind::Test { command } => command.clone(),
            crew_core::TaskKind::Custom { name, .. } => name.clone(),
        };

        if let Ok(episodes) = self
            .memory
            .find_relevant(&task.context.working_dir, &query, 3)
            .await
        {
            if !episodes.is_empty() {
                let mut context_str = String::from("## Relevant Past Experiences\n\n");
                for ep in &episodes {
                    context_str.push_str(&format!(
                        "### {} ({})\n{}\n",
                        ep.task_id,
                        match ep.outcome {
                            crew_memory::EpisodeOutcome::Success => "succeeded",
                            crew_memory::EpisodeOutcome::Failure => "failed",
                            crew_memory::EpisodeOutcome::Blocked => "blocked",
                            crew_memory::EpisodeOutcome::Cancelled => "cancelled",
                        },
                        ep.summary
                    ));
                    if !ep.key_decisions.is_empty() {
                        context_str.push_str("Key decisions:\n");
                        for decision in &ep.key_decisions {
                            context_str.push_str(&format!("- {}\n", decision));
                        }
                    }
                    context_str.push('\n');
                }

                messages.push(Message {
                    role: MessageRole::System,
                    content: context_str,
                    media: vec![],
                    tool_calls: None,
                    tool_call_id: None,
                    timestamp: chrono::Utc::now(),
                });
            }
        }

        // Add the task as user message
        let task_content = match &task.kind {
            crew_core::TaskKind::Plan { goal } => format!("Plan how to accomplish: {goal}"),
            crew_core::TaskKind::Code { instruction, files } => {
                let files_str = files
                    .iter()
                    .map(|f| f.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Code task: {instruction}\nFiles in scope: {files_str}")
            }
            crew_core::TaskKind::Review { diff } => format!("Review this diff:\n{diff}"),
            crew_core::TaskKind::Test { command } => format!("Run test: {command}"),
            crew_core::TaskKind::Custom { name, params } => {
                format!("Custom task '{name}': {params}")
            }
        };

        messages.push(Message {
            role: MessageRole::User,
            content: task_content,
            media: vec![],
            tool_calls: None,
            tool_call_id: None,
            timestamp: chrono::Utc::now(),
        });

        messages
    }

    fn response_to_message(&self, response: &ChatResponse) -> Message {
        Message {
            role: MessageRole::Assistant,
            content: response.content.clone().unwrap_or_default(),
            media: vec![],
            tool_calls: if response.tool_calls.is_empty() {
                None
            } else {
                Some(response.tool_calls.clone())
            },
            tool_call_id: None,
            timestamp: chrono::Utc::now(),
        }
    }

    async fn execute_tools(
        &self,
        response: &ChatResponse,
    ) -> Result<(Vec<Message>, Vec<std::path::PathBuf>, TokenUsage)> {
        let mut messages = Vec::new();
        let mut files_modified = Vec::new();
        let mut tokens_used = TokenUsage::default();

        for tool_call in &response.tool_calls {
            let tool_start = Instant::now();
            debug!(tool = %tool_call.name, tool_id = %tool_call.id, "executing tool");

            self.reporter.report(ProgressEvent::ToolStarted {
                name: tool_call.name.clone(),
                tool_id: tool_call.id.clone(),
            });

            let result = self
                .tools
                .execute(&tool_call.name, &tool_call.arguments)
                .await;

            let duration = tool_start.elapsed();

            let content = match result {
                Ok(tool_result) => {
                    debug!(
                        tool = %tool_call.name,
                        success = tool_result.success,
                        duration_ms = duration.as_millis() as u64,
                        "tool completed"
                    );

                    if let Some(ref file) = tool_result.file_modified {
                        info!(tool = %tool_call.name, file = %file.display(), "file modified");
                        files_modified.push(file.clone());
                        self.reporter.report(ProgressEvent::FileModified {
                            path: file.display().to_string(),
                        });
                    }

                    if let Some(tokens) = tool_result.tokens_used {
                        tokens_used.input_tokens += tokens.input_tokens;
                        tokens_used.output_tokens += tokens.output_tokens;
                    }

                    let output_preview = if tool_result.output.len() > 200 {
                        format!("{}...", &tool_result.output[..200])
                    } else {
                        tool_result.output.clone()
                    };

                    self.reporter.report(ProgressEvent::ToolCompleted {
                        name: tool_call.name.clone(),
                        tool_id: tool_call.id.clone(),
                        success: tool_result.success,
                        output_preview,
                        duration,
                    });

                    tool_result.output
                }
                Err(e) => {
                    warn!(
                        tool = %tool_call.name,
                        error = %e,
                        duration_ms = duration.as_millis() as u64,
                        "tool failed"
                    );

                    self.reporter.report(ProgressEvent::ToolCompleted {
                        name: tool_call.name.clone(),
                        tool_id: tool_call.id.clone(),
                        success: false,
                        output_preview: e.to_string(),
                        duration,
                    });

                    format!("Error: {e}")
                }
            };

            messages.push(Message {
                role: MessageRole::Tool,
                content,
                media: vec![],
                tool_calls: None,
                tool_call_id: Some(tool_call.id.clone()),
                timestamp: chrono::Utc::now(),
            });
        }

        Ok((messages, files_modified, tokens_used))
    }

    fn build_result(
        &self,
        response: &ChatResponse,
        usage: TokenUsage,
        files_modified: Vec<std::path::PathBuf>,
    ) -> TaskResult {
        TaskResult {
            success: true,
            output: response.content.clone().unwrap_or_default(),
            files_modified,
            subtasks: Vec::new(),
            token_usage: crew_core::TokenUsage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
            },
        }
    }
}
