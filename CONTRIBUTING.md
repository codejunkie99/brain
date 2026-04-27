# Contributing

Thanks for working on `brain`. The project is intentionally small-crate Rust:
each crate owns one boundary, and git remains the source of truth for memory.

## Local setup

```bash
cargo install --path crates/brain-cli
cargo test --workspace
```

## Quality checks

Run these before opening a pull request:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Project boundaries

- `brain-types`: schema and shared domain types.
- `brain-store`: git storage, append semantics, secret detection, forgery
  defenses.
- `brain-index`: SQLite FTS5 index and projections.
- `brain-app`: orchestration, catch-up, rebuild, sync.
- `brain-mcp`: MCP server surface.
- `brain-cli`: user-facing command line.
- `brain-tui`: full-screen terminal UI.

Avoid making storage, indexing, and UI changes in one patch unless the behavior
requires it.

## Security-sensitive changes

Add regression tests for any change touching:

- secret detection or rejected-note output;
- redaction/archive visibility;
- git commit parsing, trailers, or event ids;
- catch-up/rebuild watermarks;
- MCP stdout/stderr behavior.

Fake credential-looking strings should be clearly test-only and should not be
real tokens.
