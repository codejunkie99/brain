import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useEffect, useState } from "react";

import * as bridge from "@/ipc/bridge";
import type { McpConfigSnippets, McpStatus } from "@/ipc/types";

import { Button } from "../common/Button";
import { Pill } from "../common/Pill";

export function McpPanel() {
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [snippets, setSnippets] = useState<McpConfigSnippets | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function refresh() {
    try {
      const [st, sn] = await Promise.all([
        bridge.mcpStatus(),
        bridge.mcpConfigSnippets().catch(() => null),
      ]);
      setStatus(st);
      setSnippets(sn);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    void refresh();
  }, []);

  return (
    <div style={{ maxWidth: 720 }}>
      <h2 style={{ fontSize: 16, margin: "0 0 12px" }}>MCP host</h2>
      <p style={{ color: "var(--fg-2)", fontSize: 12, lineHeight: 1.6, marginBottom: 16 }}>
        External agents (Claude Code, Cursor, the Codex CLI) attach to
        the same brain memory as this IDE through MCP. The IDE can
        launch <code>brain serve --mcp</code> for the active brain dir,
        or you can paste the snippets below into each agent's config.
      </p>

      {error && (
        <div
          style={{
            color: "var(--red)",
            fontSize: 12,
            marginBottom: 12,
          }}
        >
          {error}
        </div>
      )}

      {status && (
        <div
          style={{
            padding: 16,
            border: "1px solid var(--line-1)",
            borderRadius: 10,
            background: "var(--bg-1)",
            display: "flex",
            alignItems: "center",
            gap: 12,
            marginBottom: 16,
          }}
        >
          <Pill
            color={status.running ? "var(--green)" : "var(--fg-3)"}
            tone="soft"
            size="sm"
          >
            {status.running ? "running" : "stopped"}
          </Pill>
          <div style={{ flex: 1, fontSize: 12, color: "var(--fg-2)" }}>
            <div>
              binary <code>{status.binary}</code>
            </div>
            <div>
              brain dir{" "}
              <code>{status.brain_dir ?? "(no brain open)"}</code>
            </div>
            {status.pid && <div>pid {status.pid}</div>}
          </div>
          {status.running ? (
            <Button
              variant="danger"
              size="sm"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                try {
                  await bridge.mcpStop();
                  await refresh();
                } finally {
                  setBusy(false);
                }
              }}
            >
              Stop
            </Button>
          ) : (
            <Button
              variant="primary"
              size="sm"
              disabled={busy || !status.brain_dir}
              onClick={async () => {
                setBusy(true);
                try {
                  await bridge.mcpStart();
                  await refresh();
                } catch (e) {
                  setError(String(e));
                } finally {
                  setBusy(false);
                }
              }}
            >
              Start
            </Button>
          )}
        </div>
      )}

      {snippets && (
        <>
          <Snippet
            label="Claude Code (~/.claude/mcp_servers.json)"
            value={JSON.stringify(snippets.claude_desktop, null, 2)}
          />
          <Snippet
            label="Cursor (<project>/.cursor/mcp.json)"
            value={JSON.stringify(snippets.cursor, null, 2)}
          />
          <Snippet
            label="Codex (~/.codex/config.toml)"
            value={snippets.codex_toml}
            lang="toml"
          />
        </>
      )}
    </div>
  );
}

function Snippet({
  label,
  value,
  lang = "json",
}: {
  label: string;
  value: string;
  lang?: string;
}) {
  const [copied, setCopied] = useState(false);
  return (
    <div style={{ marginBottom: 12 }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          marginBottom: 4,
          fontSize: 11,
          color: "var(--fg-3)",
          textTransform: "uppercase",
          letterSpacing: 0.4,
        }}
      >
        <span style={{ flex: 1 }}>{label}</span>
        <button
          onClick={async () => {
            await writeText(value);
            setCopied(true);
            window.setTimeout(() => setCopied(false), 1500);
          }}
          style={{
            color: copied ? "var(--green)" : "var(--fg-2)",
            fontSize: 11,
          }}
        >
          {copied ? "copied" : "copy"}
        </button>
      </div>
      <pre
        style={{
          margin: 0,
          padding: 12,
          background: "var(--bg-0)",
          border: "1px solid var(--line-1)",
          borderRadius: 6,
          color: "var(--fg-1)",
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          lineHeight: 1.5,
          overflowX: "auto",
          userSelect: "text",
        }}
      >
        <code>{value}</code>
      </pre>
      <div
        style={{
          fontSize: 10,
          color: "var(--fg-3)",
          marginTop: 4,
        }}
      >
        {lang}
      </div>
    </div>
  );
}
