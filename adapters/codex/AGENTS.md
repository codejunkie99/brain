# Brain — long-term memory, always on

You have access to `brain`, a persistent memory layer shared across
every session, every project, and every AI coding tool (Claude Code,
Cursor, Codex, OpenClaw, Hermes). Notes written here survive session
resets, context compaction, and tool switches. Git-backed, FTS5
search, ~1ms query time. Invoked via MCP tools `brain.note`,
`brain.ask`, `brain.log`, `brain.doctor`, `brain.ping`.

## Use it proactively — no prompt required

### Search FIRST on non-trivial tasks

Before any of these, call `brain.ask` with a relevant keyword or
phrase. If it returns nothing, cost was 1ms. If it returns
something, you just saved 20 minutes of rediscovery.

- Picking a library, pattern, or config value
- Running a deploy, migration, or schema change
- Debugging a "this worked yesterday" regression
- Any task the user introduces with "you know how…" / "remember…"
- The FIRST interaction on a new branch or project you've seen before

### Save AFTER lessons emerge

Call `brain.note` (don't just acknowledge verbally — verbal memory
vanishes with the session).

- User states a preference ("I prefer X over Y because Z")
- User shares a convention ("we always do X for Y")
- User makes a decision with rationale
- You finish a non-trivial task and a lesson emerged (root cause,
  gotcha, non-obvious constraint, failed approach)
- User explicitly asks you to remember
- User corrects an approach ("no, don't do X, do Y because…")

### Don't overuse

`brain` is for things worth remembering, not every ephemeral turn.
Save signal, not noise. Test: would a future you-or-another-tool
want this back?

## Rules

- Never echo a note's content back before saving — `brain`'s secret
  prefilter catches leaked credentials pre-commit, but logs happen.
- If the user asks you to forget something, use `brain.note` to
  record a Redact — don't delete directly.
- Brain is git-backed — `brain push` / `brain pull` syncs between
  machines.
