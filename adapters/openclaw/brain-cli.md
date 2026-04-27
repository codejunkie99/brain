# OpenClaw — `brain` CLI integration (v0.1)

OpenClaw doesn't speak MCP directly today, so we wire the `brain` Rust
runtime in as a CLI that the agent shells out to. The agent gets the
same five operations (note, log, ask, doctor, ping) via Bash tool
calls.

**Paste this block into your OpenClaw system prompt** (in addition to
the file-based include at `adapters/openclaw/config.md`).

---

You have access to a persistent memory layer called **brain**, a
git-backed event log with full-text search. When you learn something
worth remembering, save it. Before doing non-trivial work, search it.

## Commands

- **Save a note**
  ```bash
  brain note "picked fastapi-users over authlib for PKCE ergonomics"
  ```

- **Search prior notes** (prefix-matching by default; no wildcard needed)
  ```bash
  brain ask "auth"
  brain ask "fastapi"
  ```

- **See recent** (last 20, newest first)
  ```bash
  brain log
  ```

- **Health check**
  ```bash
  brain doctor
  ```

## When to save

- User states a preference or lesson. Save before answering.
- You made a non-obvious choice between options. Save why.
- A test / migration / deploy just succeeded or failed with a lesson.
- Before committing a bug fix, save the root cause.

## When to search

- Before running deploys, migrations, schema changes.
- Before choosing a library, pattern, or config value.
- When the user asks "didn't we fix this before?" or "what did I say about X?".
- At the start of a non-trivial task — 2 seconds to search saves 20 minutes of redo.

## Behavior rules

- NEVER echo the contents of a note back to the terminal before saving.
  The prefilter rejects secrets, but belt-and-braces.
- If `brain` isn't installed, fail gracefully — don't try to save.
  Tell the user to run `brew install codejunkie99/tap/brain`.
- Default location is `~/.brain`. Override with `BRAIN_DIR` env var
  or `--brain-dir <path>`.

---

## Install

```bash
brew install codejunkie99/tap/brain
brain onboard --agents openclaw --yes
```

Verify from the shell:
```bash
brain note "hello openclaw"
brain log
```
