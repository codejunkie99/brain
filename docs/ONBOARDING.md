# Onboarding

Run:

```bash
brain onboard
```

It creates or keeps `~/.brain`, checks local health, and can wire your agents
to the same memory. It is safe to re-run.

## Choose Agents

At the prompt, type numbers, agent names, `all`, or `none`:

```text
1,3,5
claude code
wire claude code and cursor
codex openclaw hermes
all
none
```

Before saving, onboarding previews every file it will create, append, replace,
or skip.

## Script It

```bash
brain onboard --agents all --yes
brain onboard --agents claude-code,cursor,codex --yes
brain onboard --agents openclaw,hermes --yes
brain onboard --agents none --yes
```

Use `--reconfigure` to refresh managed wiring:

```bash
brain onboard --agents all --yes --reconfigure
```

## Files Written

| Agent | Files |
|---|---|
| Claude Code | `~/.claude/mcp_servers.json`, `~/.claude/CLAUDE.md` |
| Cursor | `<project>/.cursor/mcp.json`, `<project>/.cursor/rules/brain.mdc` |
| Codex | `~/.codex/config.toml`, `~/.codex/AGENTS.md` |
| OpenClaw | `~/.openclaw/workspace/BRAIN.md` |
| Hermes | `<project>/AGENTS.md` |

Existing files are not overwritten by default. Managed prompt blocks use
`BRAIN:START` / `BRAIN:END` markers.

## Try It

```bash
brain note "remember that auth uses PKCE"
brain ask "auth"
brain log
brain tui
```

## Fix Search Or Log

```bash
brain doctor
brain doctor --deep
```

`doctor --deep` rebuilds the local SQLite index from git.
