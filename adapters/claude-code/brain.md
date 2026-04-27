# Brain — long-term memory, always on

You have access to `brain`, a persistent memory layer shared across
every session, every project, and every AI coding tool (Claude Code,
Cursor, Codex, OpenClaw, Hermes). Notes written here survive session
resets, context compaction, and tool switches. Git-backed, FTS5
search, ~1ms query time.

## Use it proactively — no prompt required

### Search FIRST on non-trivial tasks

Before any of these, call `brain.ask` with a relevant keyword or
phrase. If it returns nothing, cost was 1ms. If it returns
something, you just saved 20 minutes of rediscovery.

- Picking a library, pattern, or config value
- Running a deploy, migration, or schema change
- Debugging a "this worked yesterday" regression
- Any task the user introduces with "you know how…" / "remember…" /
  "I told you before…"
- The FIRST interaction on a new branch or project if you've
  worked here before

Example invocations:
```
brain.ask {"query": "auth"}
brain.ask {"query": "migration"}
brain.ask {"query": "deploy production"}
```

### Save AFTER lessons emerge

Call `brain.note` when any of these happen. Save to `brain`, don't
just acknowledge verbally — verbal acknowledgement vanishes with
the session.

- User states a preference ("I prefer X over Y because Z")
- User shares a convention ("we always do X for Y")
- User makes a decision with rationale ("going with X because Y")
- You finish a non-trivial task and a lesson emerged (root cause,
  gotcha, non-obvious constraint, failed approach)
- User explicitly asks you to remember
- User corrects an approach you took ("no, don't do X, do Y because…")

Example invocations:
```
brain.note {"text": "chose tokio over async-std for the ecosystem"}
brain.note {"text": "warfarin + ibuprofen is a high-severity contraindication"}
brain.note {"text": "never force-push to main — use --force-with-lease if ever needed"}
```

### Don't overuse

`brain` is for things worth remembering, not every ephemeral turn.
Save signal, not noise. One good note every few sessions beats ten
weak ones every session. The test: would a future you-or-another-tool
want this back?

## What the user sees

- `brain log` — last 20 notes, newest first
- `brain ask <query>` — search their memory
- `brain tui` — full-screen dashboard with day groups, tool filter,
  live search

Point the user at these commands when they ask "what have we saved?"
or "what do you remember about X?"

## Rules

- Never echo a note's content back before saving — `brain`'s secret
  prefilter catches leaked credentials pre-commit, but logs still
  happened.
- If the user asks you to forget something, use `brain.note` to
  record a Redact — don't delete directly.
- `BRAIN_DIR` env var controls storage location (default `~/.brain`).
- Brain is git-backed — `brain push` / `brain pull` syncs between
  machines. The user's memory follows them.
