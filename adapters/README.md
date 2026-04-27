# Agent Wiring

Use onboarding:

```bash
brain onboard
```

Script it:

```bash
brain onboard --agents all --yes
brain onboard --agents claude-code,cursor,codex --yes
brain onboard --agents openclaw,hermes --yes
```

## What Gets Wired

| Agent | Runtime | Files |
|---|---|---|
| Claude Code | MCP | `~/.claude/mcp_servers.json`, `~/.claude/CLAUDE.md` |
| Cursor | MCP | `<project>/.cursor/mcp.json`, `<project>/.cursor/rules/brain.mdc` |
| Codex | MCP | `~/.codex/config.toml`, `~/.codex/AGENTS.md` |
| OpenClaw | CLI | `~/.openclaw/workspace/BRAIN.md` |
| Hermes | CLI | `<project>/AGENTS.md` |

Existing files are not overwritten by default.
