# Product Requirements Document: crew-rs

## Executive Summary

crew-rs is a Rust-native coding agent orchestration framework that enables developers to deploy, coordinate, and manage AI coding agents for software engineering tasks. The framework provides a simple CLI interface for running autonomous coding tasks with support for multiple LLM providers, resumable execution, and episodic memory.

## Problem Statement

### Current Challenges

1. **Fragmented AI Coding Tools**: Existing solutions are often Python-based, slow, or tightly coupled to specific LLM providers
2. **No Task Persistence**: When AI coding sessions are interrupted, all progress is lost
3. **Limited Coordination**: Most tools support single-agent execution without task decomposition
4. **Vendor Lock-in**: Switching between LLM providers requires significant code changes
5. **Resource Waste**: No mechanism to limit token usage or control costs

### Target Users

- **Individual Developers**: Automate repetitive coding tasks
- **Development Teams**: Parallelize code changes across multiple agents
- **DevOps Engineers**: Integrate AI coding into CI/CD pipelines
- **Open Source Maintainers**: Triage and address issues at scale

## Product Vision

Build the fastest, most reliable coding agent framework in Rust that:
- Runs 10x faster than Python-based alternatives
- Supports seamless provider switching
- Enables complex task coordination
- Provides full execution observability

## Functional Requirements

### FR-1: Task Execution

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-1.1 | Execute coding tasks from natural language goals | P0 |
| FR-1.2 | Support maximum iteration limits | P0 |
| FR-1.3 | Support token budget limits | P0 |
| FR-1.4 | Display real-time progress during execution | P0 |
| FR-1.5 | Handle graceful interruption (Ctrl+C) | P0 |

### FR-2: Task Persistence

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-2.1 | Save task state on interruption | P0 |
| FR-2.2 | Resume interrupted tasks | P0 |
| FR-2.3 | List all resumable tasks | P0 |
| FR-2.4 | Show task status and details | P1 |
| FR-2.5 | Clean up stale task files | P1 |

### FR-3: LLM Provider Support

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-3.1 | Support Anthropic Claude models | P0 |
| FR-3.2 | Support OpenAI GPT models | P0 |
| FR-3.3 | Support Google Gemini models | P1 |
| FR-3.4 | Automatic retry on transient errors | P0 |
| FR-3.5 | Custom base URL for proxies/compatible APIs | P1 |

### FR-4: Tool System

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-4.1 | Shell command execution | P0 |
| FR-4.2 | File read operations | P0 |
| FR-4.3 | File write operations | P0 |
| FR-4.4 | File edit with search/replace | P0 |
| FR-4.5 | Glob pattern file search | P0 |
| FR-4.6 | Grep content search | P0 |
| FR-4.7 | Task delegation (coordinator mode) | P1 |
| FR-4.8 | Parallel batch delegation | P2 |

### FR-5: Configuration

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-5.1 | Project-local configuration | P0 |
| FR-5.2 | Global user configuration | P1 |
| FR-5.3 | Interactive configuration setup | P0 |
| FR-5.4 | Environment variable expansion | P1 |
| FR-5.5 | Configuration validation | P1 |

### FR-6: Developer Experience

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-6.1 | Shell completion scripts | P1 |
| FR-6.2 | Verbose output mode | P1 |
| FR-6.3 | Colored terminal output | P1 |
| FR-6.4 | Clear error messages with suggestions | P0 |

## Non-Functional Requirements

### NFR-1: Performance

| ID | Requirement | Target |
|----|-------------|--------|
| NFR-1.1 | CLI startup time | < 50ms |
| NFR-1.2 | Memory usage (idle) | < 50MB |
| NFR-1.3 | File operation latency | < 10ms |
| NFR-1.4 | Concurrent tool execution | Supported |

### NFR-2: Reliability

| ID | Requirement | Target |
|----|-------------|--------|
| NFR-2.1 | State persistence durability | 100% on clean shutdown |
| NFR-2.2 | Retry success rate (transient errors) | > 95% |
| NFR-2.3 | Graceful degradation on API errors | Required |

