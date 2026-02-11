# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```bash
cargo build --workspace          # Build all crates
cargo test --workspace           # Run all tests
cargo test -p crew-agent         # Test single crate
cargo test -p crew-agent test_name  # Run single test
cargo clippy --workspace         # Lint
cargo fmt --all                  # Format
cargo fmt --all -- --check       # Check formatting
cargo install --path crates/crew-cli  # Install CLI locally
```

## Architecture

crew-rs is a Rust-native AI coding agent framework. 5-crate workspace, layered:

```
crew-cli  (CLI: clap commands, config loading)
    |
crew-agent  (Agent loop, tool system, progress reporting)
    |          \
crew-memory   crew-llm  (redb episodes + JSON task state | LLM providers)
    \           /
    crew-core  (Task, Message, AgentRole, Error types - no internal deps)
```

### Key Flow: Agent Loop (`crew-agent/src/agent.rs`)

1. Build messages (system prompt + conversation history + episodic memory context)
2. Call LLM with tool specs
3. If tool calls returned -> execute tools -> append results -> loop
4. If EndTurn or budget exceeded -> return result
5. State saved to `.crew/tasks/{id}.json` after each iteration (enables Ctrl+C resume)

### Coordinator/Worker Pattern

- **Worker** (default): Has file/shell/search tools, executes tasks directly
- **Coordinator** (`--coordinate`): Additionally has `delegate_task` and `delegate_batch` tools, decomposes goals into subtasks, spawns worker agents via `tokio::spawn`

### Tool System (`crew-agent/src/tools/`)

All tools implement `Tool` trait (`spec() -> ToolSpec`, `execute(&Value) -> ToolResult`). Registered in `ToolRegistry` (HashMap). Tools: shell, read_file, write_file, edit_file, glob, grep, delegate_task, delegate_batch.

### LLM Providers (`crew-llm/src/`)

`LlmProvider` trait with `chat()` method. Three providers: `AnthropicProvider`, `OpenAIProvider`, `GeminiProvider`. `RetryProvider` wraps any provider with exponential backoff on 429/5xx.

### Memory (`crew-memory/src/`)

- `EpisodeStore`: redb database at `.crew/episodes.redb`, stores task completion summaries, queried by keyword relevance
- `TaskStore`: JSON files at `.crew/tasks/`, enables resume of interrupted tasks

## Key Types

- `Task` (crew-core): UUID v7 ID, kind (Code/Plan/Review/Custom), status, context
- `Message` (crew-core): role (System/User/Assistant/Tool), content, tool_call_id
- `ChatResponse` (crew-llm): content, tool_calls, stop_reason, token usage
- `AgentConfig` (crew-agent): max_iterations (default 50), max_tokens, save_episodes

## Project Conventions

- Edition 2024, rust-version 1.85.0
- Pure Rust TLS via rustls (no OpenSSL dependency)
- `eyre`/`color-eyre` for error handling (not `anyhow`)
- `Arc<dyn Trait>` for shared providers/tools/reporters
- `AtomicBool` for shutdown signaling
- API keys always from env vars via `api_key_env` config field
- `ShellTool` has `SafePolicy` that denies dangerous commands (rm -rf /, dd, mkfs, fork bomb)
