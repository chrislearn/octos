# Architecture Document: crew-rs

## Overview

crew-rs is structured as a Rust workspace with 5 crates, each with a specific responsibility. The architecture follows a layered design with clear separation of concerns.

```
┌─────────────────────────────────────────────────────────────┐
│                        crew-cli                              │
│                    (CLI Interface)                           │
├─────────────────────────────────────────────────────────────┤
│                       crew-agent                             │
│              (Agent Runtime & Tools)                         │
├───────────────────────┬─────────────────────────────────────┤
│      crew-memory      │           crew-llm                   │
│   (Episodic Store)    │      (LLM Providers)                 │
├───────────────────────┴─────────────────────────────────────┤
│                       crew-core                              │
│                  (Types & Protocols)                         │
└─────────────────────────────────────────────────────────────┘
```

## Crate Structure

### crew-core

**Purpose**: Shared types, task model, and error handling.

**Key Components**:

```rust
// Task model
pub struct Task {
    pub id: TaskId,
    pub kind: TaskKind,
    pub status: TaskStatus,
    pub context: TaskContext,
    pub parent_id: Option<TaskId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub enum TaskKind {
    Code { instruction: String, files: Vec<PathBuf> },
    Plan { goal: String },
    Review { files: Vec<PathBuf>, criteria: String },
    Custom { name: String, data: Value },
}

pub enum TaskStatus {
    Pending,
    InProgress,
    Blocked { reason: String },
    Completed,
    Failed { error: String },
}

// Message types
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    pub tool_call_id: Option<String>,
}

pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

// Agent types
pub struct AgentId(String);

pub enum AgentRole {
    Coordinator,
    Worker,
}
```

**Dependencies**: serde, chrono, uuid, eyre

### crew-llm

**Purpose**: LLM provider abstraction with unified interface.

**Key Components**:

```rust
// Provider trait
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        config: &ChatConfig,
    ) -> Result<ChatResponse>;

    fn model_id(&self) -> &str;
    fn provider_name(&self) -> &str;
}

// Response types
pub struct ChatResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: StopReason,
    pub usage: TokenUsage,
}

pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
}

// Retry wrapper
pub struct RetryProvider {
    inner: Arc<dyn LlmProvider>,
    config: RetryConfig,
}
```

**Providers**:
- `AnthropicProvider` - Claude models via Messages API
- `OpenAIProvider` - GPT models via Chat Completions API
- `GeminiProvider` - Gemini models via GenerateContent API

**Dependencies**: async-trait, reqwest, crew-core

### crew-memory

**Purpose**: Episodic memory and task state persistence.

**Key Components**:

```rust
// Episode storage
pub struct EpisodeStore {
    db: Database,
}

impl EpisodeStore {
    pub async fn open(data_dir: &Path) -> Result<Self>;
    pub async fn save(&self, episode: &Episode) -> Result<()>;
    pub async fn find_relevant(&self, cwd: &Path, goal: &str, limit: usize) -> Result<Vec<Episode>>;
}

// Task state storage
pub struct TaskStore {
    dir: PathBuf,
}

impl TaskStore {
    pub async fn open(data_dir: &Path) -> Result<Self>;
    pub async fn save(&self, state: &TaskState) -> Result<()>;
    pub async fn load(&self, task_id: &TaskId) -> Result<Option<TaskState>>;
    pub async fn list(&self) -> Result<Vec<TaskState>>;
    pub async fn delete(&self, task_id: &TaskId) -> Result<()>;
}

// Task state for resumption
pub struct TaskState {
    pub task: Task,
    pub messages: Vec<Message>,
    pub token_usage: TokenUsage,
    pub is_coordinator: bool,
}
```

**Storage**: redb (embedded key-value database)

**Dependencies**: redb, crew-core

### crew-agent

**Purpose**: Agent runtime, tool execution, and progress reporting.

**Key Components**:

