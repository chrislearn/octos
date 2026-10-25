# Compact tools without capability loss

The four-tool evaluation allowlist excluded useful tools. It was not a general
replacement for the full coding environment. The built-in `coding` profile also
filters out capabilities such as web access and multi-file patching. Neither
filter should be used to infer that those implementations are unnecessary.

Select the new profile with `octos chat --profile coding-compact`. It restores
the full **authorized** registry, presents canonical entry points, and retains
plugin and MCP tools. Existing host allowlists, provider denials, filesystem
scopes and sandbox policies still apply. The older `coding` and `coding-full`
profiles remain available. This change does not rewrite saved evaluation runs.

Embedders can call `ToolRegistry::set_compact_surface(true)`, or set a profile's
`compact_tool_surface` to `true`. `OCTOS_TOOL_SURFACE=compact` selects compact
presentation for built-in registry construction; an explicit profile setting
overrides it. Setting presentation on an already restricted registry cannot
restore tools that its authorization filter removed.

## Audit and decisions

| Original capability | Useful? | Implementation decision |
|---|---|---|
| `read_file`, `view_image` | Yes: source and visual evidence | Present `read_file`; retain image attachment and legacy `view_image` execution. Add `files` batches. |
| `write_file`, `edit_file` | Yes: new files and precise edits | Keep both. `edit_file.edits` already applies several replacements in one call. |
| `apply_patch`, `diff_edit` | Yes: multi-file edits and unified diffs | Present `apply_patch`, which already accepts both patch text and `{path, diff}`. Keep the legacy entry point. |
| `exec_command`, `bash` | Yes: running and verifying work | Present `exec_command`, with PTY, persistent sessions and recoverable output. Keep legacy `bash` execution. |
| `shell`, `write_stdin` | Yes: detached jobs and interactive sessions | Keep: detached supervisor jobs and PTY input/polling are distinct lifecycle operations. |
| `glob`, `grep`, `list_dir` | Yes: efficient code discovery | Keep typed searches, contextual matches and directory inspection. Replacing them with Bash loses useful output and validation contracts. |
| `web_search`, `web_fetch`, `browser` | Yes: external evidence and UI work | Batch search/fetch requests; retain browser separately because it has interactive state and actions. |
| `spawn`, `spawn_agent`, `delegate`, send/resume/wait/close | Yes: parallel work | Preserve lifecycle semantics. `wait_agent(include_output=true)` collects status, artifact paths and final output together. |
| `check_background_tasks`, `read_task_output`, peer tools | Yes: supervised/external work | Preserve session-scoped inspection, log paging and peer identity. These are dynamically registered, not always part of the base count. |
| workspace log/show/diff, `check`, contract check | Yes: review and verification | Keep exact validation and history operations. They are not equivalent to reporting progress in prose. |
| `tool_search`, `tool_suggest` | Yes: integration discovery | Present `tool_search`; both use the live registry catalog. |
| `ask_user_question`, `request_user_input` | Yes: missing user input | Prefer the answer-routed `ask_user_question`; retain the older entry point for compatibility. |
| `update_plan` | Yes: user-visible progress | Keep; do not confuse planning with execution evidence. |
| `view_video`, optional `git`/`code_structure` | Yes: distinct media/code capabilities | Keep when built and authorized. |
| unbound `image_generation` | Stub is not usable | Omit the stub in compact mode. A real implementation is available by default and remains visible. |

Compact suppression is conditional: if a replacement is absent, hidden, denied
or unavailable, the original entry remains advertised. Spec and discovery
ordering remains deterministic. Registry snapshots inherit the chosen surface.
No tool implementation is deleted; historical calls still pass through their
ordinary execution, permission, hook and sandbox paths.

## Batching that reduces calls

Read several files or images in one model-visible call:

```json
{"files":[{"path":"src/main.rs","start_line":20,"limit":60},{"path":"screenshots/result.png"}]}
```

For `web_search` and `web_fetch`, the batch field is `requests`. Each item uses
the original single-request parameters, for example:

```json
{"requests":[{"query":"official documentation topic A"},{"query":"official documentation topic B"}]}
```

Batches report individual failures, preserve request order and pass all items
through the original read/network checks. A call accepts up to 32 requests and
runs up to four I/O reads concurrently; these are per-call resource bounds,
not limits on agents or worker count. Cancellation drops pending reads. Output
fits the existing tool budget and identifies truncated items. Truncated batch
reads do not mark files as completely observed in the file-state cache. Image
bytes are delivered to the main model and saved immutably by the existing
executor. Path-filtered hooks inspect every file in a read batch.

Collect completed agents without a separate result call:

```json
{"targets":["exact-task-id"],"timeout_ms":30000,"include_output":true,"max_output_bytes":4096}
```

The output budget is shared across targets. Unknown or out-of-session IDs are
not treated as completed. Missing final output is represented as `null`, not
as a successful empty answer. Large logs still use `read_task_output` paging.

## Boundaries and validation

With optional Git/AST tools disabled, the same batch-enabled build exposes 34
built-in specs in native mode (23,631 serialized JSON bytes), or 28 in compact
mode (21,017 bytes): six fewer entries and 11.1% fewer schema bytes. Plugin and
MCP entries are additional. These are schema-byte measurements, not billed
token counts, and compare the two modes of this build rather than a live run
against the earlier four-tool configuration.

This is not a universal dispatcher. Approval rules, progress accounting and
hooks are keyed to native tools; hiding arbitrary calls inside another tool
would require more than renaming schemas. Commands and dependent verification
still require sequencing: a normal mixed batch executes safe reads before
mutations, so do not place a read-after-write dependency in that batch.

Tests cover capability retention, fallback when a replacement is denied,
host restrictions, scope and SSRF checks, hook denial of a batched path, main
history image delivery, snapshot immutability, cancellation, truncation/cache
interaction and session-isolated agent results. Tool/schema counts measure
presentation overhead only. Fewer calls or names do not establish faster model
completion, higher cache hit rate, or better PPTX quality; those require another
controlled live evaluation.

Validation on 2026-10-24: the full agent library run passed 3,105 tests with
three existing ignored tests; after removing duplicated batch-schema help,
the focused tool run passed 1,287 tests with two ignored, and all eight compact
surface integration tests passed. Agent Clippy (`-D warnings`), the CLI check
(`--no-default-features --features api`), formatting and `git diff --check`
passed. No paid-provider or live PPTX benchmark was run for this change.
