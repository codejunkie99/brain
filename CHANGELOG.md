# Changelog

## Unreleased

### Added

- `brain onboard` guided first-run setup, modeled after OpenClaw's CLI
  onboarding style: keep existing state, create missing local memory, show what
  was written, run health checks, and print next commands. The onboarding
  output uses terminal color when supported and respects `NO_COLOR`.
- Agent selection inside `brain onboard`: users can wire Claude Code, Cursor,
  Codex, OpenClaw, and/or Hermes from one terminal flow, review the exact files
  that will change, then save. Scripted installs can use
  `brain onboard --agents claude-code,codex --yes`; adapter templates are
  embedded in the binary so this works from Homebrew installs too.
- Onboarding now accepts displayed agent names with spaces, such as
  `Claude Code`, in addition to ids like `claude-code`.
- The onboarding chooser scans the whole answer with word-based regexes, so
  natural inputs like `wire claude code and cursor` work too.

## v0.1.0 — initial release

First standalone release. Split from the agentic-stack parent project.

### Core

- **brain-types** — typed events (Observe / Claim / Lesson / Pref / SkillEdit /
  Verify / Archive / Redact / Import / Audit), UUIDv7 event IDs, idempotency
  keys, `shallow_validate` with `time_observed` range check.
- **brain-store** — git-backed event log via libgit2. One commit per event,
  one file per event. Multi-writer retry with compare-and-swap on HEAD.
- **brain-index** — SQLite FTS5 index with `unicode61` tokenizer, prefix
  indexing on 2/3/4 chars, BM25 ranking (title 10x body 5x tags). Current-
  truth projections for Pref and Claim. Redact rolls back projections to
  the most recent non-redacted predecessor.
- **brain-app** — orchestration layer. Two-phase write (git first, index
  best-effort). Catch-up reconciliation on open via set-difference against
  indexed ids. Rebuild self-heal on schema mismatch.
- **brain-mcp** — rmcp stdio server exposing `ping / note / log / ask /
  doctor` tools with prescriptive descriptions.
- **brain-cli** — `brain` binary: `init / note / log / ask / doctor / tui /
  serve --mcp / push / pull / remote`.
- **brain-tui** — ratatui full-screen dashboard. Day-group list with per-
  tool glyphs, live search, in-UI push/pull, compose modal.

### Security

- 18-pattern secret prefilter (Anthropic / OpenAI / GitHub / AWS / GCP /
  Twilio / Stripe / Slack / Vault / Azure / JWT / PEM / Bearer) with NFKC
  normalization and zero-width stripping.
- Commit-trailer injection defense via trailer-block parsing.
- Commit-subject scrubbing for Observe / Lesson / Redact (no user text in
  `git log --oneline`).
- Blob/trailer cross-check + filename-in-parent skip for commit forgery.
- Detached-HEAD reject at open and append time.
- 0700 on brain dir, 0600 on sqlite index on first create.
- Watermark pinned to HEAD after catch-up and rebuild.
- Topological revwalk for deterministic replay under non-linear history.
- Redact rolls back projections to predecessor (not erase).

### Adapters

- **claude-code** — MCP + `~/.claude/CLAUDE.md` addendum
- **cursor** — MCP + `.cursor/rules/brain.mdc` with `alwaysApply: true`
- **codex** — MCP TOML + `~/.codex/AGENTS.md` addendum
- **openclaw** — `~/.openclaw/workspace/BRAIN.md` (CLI shell-out, no MCP)
- **hermes** — project `AGENTS.md` addendum (CLI shell-out, no MCP)

### Tests

147 across 7 crates. Forgery tests use git2 to construct malformed
commits and assert defenses fire. Redact rollback tests cover the
no-predecessor, with-predecessor, and skips-redacted-events cases.

### Shipped during 13 adversarial review passes

- R5: catch-up replay ordering, SCHEMA integer parse, rebuild self-heal
- R6–R10: secret prefilter widening, commit subject scrubbing, NFKC pass
- R11: blob/trailer cross-check, filename-in-parent skip
- R12: blob-content-swap defense
- R13: revwalk topological, redact rollback, detached-HEAD reject
- R14: terminal guard race, compose buffer on save error
- R15+: source-field FTS pollution, typed StoreError variants, install.sh
  for all 5 harnesses
