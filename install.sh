#!/usr/bin/env bash
# install.sh — wire `brain` into your AI coding harness.
#
# Usage:  ./install.sh <harness> [--mcp] [project-dir]
#
# Harnesses: claude-code | cursor | codex | openclaw | hermes
#
# What it does per harness:
#   claude-code → ~/.claude/mcp_servers.json + ~/.claude/CLAUDE.md
#   cursor      → .cursor/mcp.json + .cursor/rules/brain.mdc
#   codex       → ~/.codex/config.toml + ~/.codex/AGENTS.md
#   openclaw    → ~/.openclaw/workspace/BRAIN.md
#   hermes      → <project>/AGENTS.md
#
# Idempotent. Re-run safely; `<!-- BRAIN:START -->` markers prevent
# duplicate appends.

set -euo pipefail

HARNESS="${1:-}"
HERE="$(cd "$(dirname "$0")" && pwd)"
TARGET="$PWD"
MCP=0

if [[ -z "$HARNESS" ]]; then
  cat <<'USAGE' >&2
brain install script

Usage: ./install.sh <harness> [--mcp] [project-dir]

Harnesses:
  claude-code  — MCP + ~/.claude/CLAUDE.md
  cursor       — MCP + .cursor/rules/brain.mdc
  codex        — MCP + ~/.codex/AGENTS.md
  openclaw     — CLI + ~/.openclaw/workspace/BRAIN.md
  hermes       — CLI + project AGENTS.md

First time? Install the brain binary:
  cargo install --path crates/brain-cli

Then wire it into your harness:
  ./install.sh claude-code

Verify:
  brain note "hello"
  brain log
USAGE
  exit 2
fi

shift || true
while [[ $# -gt 0 ]]; do
  case "$1" in
    --mcp)
      MCP=1
      shift
      ;;
    -*)
      echo "error: unknown option '$1'" >&2
      exit 2
      ;;
    *)
      TARGET="$1"
      shift
      ;;
  esac
done

SRC="$HERE/adapters/$HARNESS"
if [[ ! -d "$SRC" ]]; then
  echo "error: adapter '$HARNESS' not found at $SRC" >&2
  echo "available: claude-code cursor codex openclaw hermes" >&2
  exit 1
fi

if [[ $MCP -eq 1 ]]; then
  echo "wiring brain into $HARNESS (runtime enabled)..."
else
  echo "wiring brain into $HARNESS..."
fi

# Warn if brain isn't on PATH. Don't fail — config is static, user can
# install the binary later.
if ! command -v brain &>/dev/null; then
  echo "  ! 'brain' not on \$PATH"
  echo "    install: cd $HERE && cargo install --path crates/brain-cli"
  echo "    (continuing; the config works once brain is installed)"
fi

# Append a marked block to a file, idempotent via BRAIN:START marker.
append_brain_block() {
  local target="$1" source="$2" label="$3"
  mkdir -p "$(dirname "$target")"
  touch "$target"
  if grep -q "BRAIN:START" "$target" 2>/dev/null; then
    echo "  ~ $target already has brain block; skipping"
    return
  fi
  {
    echo ""
    echo "<!-- BRAIN:START — managed by brain install.sh -->"
    cat "$source"
    echo ""
    echo "<!-- BRAIN:END -->"
  } >> "$target"
  echo "  + appended brain block to $target ($label)"
}

case "$HARNESS" in
  claude-code)
    mkdir -p "$HOME/.claude"
    if [[ ! -f "$HOME/.claude/mcp_servers.json" ]]; then
      cp "$SRC/mcp.json" "$HOME/.claude/mcp_servers.json"
      echo "  + ~/.claude/mcp_servers.json (brain MCP server)"
    else
      echo "  ~ ~/.claude/mcp_servers.json exists; merge manually:"
      echo "    cat $SRC/mcp.json"
    fi
    append_brain_block "$HOME/.claude/CLAUDE.md" "$SRC/brain.md" "user-wide"
    ;;

  cursor)
    mkdir -p "$TARGET/.cursor"
    if [[ ! -f "$TARGET/.cursor/mcp.json" ]]; then
      cp "$SRC/mcp.json" "$TARGET/.cursor/mcp.json"
      echo "  + $TARGET/.cursor/mcp.json (brain MCP server)"
    else
      echo "  ~ $TARGET/.cursor/mcp.json exists; merge manually"
    fi
    mkdir -p "$TARGET/.cursor/rules"
    if [[ ! -f "$TARGET/.cursor/rules/brain.mdc" ]]; then
      cp "$SRC/brain.mdc" "$TARGET/.cursor/rules/brain.mdc"
      echo "  + $TARGET/.cursor/rules/brain.mdc (alwaysApply rule)"
    else
      echo "  ~ $TARGET/.cursor/rules/brain.mdc exists; skipping"
    fi
    ;;

  codex)
    mkdir -p "$HOME/.codex"
    CODEX_CFG="$HOME/.codex/config.toml"
    touch "$CODEX_CFG"
    if grep -q "^\[mcp_servers\.brain\]" "$CODEX_CFG" 2>/dev/null; then
      echo "  ~ $CODEX_CFG already has [mcp_servers.brain]; skipping"
    else
      echo "" >> "$CODEX_CFG"
      cat "$SRC/config.toml" >> "$CODEX_CFG"
      echo "  + appended [mcp_servers.brain] to $CODEX_CFG"
    fi
    append_brain_block "$HOME/.codex/AGENTS.md" "$SRC/AGENTS.md" "user-wide"
    ;;

  openclaw)
    mkdir -p "$HOME/.openclaw/workspace"
    OC_BRAIN="$HOME/.openclaw/workspace/BRAIN.md"
    if [[ -f "$OC_BRAIN" ]]; then
      echo "  ~ $OC_BRAIN exists; skipping"
    else
      cp "$SRC/brain-cli.md" "$OC_BRAIN"
      echo "  + $OC_BRAIN (auto-loaded by OpenClaw workspace scan)"
    fi
    ;;

  hermes)
    append_brain_block "$TARGET/AGENTS.md" "$SRC/brain-cli.md" "project-local"
    ;;

  *)
    echo "error: unknown harness '$HARNESS'" >&2
    exit 1
    ;;
esac

echo ""
echo "  verify: brain note 'testing brain' && brain log"
echo "  tip:    brain tui   — dashboard across all tools"