```rust
// Agent runtime
pub struct Agent {
    id: AgentId,
    role: AgentRole,
    llm: Arc<dyn LlmProvider>,
    tools: ToolRegistry,
    memory: Arc<EpisodeStore>,
    config: AgentConfig,
    reporter: Arc<dyn ProgressReporter>,
    shutdown: Arc<AtomicBool>,
}

impl Agent {
    pub async fn run_task(&self, task: &Task) -> Result<TaskResult>;
    pub async fn run_task_resumable(
        &self,
        task: &Task,
        store: &TaskStore,
        state: Option<TaskState>,
    ) -> Result<TaskResult>;
}

// Agent configuration
pub struct AgentConfig {
    pub max_iterations: u32,
    pub max_tokens: Option<u32>,
    pub save_episodes: bool,
}

// Tool system
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    async fn execute(&self, args: &Value) -> Result<ToolResult>;
}

// Progress reporting
pub trait ProgressReporter: Send + Sync {
    fn report(&self, event: ProgressEvent);
}

pub enum ProgressEvent {
    TaskStarted { task_id: String },
    Thinking { iteration: u32 },
    Response { content: String, iteration: u32 },
    ToolStarted { name: String, tool_id: String },
    ToolCompleted { name, tool_id, success, output_preview, duration },
    FileModified { path: String },
    TokenUsage { input_tokens: u32, output_tokens: u32 },
    TaskCompleted { success, iterations, duration },
    TaskInterrupted { iterations: u32 },
    MaxIterationsReached { limit: u32 },
    TokenBudgetExceeded { used: u32, limit: u32 },
}
```

**Built-in Tools**:

| Tool | Description |
|------|-------------|
| `ShellTool` | Execute shell commands |
| `ReadFileTool` | Read file contents |
| `WriteFileTool` | Write/create files |
| `EditFileTool` | Search/replace edits |
| `GlobTool` | Find files by pattern |
| `GrepTool` | Search file contents |
| `DelegateTaskTool` | Single task delegation |
| `DelegateBatchTool` | Parallel task delegation |

**Dependencies**: crew-core, crew-llm, crew-memory, tokio

### crew-cli

**Purpose**: Command-line interface.

**Key Components**:

```rust
// Command structure
pub enum Command {
    Init(InitCommand),
    Run(RunCommand),
    Resume(ResumeCommand),
    List(ListCommand),
    Status(StatusCommand),
    Clean(CleanCommand),
    Completions(CompletionsCommand),
}

pub trait Executable {
    fn execute(self) -> Result<()>;
}

// Configuration
pub struct Config {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
}
```

**Dependencies**: crew-agent, crew-memory, crew-llm, clap, clap_complete

## Data Flow

### Task Execution Flow

```
┌──────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐
│   CLI    │────▶│  Agent   │────▶│   LLM    │────▶│ Provider │
│          │     │          │     │          │     │   API    │
└──────────┘     └────┬─────┘     └──────────┘     └──────────┘
                      │
                      ▼
                ┌──────────┐
                │  Tools   │
                │          │
                └────┬─────┘
                     │
        ┌────────────┼────────────┐
        ▼            ▼            ▼
   ┌────────┐   ┌────────┐   ┌────────┐
   │ Shell  │   │  File  │   │ Search │
   │        │   │  Ops   │   │        │
   └────────┘   └────────┘   └────────┘
```

### Agent Loop

```
┌─────────────────────────────────────────────────────────┐
│                    Agent Loop                            │
├─────────────────────────────────────────────────────────┤
│                                                          │
│  1. Build messages (system prompt + history + context)   │
│                          │                               │
│                          ▼                               │
│  2. Call LLM provider with tools                         │
│                          │                               │
│                          ▼                               │
│  3. Parse response                                       │
│         │                │                               │
│         ▼                ▼                               │
│    [Text only]     [Tool calls]                          │
│         │                │                               │
│         │                ▼                               │
│         │          4. Execute tools                      │
│         │                │                               │
│         │                ▼                               │
│         │          5. Append results                     │
│         │                │                               │
│         ▼                ▼                               │
│  6. Check termination conditions                         │
│     - stop_reason == EndTurn                             │
│     - iteration >= max_iterations                        │
│     - tokens >= max_tokens                               │
│     - shutdown signal                                    │
│                          │                               │
│         ┌────────────────┴────────────────┐              │
│         ▼                                 ▼              │
│    [Continue]                        [Terminate]         │
│         │                                 │              │
│         └─────────────────┐               │              │
│                           ▼               ▼              │
│                    Back to step 1    Return result       │
│                                                          │
└─────────────────────────────────────────────────────────┘
```

