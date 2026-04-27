# Hermes Agent — `brain` CLI integration (v0.1)

Hermes Agent uses native `AGENTS.md` for workspace-level context. This
file is a system-prompt addendum teaching the agent to use the
`brain` Rust runtime for persistent memory. Include it after
`AGENTS.md` in your Hermes project.

---

You have access to a persistent memory layer called **brain**, a
git-backed event log with full-text search. It's complementary to
Hermes's built-in `MEMORY.md` / `USER.md` / `SOUL.md`: use Hermes's
files for agent identity + user preferences, use `brain` for
individual observations, claims, and lessons.

## Commands

- **Save an observation**
  ```bash
  brain note "picked fastapi-users over authlib for PKCE ergonomics"
  ```

- **Search** (prefix-matching by default)
  ```bash
  brain ask "auth"
  ```

- **Recent**
  ```bash
  brain log
  ```

- **Health**
  ```bash
  brain doctor
  ```

## When to save vs. where

| Content | Goes to |
|---------|---------|
| Ephemeral turn notes | Hermes scratch |
| User preferences + conventions | Hermes `USER.md` |
| Agent identity + persona | Hermes `SOUL.md` |
| Specific observations + lessons + decisions | `brain note` |
| Source-of-truth claims (typed) | `brain` via MCP once supported |

## Rules

- Don't echo secrets to the terminal even when you think you're just
  about to save them — `brain`'s prefilter catches them pre-commit,
  but logs still happened.
- If `brain` isn't on `$PATH`, fall back to Hermes's `MEMORY.md`.
- `BRAIN_DIR` env var overrides the default `~/.brain` location.

---

## Install

```bash
brew install codejunkie99/tap/brain
brain onboard --agents hermes --yes
```

Verify:
```bash
brain note "hello hermes"
brain ask hello
```
