# crew-rs

Rust-native coding agent orchestration framework. Build fast, modular AI coding agents with multi-provider LLM support.

## Features

- **Multi-provider LLM support**: Anthropic (Claude), OpenAI (GPT-4), Google Gemini
- **Task coordination**: Coordinator/worker pattern for complex tasks
- **Resumable tasks**: Interrupt with Ctrl+C and resume later
- **Episodic memory**: Agents learn from past task completions
- **Built-in tools**: Shell, file read/write/edit, glob, grep
- **Retry with backoff**: Automatic retry on transient API errors
- **Token budgeting**: Limit total tokens per task

## Installation

```bash
# From source
cargo install --path crates/crew-cli

# Or build locally
cargo build --release
./target/release/crew --help
```

## Quick Start

```bash
# Initialize configuration
crew init

# Set your API key
export ANTHROPIC_API_KEY=your-key-here

# Run a task
crew run "Add a hello function to lib.rs"
```

## Commands

### `crew init`

Initialize `.crew/config.json` in your project:

```bash
crew init              # Interactive setup
crew init --defaults   # Use defaults (Anthropic/Claude)
```

### `crew run <goal>`

Execute a task with an AI agent:

```bash
# Basic usage
crew run "Fix the bug in auth.rs"

# With options
crew run "Refactor the API" \
  --provider openai \
  --model gpt-4o \
  --max-iterations 100 \
  --max-tokens 50000 \
  --verbose

# Run as coordinator (decomposes into subtasks)
crew run "Add user authentication" --coordinate
```

**Options:**
- `--provider`: LLM provider (`anthropic`, `openai`, `gemini`)
- `--model`: Model name (e.g., `claude-sonnet-4-20250514`, `gpt-4o`, `gemini-2.0-flash`)
- `--max-iterations`: Maximum agent loop iterations (default: 50)
- `--max-tokens`: Token budget limit (input + output)
- `--verbose`: Show tool outputs
- `--no-retry`: Disable automatic retry on errors
- `--coordinate`: Run as coordinator with subtask delegation

### `crew resume [task-id]`

Resume an interrupted task:

```bash
crew resume              # List resumable tasks
crew resume abc123       # Resume specific task
```

### `crew list`

List all resumable tasks:

```bash
crew list
```

### `crew status <task-id>`

Show details of a specific task:

```bash
crew status abc123
```

### `crew clean`

Clean up task state files:

```bash
crew clean              # Remove completed task files
crew clean --all        # Remove all task data including databases
crew clean --dry-run    # Show what would be deleted
```

### `crew completions <shell>`

Generate shell completions:

```bash
# Bash
crew completions bash > ~/.local/share/bash-completion/completions/crew

# Zsh
crew completions zsh > ~/.zfunc/_crew

# Fish
crew completions fish > ~/.config/fish/completions/crew.fish

# PowerShell
crew completions powershell >> $PROFILE
```

## Configuration

Configuration is loaded from (in order):
1. `.crew/config.json` in current directory
2. `~/.config/crew/config.json` (global)

### Example config

```json
{
  "provider": "anthropic",
  "model": "claude-sonnet-4-20250514",
  "api_key_env": "ANTHROPIC_API_KEY"
}
```

### Environment variable expansion

Config values support `${VAR_NAME}` syntax:

```json
{
  "base_url": "${ANTHROPIC_BASE_URL}"
}
```

### Supported providers

| Provider | API Key Env | Default Model |
|----------|-------------|---------------|
| anthropic | `ANTHROPIC_API_KEY` | claude-sonnet-4-20250514 |
| openai | `OPENAI_API_KEY` | gpt-4o |
| gemini | `GEMINI_API_KEY` | gemini-2.0-flash |

## Architecture

```
crew-rs/
  crates/
    crew-core/      # Types, task model, protocols
    crew-memory/    # Episodic memory store
    crew-llm/       # LLM provider abstraction
    crew-agent/     # Agent runtime, tools, coordination
    crew-cli/       # CLI interface
```

### Agent Roles

- **Worker**: Executes tasks directly using tools
- **Coordinator**: Decomposes complex goals into subtasks, delegates to workers

### Built-in Tools

| Tool | Description |
|------|-------------|
| `shell` | Execute shell commands |
| `read_file` | Read file contents |
| `write_file` | Write/create files |
| `edit_file` | Edit files with search/replace |
| `glob` | Find files by pattern |
| `grep` | Search file contents |
| `delegate_task` | (Coordinator) Assign subtask to worker |
| `delegate_batch` | (Coordinator) Assign multiple subtasks in parallel |

## Development

```bash
# Build
cargo build --workspace

# Test
cargo test --workspace

# Lint
cargo clippy --workspace

# Format
cargo fmt --all
```

## License

Apache-2.0
