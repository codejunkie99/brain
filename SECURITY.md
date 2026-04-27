# Security Policy

`brain` is local-first software that stores memory as git commits. Treat the
brain repo the way you would treat private notes or source history.

## Supported versions

Security fixes target the latest `main` branch until tagged releases become
available.

## Reporting a vulnerability

Please open a private GitHub security advisory for this repository. If that is
not available, contact the maintainer through the GitHub profile linked from
the repository owner.

Useful reports include:

- How the issue can be reproduced.
- Whether it affects `brain-store`, `brain-index`, `brain-mcp`, adapters, or
  the CLI/TUI.
- Whether secrets or private memory could land in git history, logs, stdout,
  MCP responses, or the derived SQLite index.

## Design expectations

- Free-form notes must pass through the secret prefilter before commit.
- CLI and MCP paths must not echo rejected note content.
- Git history is the source of truth; derived indexes must be rebuildable.
- Redacted and archived events must stay hidden from default search/log views.
- The MCP server must write logs to stderr, never stdout.