### State Persistence Flow

```
┌─────────────┐
│  Task Run   │
└──────┬──────┘
       │
       ▼
┌─────────────┐     On each iteration
│ Save State  │◄──────────────────────
└──────┬──────┘
       │
       ▼
┌─────────────┐
│  .crew/     │
│  tasks/     │
│  {id}.json  │
└─────────────┘
       │
       │  On interrupt (Ctrl+C)
       ▼
┌─────────────┐
│   Resume    │
│   Command   │
└──────┬──────┘
       │
       ▼
┌─────────────┐
│ Load State  │
│ Continue    │
└─────────────┘
```

## Coordinator Pattern

### Role Separation

```
┌────────────────────────────────────────────────────────┐
│                    Coordinator                          │
│                                                         │
│  - Receives high-level goal                             │
│  - Decomposes into subtasks                             │
│  - Delegates to workers                                 │
│  - Aggregates results                                   │
│                                                         │
│  Tools: shell, read_file, edit_file, write_file,       │
│         glob, grep, delegate_task, delegate_batch       │
└───────────────────────┬────────────────────────────────┘
                        │
           ┌────────────┼────────────┐
           ▼            ▼            ▼
┌──────────────┐ ┌──────────────┐ ┌──────────────┐
│   Worker 1   │ │   Worker 2   │ │   Worker 3   │
│              │ │              │ │              │
│ - Executes   │ │ - Executes   │ │ - Executes   │
│   subtask    │ │   subtask    │ │   subtask    │
│              │ │              │ │              │
│ Tools:       │ │ Tools:       │ │ Tools:       │
│ shell,       │ │ shell,       │ │ shell,       │
│ read_file,   │ │ read_file,   │ │ read_file,   │
│ edit_file,   │ │ edit_file,   │ │ edit_file,   │
│ write_file,  │ │ write_file,  │ │ write_file,  │
│ glob, grep   │ │ glob, grep   │ │ glob, grep   │
└──────────────┘ └──────────────┘ └──────────────┘
```

### Parallel Execution

```rust
// DelegateBatchTool spawns workers concurrently
let handles: Vec<_> = subtasks
    .into_iter()
    .map(|subtask| {
        let worker = create_worker();
        tokio::spawn(async move {
            worker.run_task(&subtask).await
        })
    })
    .collect();

let results = futures::future::join_all(handles).await;
```

## Error Handling

### Error Hierarchy

```
CrewError
├── ApiError { status, message, suggestions }
├── TaskNotFound { task_id }
├── ApiKeyNotSet { provider, env_var }
├── ConfigError { path, details }
├── ToolError { tool, message }
└── IoError(std::io::Error)
```

### Retry Strategy

```
┌─────────────┐
│  API Call   │
└──────┬──────┘
       │
       ▼
┌─────────────┐     Success
│  Response   │────────────────▶ Return
└──────┬──────┘
       │ Error
       ▼
┌─────────────┐
│ Retryable?  │
│ (429, 5xx)  │
└──────┬──────┘
       │
  ┌────┴────┐
  │ Yes     │ No
  ▼         ▼
┌─────┐   ┌─────┐
│Wait │   │Fail │
│(exp │   │     │
│back)│   └─────┘
└──┬──┘
   │
   ▼
┌─────────────┐
│ Retry < 3?  │
└──────┬──────┘
       │
  ┌────┴────┐
  │ Yes     │ No
  ▼         ▼
Back to   ┌─────┐
API Call  │Fail │
          └─────┘
```

## File Structure

