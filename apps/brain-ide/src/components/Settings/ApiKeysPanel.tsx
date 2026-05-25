import { useState } from "react";

import * as bridge from "@/ipc/bridge";
import { useAgentsStore } from "@/state/agents";

import { Button } from "../common/Button";
import { Pill } from "../common/Pill";

interface KeyDef {
  kind: "anthropic" | "openai";
  label: string;
  hint: string;
}

const KEYS: KeyDef[] = [
  {
    kind: "anthropic",
    label: "Anthropic (Claude)",
    hint: "Used for Claude chat + spec planning. Get one at console.anthropic.com.",
  },
  {
    kind: "openai",
    label: "OpenAI (Codex)",
    hint: "Used for Codex / GPT chat. Get one at platform.openai.com.",
  },
];

export function ApiKeysPanel() {
  const statuses = useAgentsStore((s) => s.credentials);
  const reload = useAgentsStore((s) => s.reload);
  const [drafts, setDrafts] = useState<Record<string, string>>({});

  return (
    <div style={{ maxWidth: 640 }}>
      <h2 style={{ fontSize: 16, margin: "0 0 16px" }}>API keys</h2>
      <p style={{ fontSize: 12, color: "var(--fg-2)", lineHeight: 1.6, marginBottom: 20 }}>
        Keys are stored in the macOS Keychain. They never leave your machine
        except as outbound requests to the model providers.
      </p>
      {KEYS.map((k) => {
        const status = statuses.find((s) => s.kind === k.kind);
        const isSet = status?.is_set ?? false;
        const draft = drafts[k.kind] ?? "";
        return (
          <div
            key={k.kind}
            style={{
              padding: 16,
              border: "1px solid var(--line-1)",
              borderRadius: 10,
              background: "var(--bg-1)",
              marginBottom: 12,
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
              <span style={{ fontWeight: 600, fontSize: 13 }}>{k.label}</span>
              <Pill color={isSet ? "var(--green)" : "var(--orange)"} tone="soft" size="xs">
                {isSet ? "configured" : "not set"}
              </Pill>
            </div>
            <p style={{ fontSize: 12, color: "var(--fg-2)", margin: "0 0 12px" }}>{k.hint}</p>
            <div style={{ display: "flex", gap: 6 }}>
              <input
                type="password"
                value={draft}
                onChange={(e) =>
                  setDrafts((d) => ({ ...d, [k.kind]: e.target.value }))
                }
                placeholder={isSet ? "•••••••• (already set)" : "sk-..."}
                style={{
                  flex: 1,
                  background: "var(--bg-2)",
                  color: "var(--fg-0)",
                  border: "1px solid var(--line-1)",
                  borderRadius: 6,
                  padding: "6px 10px",
                  fontSize: 12,
                  fontFamily: "var(--font-mono)",
                }}
              />
              <Button
                variant="primary"
                size="sm"
                disabled={!draft.trim()}
                onClick={async () => {
                  await bridge.setCredential(k.kind, draft.trim());
                  setDrafts((d) => ({ ...d, [k.kind]: "" }));
                  void reload();
                }}
              >
                Save
              </Button>
              {isSet && (
                <Button
                  variant="danger"
                  size="sm"
                  onClick={async () => {
                    await bridge.clearCredential(k.kind);
                    void reload();
                  }}
                >
                  Clear
                </Button>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}