### NFR-3: Security

| ID | Requirement | Target |
|----|-------------|--------|
| NFR-3.1 | No secrets in config files | Required |
| NFR-3.2 | API keys via environment variables | Required |
| NFR-3.3 | Shell command policy enforcement | Planned |
| NFR-3.4 | Sandboxed execution | Planned |

### NFR-4: Compatibility

| ID | Requirement | Target |
|----|-------------|--------|
| NFR-4.1 | Linux support | Required |
| NFR-4.2 | macOS support | Required |
| NFR-4.3 | Windows support | Best effort |
| NFR-4.4 | Minimum Rust version | 1.85.0 |

## User Stories

### Epic 1: Basic Task Execution

**US-1.1**: As a developer, I want to run a coding task with a single command so that I can automate repetitive work.

```bash
crew run "Add input validation to the login form"
```

**US-1.2**: As a developer, I want to see real-time progress so that I know what the agent is doing.

**US-1.3**: As a developer, I want to interrupt a task with Ctrl+C and resume later so that I don't lose progress.

### Epic 2: Provider Flexibility

**US-2.1**: As a developer, I want to switch between LLM providers so that I can use the best model for each task.

```bash
crew run --provider gemini "Optimize database queries"
```

**US-2.2**: As a developer, I want automatic retries on API errors so that transient failures don't stop my work.

### Epic 3: Cost Control

**US-3.1**: As a developer, I want to set a token budget so that I don't exceed my API spending limits.

```bash
crew run --max-tokens 50000 "Refactor the authentication module"
```

**US-3.2**: As a developer, I want to limit iterations so that runaway tasks don't consume resources indefinitely.

### Epic 4: Team Collaboration

**US-4.1**: As a team lead, I want to use coordinator mode to break complex tasks into subtasks.

```bash
crew run --coordinate "Implement user authentication with OAuth"
```

## Success Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| Task completion rate | > 80% | Tasks completed / tasks started |
| Average task duration | < 5 min | For typical file edits |
| User retention (weekly) | > 60% | Active users week-over-week |
| Error rate | < 5% | Failed tasks / total tasks |
| Provider switch rate | > 20% | Users trying multiple providers |

## Roadmap

### Phase 1: Foundation (Completed)
- [x] Core type system
- [x] LLM provider abstraction
- [x] Basic tool system
- [x] CLI interface
- [x] Task persistence

### Phase 2: Production Ready (Current)
- [x] Retry mechanism
- [x] Token budgeting
- [x] Multi-provider support (Anthropic, OpenAI, Gemini)
- [x] Shell completions
- [x] Comprehensive testing
- [ ] Command policy enforcement
- [ ] Sandbox integration

### Phase 3: Advanced Features (Planned)
- [ ] MCP server mode
- [ ] Interactive TUI
- [ ] Streaming responses
- [ ] Custom tool plugins
- [ ] Distributed execution

### Phase 4: Enterprise (Future)
- [ ] Team management
- [ ] Usage analytics
- [ ] Audit logging
- [ ] SSO integration

## Appendix

### A. Competitive Analysis

| Feature | crew-rs | Codex CLI | Aider | Continue |
|---------|-----------|-----------|-------|----------|
| Language | Rust | Rust | Python | TypeScript |
| Multi-provider | Yes | No | Yes | Yes |
| Task persistence | Yes | Yes | No | No |
| Coordinator mode | Yes | No | No | No |
| Token budgets | Yes | No | No | No |

### B. Technology Stack

- **Language**: Rust 2024 Edition
- **Async Runtime**: Tokio
- **HTTP Client**: Reqwest with rustls
- **Database**: redb (embedded)
- **CLI Framework**: Clap
- **Error Handling**: eyre

### C. API Dependencies

- Anthropic Messages API v1
- OpenAI Chat Completions API v1
- Google Gemini API v1beta
