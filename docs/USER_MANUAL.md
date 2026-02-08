# User Manual: crew-rs

## Table of Contents

1. [Introduction](#introduction)
2. [Installation](#installation)
3. [Quick Start](#quick-start)
4. [Configuration](#configuration)
5. [Commands Reference](#commands-reference)
6. [Working with Providers](#working-with-providers)
7. [Task Management](#task-management)
8. [Advanced Usage](#advanced-usage)
9. [Troubleshooting](#troubleshooting)
10. [Best Practices](#best-practices)

---

## Introduction

crew-rs is a command-line tool for running AI coding agents. It enables you to:

- Execute coding tasks using natural language instructions
- Switch between multiple LLM providers (Anthropic, OpenAI, Google)
- Interrupt and resume long-running tasks
- Control costs with token budgets
- Coordinate complex tasks across multiple agents

### Key Concepts

| Term | Description |
|------|-------------|
| **Task** | A unit of work defined by a natural language goal |
| **Agent** | An AI that executes tasks using tools |
| **Tool** | A capability like reading files or running commands |
| **Provider** | An LLM API service (Anthropic, OpenAI, Gemini) |
| **Coordinator** | An agent that breaks down complex goals into subtasks |
| **Worker** | An agent that executes individual subtasks |

---

## Installation

### Prerequisites

- Rust 1.85.0 or later
- An API key from at least one supported provider

### From Source

```bash
# Clone the repository
git clone https://github.com/heyong4725/crew-rs
cd crew-rs

# Build and install
cargo install --path crates/crew-cli

# Verify installation
crew --version
```

### Setting Up API Keys

Set your API key as an environment variable:

```bash
# For Anthropic (Claude)
export ANTHROPIC_API_KEY="sk-ant-..."

# For OpenAI (GPT)
export OPENAI_API_KEY="sk-..."

# For Google (Gemini)
export GEMINI_API_KEY="..."
```

Add to your shell profile (`~/.bashrc`, `~/.zshrc`) for persistence.

---

## Quick Start

### 1. Initialize Configuration

```bash
cd your-project
crew init
```

Follow the interactive prompts to select your provider and model.

### 2. Run Your First Task

```bash
crew run "Add a hello world function to src/main.rs"
```

### 3. View Progress

The agent will show real-time progress:

```
crew-rs

Goal: Add a hello world function to src/main.rs
Working dir: /home/user/project
Provider: anthropic
Model: claude-sonnet-4-20250514

────────────────────────────────────────────────────────────

⟳ Thinking... (iteration 1)
✓ read_file (45ms)
✓ edit_file (12ms)

◆ I've added a hello world function to src/main.rs

────────────────────────────────────────────────────────────

✓ Completed 2 iterations, 3.2s

Output:
Added the function `fn hello_world()` that prints "Hello, World!"

Files modified:
  - src/main.rs
```

---

## Configuration

### Configuration Files

crew-rs looks for configuration in this order:

1. `.crew/config.json` in the current directory (project-specific)
2. `~/.config/crew/config.json` (global)

### Creating Configuration

**Interactive:**
```bash
crew init
```

**With defaults:**
```bash
crew init --defaults
```

### Configuration Options

```json
{
  "provider": "anthropic",
  "model": "claude-sonnet-4-20250514",
  "base_url": null,
  "api_key_env": "ANTHROPIC_API_KEY"
}
```

| Field | Description | Default |
|-------|-------------|---------|
| `provider` | LLM provider to use | `"anthropic"` |
| `model` | Model identifier | Provider-specific |
| `base_url` | Custom API endpoint | Provider default |
| `api_key_env` | Environment variable for API key | Provider-specific |

### Environment Variable Expansion

Use `${VAR_NAME}` syntax in configuration:

```json
{
  "base_url": "${ANTHROPIC_BASE_URL}",
  "model": "${CREW_MODEL}"
}
```

### Default Models by Provider

| Provider | Default Model | Alternatives |
|----------|---------------|--------------|
| anthropic | claude-sonnet-4-20250514 | claude-opus-4-20250514, claude-3-5-haiku-20241022 |
| openai | gpt-4o | gpt-4o-mini, gpt-4-turbo |
| gemini | gemini-2.0-flash | gemini-2.0-flash-lite, gemini-1.5-pro |

---

## Commands Reference

### `crew init`

Initialize a new configuration in the current directory.

```bash
crew init [OPTIONS]

Options:
  -c, --cwd <PATH>    Working directory (default: current)
      --defaults      Skip prompts, use defaults
  -h, --help          Show help
```

**Examples:**
```bash
# Interactive setup
crew init

# Non-interactive with defaults
crew init --defaults

# Initialize in specific directory
crew init --cwd /path/to/project
```

---

### `crew run`

Execute a task with an AI agent.

```bash
crew run [OPTIONS] <GOAL>

Arguments:
  <GOAL>    Natural language description of the task

Options:
  -c, --cwd <PATH>              Working directory
      --config <PATH>           Path to config file
      --provider <NAME>         LLM provider (anthropic, openai, gemini)
      --model <NAME>            Model identifier
      --base-url <URL>          Custom API endpoint
      --coordinate              Run as coordinator
      --max-iterations <N>      Maximum iterations (default: 50)
      --max-tokens <N>          Token budget limit
  -v, --verbose                 Show detailed output
      --no-retry                Disable automatic retry
  -h, --help                    Show help
```

**Examples:**

```bash
# Basic task
crew run "Fix the bug in auth.rs"

# Use specific provider and model
crew run --provider openai --model gpt-4o "Add unit tests"

# With iteration limit
crew run --max-iterations 20 "Refactor the database module"

# With token budget
crew run --max-tokens 50000 "Implement user authentication"

# Verbose output
crew run -v "Add error handling to API endpoints"

# Coordinator mode for complex tasks
crew run --coordinate "Build a REST API for user management"
```

---

### `crew resume`

Resume an interrupted task.

```bash
crew resume [OPTIONS] [TASK_ID]

Arguments:
  [TASK_ID]    Task ID to resume (optional - lists if omitted)

Options:
  -c, --cwd <PATH>              Working directory
      --config <PATH>           Path to config file
      --provider <NAME>         Override LLM provider
      --model <NAME>            Override model
      --base-url <URL>          Override API endpoint
      --max-iterations <N>      Maximum additional iterations
      --max-tokens <N>          Token budget limit
  -v, --verbose                 Show detailed output
      --no-retry                Disable automatic retry
  -h, --help                    Show help
```

**Examples:**

```bash
# List resumable tasks
crew resume

# Resume specific task
crew resume abc12345

# Resume with different model
crew resume abc12345 --model claude-opus-4-20250514

# Resume with additional iterations
crew resume abc12345 --max-iterations 100
```

---

### `crew list`

List all resumable tasks.

```bash
crew list [OPTIONS]

Options:
  -c, --cwd <PATH>    Working directory
  -h, --help          Show help
```

**Example Output:**

```
crew-rs

Resumable tasks:

  abc12345 Add authentication to API [coordinator]
    Progress: 15420 input, 3200 output tokens
    Updated: 2024-01-15 14:30:22

  def67890 Fix database connection pool... [worker]
    Progress: 8200 input, 1500 output tokens
    Updated: 2024-01-15 14:25:10
```

---

### `crew status`

Show detailed information about a task.

```bash
crew status [OPTIONS] <TASK_ID>

Arguments:
  <TASK_ID>    Task ID to inspect

Options:
  -c, --cwd <PATH>    Working directory
  -h, --help          Show help
```

**Example Output:**

```
crew-rs

Task: abc12345

Kind: Code
  Instruction: Add authentication to API

Status: InProgress
Role: Coordinator

Token Usage:
  Input:  15,420
  Output:  3,200
  Total:  18,620

Timeline:
  Created: 2024-01-15 14:20:00
  Updated: 2024-01-15 14:30:22

Context:
  Working directory: /home/user/api-project

Message count: 24
```

---

### `crew clean`

Remove stale task state files.

```bash
crew clean [OPTIONS]

Options:
  -c, --cwd <PATH>    Working directory
      --all           Remove all data including databases
      --dry-run       Show what would be deleted
  -h, --help          Show help
```

**Examples:**

```bash
# Preview what would be deleted
crew clean --dry-run

# Remove completed task files
crew clean

# Remove everything (databases, all state)
crew clean --all
```

---

### `crew completions`

Generate shell completion scripts.

```bash
crew completions <SHELL>

Arguments:
  <SHELL>    Target shell (bash, zsh, fish, powershell)
```

**Installation by Shell:**

```bash
# Bash
crew completions bash > ~/.local/share/bash-completion/completions/crew

# Zsh (add to ~/.zshrc: fpath=(~/.zfunc $fpath))
crew completions zsh > ~/.zfunc/_crew

# Fish
crew completions fish > ~/.config/fish/completions/crew.fish

# PowerShell (add to $PROFILE)
crew completions powershell >> $PROFILE
```

---

## Working with Providers

### Anthropic (Claude)

**Setup:**
```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

**Models:**
- `claude-sonnet-4-20250514` - Balanced performance (default)
- `claude-opus-4-20250514` - Highest capability
- `claude-3-5-haiku-20241022` - Fastest, lowest cost

**Usage:**
```bash
crew run --provider anthropic --model claude-opus-4-20250514 "Complex refactoring task"
```

### OpenAI (GPT)

**Setup:**
```bash
export OPENAI_API_KEY="sk-..."
```

**Models:**
- `gpt-4o` - Latest GPT-4 (default)
- `gpt-4o-mini` - Faster, lower cost
- `gpt-4-turbo` - Previous generation

**Usage:**
```bash
crew run --provider openai --model gpt-4o-mini "Quick code fix"
```

### Google Gemini

**Setup:**
```bash
export GEMINI_API_KEY="..."
```

**Models:**
- `gemini-2.0-flash` - Fast and capable (default)
- `gemini-2.0-flash-lite` - Fastest
- `gemini-1.5-pro` - Highest capability

**Usage:**
```bash
crew run --provider gemini --model gemini-1.5-pro "Large codebase analysis"
```

### Custom Endpoints

Use `--base-url` for proxies or compatible APIs:

```bash
# Azure OpenAI
crew run --provider openai \
  --base-url "https://your-resource.openai.azure.com/openai/deployments/gpt-4" \
  "Task description"

# Local proxy
crew run --provider anthropic \
  --base-url "http://localhost:8080" \
  "Task description"
```

---

## Task Management

### Understanding Task State

When you run a task, crew-rs saves state to `.crew/tasks/`:

```
.crew/
├── config.json
├── tasks/
│   ├── abc12345.json    # Task state
│   └── def67890.json
└── episodes.redb        # Episodic memory
```

### Interrupting Tasks

Press `Ctrl+C` to gracefully interrupt a running task:

```
⟳ Thinking... (iteration 15)

⚠ Received Ctrl+C, saving state...
⚠ Interrupted after 15 iterations. State saved. Run 'crew resume' to continue.
```

### Resuming Tasks

```bash
# List available tasks
crew resume

# Resume by ID
crew resume abc12345
```

### Cleaning Up

```bash
# Remove completed task files
crew clean

# Remove all state (start fresh)
crew clean --all
```

---

## Advanced Usage

### Coordinator Mode

For complex tasks, use coordinator mode to decompose work:

```bash
crew run --coordinate "Implement a complete user authentication system with login, logout, and password reset"
```

The coordinator will:
1. Analyze the goal
2. Break it into subtasks
3. Delegate to worker agents
4. Aggregate results

### Token Budgets

Control costs by limiting token usage:

```bash
# Limit to 50,000 total tokens
crew run --max-tokens 50000 "Large refactoring task"
```

When the budget is exceeded:
```
⚠ Token budget exceeded (52,340 used, 50,000 limit). Increase with --max-tokens
```

### Iteration Limits

Prevent runaway tasks:

```bash
# Limit to 20 iterations
crew run --max-iterations 20 "Quick fix"
```

### Verbose Mode

See detailed tool execution:

```bash
crew run -v "Add logging to all functions"
```

Output includes:
- Full tool outputs
- Token usage per iteration
- Detailed timing information

### Disabling Retry

For debugging or when retries aren't desired:

```bash
crew run --no-retry "Test task"
```

---

## Troubleshooting

### Common Errors

#### API Key Not Set

```
Error: ANTHROPIC_API_KEY environment variable not set
```

**Solution:** Set the appropriate environment variable:
```bash
export ANTHROPIC_API_KEY="your-key-here"
```

#### Rate Limited (429)

```
Error: Anthropic API error: 429 - rate limit exceeded
```

**Solution:** The retry mechanism handles this automatically. If persistent:
- Wait and retry later
- Use a different provider
- Reduce request frequency

#### Max Iterations Reached

```
⚠ Reached max iterations limit (50). Increase with --max-iterations
```

**Solution:**
```bash
crew resume <task-id> --max-iterations 100
```

#### Token Budget Exceeded

```
⚠ Token budget exceeded (52,340 used, 50,000 limit).
```

**Solution:**
```bash
crew resume <task-id> --max-tokens 100000
```

### Debug Logging

Enable detailed logging:

```bash
RUST_LOG=debug crew run "Task"
```

Log levels:
- `error` - Only errors
- `warn` - Warnings and errors
- `info` - General information
- `debug` - Detailed debugging
- `trace` - Very verbose

### Checking Configuration

Verify your configuration:

```bash
# Show what config would be used
cat .crew/config.json

# Test with verbose output
crew run -v "Simple test task"
```

---

## Best Practices

### Writing Good Goals

**Be specific:**
```bash
# Good
crew run "Add input validation to the login form in src/components/Login.tsx"

# Too vague
crew run "Fix the login"
```

**Include context:**
```bash
# Good
crew run "Add a rate limiter to the /api/auth endpoints using the existing Redis client"

# Missing context
crew run "Add rate limiting"
```

**Break down large tasks:**
```bash
# Instead of one huge task
crew run "Build a complete e-commerce system"

# Use coordinator mode or multiple smaller tasks
crew run --coordinate "Implement product catalog with search and filtering"
crew run "Add shopping cart functionality"
crew run "Implement checkout flow"
```

### Managing Costs

1. **Start with smaller models:**
   ```bash
   crew run --model gpt-4o-mini "Quick prototype"
   ```

2. **Set token budgets:**
   ```bash
   crew run --max-tokens 30000 "Focused task"
   ```

3. **Limit iterations:**
   ```bash
   crew run --max-iterations 10 "Simple fix"
   ```

4. **Review before resuming:**
   ```bash
   crew status <task-id>  # Check token usage
   ```

### Project Organization

1. **Use project-specific config:**
   ```bash
   crew init  # In each project
   ```

2. **Add .crew to .gitignore:**
   ```gitignore
   # .gitignore
   .crew/tasks/
   .crew/*.redb
   ```

3. **Version control config:**
   ```bash
   # Keep config, ignore state
   git add .crew/config.json
   ```

### Security

1. **Never commit API keys:**
   ```bash
   # Use environment variables
   export ANTHROPIC_API_KEY="..."
   ```

2. **Review generated code:**
   - Always review changes before committing
   - Run tests after agent modifications

3. **Limit shell access in production:**
   - Use in development environments
   - Consider sandboxing for untrusted code

---

## Appendix

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Ctrl+C` | Interrupt and save state |

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Configuration error |
| 130 | Interrupted (Ctrl+C) |

### Environment Variables

| Variable | Description |
|----------|-------------|
| `ANTHROPIC_API_KEY` | Anthropic API key |
| `OPENAI_API_KEY` | OpenAI API key |
| `GEMINI_API_KEY` | Google Gemini API key |
| `RUST_LOG` | Log level (error, warn, info, debug, trace) |

### File Locations

| Path | Description |
|------|-------------|
| `.crew/config.json` | Project configuration |
| `.crew/tasks/` | Task state files |
| `.crew/episodes.redb` | Episodic memory database |
| `~/.config/crew/config.json` | Global configuration |