```
crew-rs/
├── Cargo.toml                 # Workspace definition
├── README.md                  # Project overview
├── docs/
│   ├── PRD.md                 # Product requirements
│   ├── ARCHITECTURE.md        # This document
│   └── USER_MANUAL.md         # User guide
└── crates/
    ├── crew-core/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs         # Module exports
    │       ├── task.rs        # Task types
    │       ├── types.rs       # Common types
    │       └── error.rs       # Error types
    ├── crew-llm/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs         # Module exports
    │       ├── provider.rs    # LlmProvider trait
    │       ├── config.rs      # ChatConfig
    │       ├── types.rs       # Response types
    │       ├── retry.rs       # RetryProvider
    │       ├── anthropic.rs   # Anthropic provider
    │       ├── openai.rs      # OpenAI provider
    │       └── gemini.rs      # Gemini provider
    ├── crew-memory/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs         # Module exports
    │       ├── episode.rs     # Episode types
    │       ├── episode_store.rs
    │       └── task_store.rs
    ├── crew-agent/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs         # Module exports
    │       ├── agent.rs       # Agent runtime
    │       ├── progress.rs    # Progress reporting
    │       ├── policy.rs      # Command policy
    │       └── tools/
    │           ├── mod.rs     # Tool registry
    │           ├── shell.rs
    │           ├── read_file.rs
    │           ├── write_file.rs
    │           ├── edit_file.rs
    │           ├── glob_tool.rs
    │           ├── grep_tool.rs
    │           ├── delegate.rs
    │           └── delegate_batch.rs
    └── crew-cli/
        ├── Cargo.toml
        ├── src/
        │   ├── main.rs        # Entry point
        │   ├── config.rs      # Config loading
        │   └── commands/
        │       ├── mod.rs     # Command enum
        │       ├── init.rs
        │       ├── run.rs
        │       ├── resume.rs
        │       ├── list.rs
        │       ├── status.rs
        │       ├── clean.rs
        │       └── completions.rs
        └── tests/
            └── cli_tests.rs   # Integration tests
```

## Security Considerations

### Current Implementation

1. **API Key Handling**: Keys read from environment variables, never stored in config
2. **File Access**: Tools operate within working directory
3. **Shell Execution**: Commands executed with user privileges

### Planned Enhancements

1. **Command Policy**: Approve/deny shell commands based on patterns
2. **Sandbox Integration**: Use codex-linux-sandbox for isolation
3. **Audit Logging**: Record all tool executions

## Performance Characteristics

| Operation | Expected Latency | Notes |
|-----------|-----------------|-------|
| CLI startup | < 50ms | No network calls |
| Config load | < 5ms | JSON parsing |
| File read | < 10ms | Depends on size |
| File write | < 10ms | Depends on size |
| Glob search | < 100ms | Depends on patterns |
| Grep search | < 500ms | Uses ignore crate |
| LLM call | 1-30s | Network dependent |
| State save | < 50ms | JSON serialization |

## Testing Strategy

### Unit Tests
- Type serialization/deserialization
- Tool argument parsing
- Error formatting
- Config validation

### Integration Tests
- CLI command execution
- File operation tools
- State persistence
- Shell completions

### Manual Testing
- End-to-end task execution
- Provider switching
- Resume functionality
- Coordinator mode

## Future Architecture

### MCP Server Mode

```
┌─────────────┐     MCP Protocol     ┌─────────────┐
│   Client    │◄────────────────────▶│  crew-mcp   │
│  (VS Code)  │                      │   Server    │
└─────────────┘                      └──────┬──────┘
                                            │
                                            ▼
                                     ┌─────────────┐
                                     │ crew-agent  │
                                     └─────────────┘
```

### Distributed Execution

```
┌─────────────┐
│ Coordinator │
│   (Local)   │
└──────┬──────┘
       │
       ▼
┌─────────────┐     Network     ┌─────────────┐
│   Daemon    │◄───────────────▶│   Daemon    │
│  (Host A)   │                 │  (Host B)   │
└──────┬──────┘                 └──────┬──────┘
       │                               │
       ▼                               ▼
┌─────────────┐                 ┌─────────────┐
│  Workers    │                 │  Workers    │
└─────────────┘                 └─────────────┘
```
