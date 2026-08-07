// Right-rail "inspector" for the selected card. Shows context, the
// live tail of the agent's log output, and lets the user manually
// transition the card or open a terminal targeting it.

import { useMemo } from "react";

import * as bridge from "@/ipc/bridge";
import { useAgentsStore } from "@/state/agents";
import { useGraphStore } from "@/state/graph";
import { useTerminalsStore } from "@/state/terminals";
import { kindAccent, statusColor } from "@/theme/tokens";

import { Button } from "../common/Button";
import { Icon } from "../common/Icon";
import { Pill } from "../common/Pill";

export function CardInspector() {
  const card = useGraphStore((s) =>
    s.selectedCardId ? s.cards[s.selectedCardId] : null,
  );
  const log = useGraphStore((s) =>
    s.selectedCardId ? s.cardLogs[s.selectedCardId] : null,
  );
  const select = useGraphStore((s) => s.selectCard);
  const agents = useAgentsStore((s) =>
    s.descriptors.filter((d) => d.supports_card_execution),
  );

  const tailLines = useMemo(() => log?.logs.slice(-200) ?? [], [log]);

  if (!card) {
    return (
      <aside
        style={{
          width: 320,
          padding: 16,
          borderLeft: "1px solid var(--line-0)",
          background: "var(--bg-1)",
          color: "var(--fg-3)",
          fontSize: 12,
        }}
      >
        Select a card to inspect it.
      </aside>
    );
  }

  async function openTerminal() {
    if (!card) return;
    await useTerminalsStore.getState().spawn({
      cwd: undefined,
      card_id: card.id,
      label: card.title,
    });
  }

  return (
    <aside
      style={{
        width: 360,
        borderLeft: "1px solid var(--line-0)",
        background: "var(--bg-1)",
        display: "flex",
        flexDirection: "column",
        minHeight: 0,
      }}
    >
      <header
        style={{
          padding: "10px 12px",
          borderBottom: "1px solid var(--line-0)",
          display: "flex",
          alignItems: "center",
          gap: 8,
        }}
      >
        <span
          style={{
            width: 6,
            height: 28,
            borderRadius: 3,
            background: kindAccent[card.kind],
          }}
        />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div
            style={{
              fontSize: 13,
              fontWeight: 600,
              color: "var(--fg-0)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {card.title}
          </div>
          <div style={{ fontSize: 10, color: "var(--fg-3)" }}>
            {card.kind} · {card.id.slice(0, 8)}
          </div>
        </div>
        <button onClick={() => select(null)} title="Close" style={{ color: "var(--fg-3)" }}>
          ✕
        </button>
      </header>

      <div style={{ padding: 12, display: "flex", flexDirection: "column", gap: 12, overflowY: "auto" }}>
        <Pill color={statusColor[card.status]} tone="solid" size="sm">
          {card.status}
        </Pill>

        <section>
          <SectionTitle>Summary</SectionTitle>
          <div style={{ fontSize: 12, color: "var(--fg-1)", lineHeight: 1.5 }}>
            {card.summary || "—"}
          </div>
        </section>

        <section>
          <SectionTitle>Assignment</SectionTitle>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
            <select
              value={card.assigned_agent ?? ""}
              onChange={(e) => {
                const agent = e.target.value || null;
                void bridge.assignCard({ id: card.id, agent });
              }}
              style={selectStyle}
            >
              <option value="">unassigned</option>
              {agents.map((d) => (
                <option key={d.slug} value={d.slug}>
                  {d.label}
                </option>
              ))}
            </select>
            {card.assigned_agent && (
              <select
                value={card.model ?? ""}
                onChange={(e) => {
                  void bridge.assignCard({
                    id: card.id,
                    agent: card.assigned_agent,
                    model: e.target.value || null,
                  });
                }}
                style={selectStyle}
              >
                <option value="">default model</option>
                {agents
                  .find((d) => d.slug === card.assigned_agent)
                  ?.available_models.map((m) => (
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
              </select>
            )}
          </div>
        </section>

        <section>
          <SectionTitle>Actions</SectionTitle>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
            <Button
              size="sm"
              variant="primary"
              leftIcon={<Icon.Play size={12} />}
              onClick={() =>
                void bridge.transitionCard({ id: card.id, status: "running" })
              }
              disabled={card.status === "running"}
            >
              Run
            </Button>
            <Button
              size="sm"
              onClick={() =>
                void bridge.transitionCard({
                  id: card.id,
                  status: "succeeded",
                })
              }
            >
              Mark done
            </Button>
            <Button
              size="sm"
              variant="danger"
              onClick={() =>
                void bridge.transitionCard({
                  id: card.id,
                  status: "cancelled",
                })
              }
            >
              Cancel
            </Button>
            <Button size="sm" variant="ghost" leftIcon={<Icon.Terminal size={12} />} onClick={() => void openTerminal()}>
              Open terminal
            </Button>
          </div>
        </section>

        <section>
          <SectionTitle>Live log</SectionTitle>
          <div
            style={{
              background: "var(--bg-0)",
              border: "1px solid var(--line-1)",
              borderRadius: 6,
              padding: 8,
              maxHeight: 240,
              overflowY: "auto",
              fontFamily: "var(--font-mono)",
              fontSize: 11,
              color: "var(--fg-1)",
              userSelect: "text",
            }}
          >
            {tailLines.length === 0 ? (
              <span style={{ color: "var(--fg-3)" }}>—</span>
            ) : (
              tailLines.map((l, i) => (
                <div
                  key={i}
                  style={{
                    color:
                      l.kind === "status"
                        ? "var(--accent)"
                        : l.kind === "discovered"
                          ? "var(--cyan)"
                          : "var(--fg-1)",
                  }}
                >
                  {l.text}
                </div>
              ))
            )}
          </div>
        </section>
      </div>
    </aside>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        fontSize: 10,
        letterSpacing: 0.5,
        color: "var(--fg-3)",
        textTransform: "uppercase",
        marginBottom: 6,
      }}
    >
      {children}
    </div>
  );
}

const selectStyle: React.CSSProperties = {
  background: "var(--bg-2)",
  color: "var(--fg-1)",
  padding: "4px 8px",
  borderRadius: 6,
  border: "1px solid var(--line-1)",
  fontSize: 11,
  minWidth: 120,
};
