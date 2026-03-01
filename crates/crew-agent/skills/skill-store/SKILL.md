---
name: skill-store
description: Browse, install, update, and manage skill packages from the registry.
version: 1.0.0
author: hagency
always: true
---

# Skill Store

IMPORTANT: You MUST use the shell tool to run `crew skills` commands. Do NOT use web search or try to look up skills online. The registry is accessed via the CLI command only.

When the user asks to browse skills, install skills, find available skills, show skill store, or similar (including Chinese: 技能商店, 安装技能, 查看技能, 浏览技能, 搜索技能):

## Browse Available Packages

```bash
# List all packages in the registry
crew skills search --cwd {{CWD}}

# Filter by keyword
crew skills search mofa --cwd {{CWD}}
crew skills search news --cwd {{CWD}}
```

Show the command output directly to the user.

## Install a Package

```bash
# Install all skills from a repo
crew skills install <user/repo> --cwd {{CWD}}

# Install a single skill from a multi-skill repo
crew skills install <user/repo/skill-name> --cwd {{CWD}}
```

Examples:
```bash
crew skills install hagency-org/app-skills --cwd {{CWD}}
crew skills install hagency-org/app-skills/weather --cwd {{CWD}}
```

Options:
- `--force` to overwrite existing skills
- `--branch <tag>` for a specific version

## Verify Installation

```bash
crew skills list --cwd {{CWD}}
```

## Get Skill Details

```bash
crew skills info <skill-name> --cwd {{CWD}}
```

Shows version, author, tools provided, source repo, and install date.

## Update Skills

```bash
# Update a single skill
crew skills update <skill-name> --cwd {{CWD}}

# Update all installed skills
crew skills update all --cwd {{CWD}}
```

## Remove a Skill

```bash
crew skills remove <skill-name> --cwd {{CWD}}
```

Tell the user what was installed and any requirements (e.g. API keys, binaries).
