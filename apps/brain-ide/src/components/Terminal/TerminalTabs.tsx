// Tabbed bottom panel. Each tab is a PTY session; clicking a card on
// the graph opens (or focuses) a terminal tagged with its card_id.

import { useGraphStore } from "@/state/graph";
import { useTerminalsStore } from "@/state/terminals";

import { Button } from "../common/Button";
import { Icon } from "../common/Icon";
import { Pill } from "../common/Pill";
import { XTerm } from "./XTerm";

export function TerminalTabs() {
  const sessions = useTerminalsStore((s) => s.sessions);
  const active = useTerminalsStore((s) => s.activeSessionId);
  const setActive = useTerminalsStore((s) => s.setActive);
  const close = useTerminalsStore((s) => s.close);
  const spawn = useTerminalsStore((s) => s.spawn);
  const removeLocal = useTerminalsStore((s) => s.removeLocal);
  const cards = useGraphStore((s) => s.cards);

  const activeSession = sessions.find((s) => s.id === active);

  return (
    <div
      style={{
        flex: 1,
        display: "flex",
        flexDirection: "column",
        minHeight: 0,
        minWidth: 0,
        background: "var(--bg-1)",
      }}
    >
      <header
        style={{
          height: 32,
          display: "flex",
          alignItems: "stretch",
          gap: 1,
          borderBottom: "1px solid var(--line-0)",
          background: "var(--bg-1)",
          flexShrink: 0,
          overflowX: "auto",
        }}
      >
        {sessions.map((s) => {
          const card = s.card_id ? cards[s.card_id] : null;
          const isActive = s.id === active;
          return (
            <div
              key={s.id}
              onClick={() => setActive(s.id)}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                padding: "0 10px",
                background: isActive ? "var(--bg-0)" : "transparent",
                borderRight: "1px solid var(--line-0)",
                color: isActive ? "var(--fg-0)" : "var(--fg-2)",
                fontSize: 11,
                cursor: "pointer",
                whiteSpace: "nowrap",
              }}
            >
              {card && (
                <Pill color="var(--accent)" tone="soft" size="xs">
                  {card.title.slice(0, 24)}
                </Pill>
              )}
              <span>{s.label || s.program}</span>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  void close(s.id);
                }}
                style={{ color: "var(--fg-3)", padding: "0 2px" }}
                title="Close"
              >
                ✕
              </button>
            </div>
          );
        })}
        <div style={{ flex: 1 }} />
        <button
          onClick={() => void spawn({})}
          style={{
            padding: "0 12px",
            color: "var(--fg-1)",
            display: "flex",
            alignItems: "center",
            gap: 4,
            fontSize: 11,
          }}
        >
          <Icon.Plus size={12} /> shell
        </button>
      </header>
      <div
        style={{
          flex: 1,
          minHeight: 0,
          minWidth: 0,
          padding: 6,
          display: "flex",
        }}
      >
        {activeSession ? (
          <XTerm
            session={activeSession}
            onExit={() => removeLocal(activeSession.id)}
          />
        ) : (
          <div
            style={{
              flex: 1,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              color: "var(--fg-3)",
              fontSize: 12,
            }}
          >
            <Button
              size="sm"
              variant="primary"
              leftIcon={<Icon.Terminal size={12} />}
              onClick={() => void spawn({})}
            >
              Open a shell
            </Button>
          </div>
        )}
      </div>
    </div>
  );
}
