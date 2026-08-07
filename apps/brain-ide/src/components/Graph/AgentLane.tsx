// Right-side floating lane of agent "drop zones". Drag a card onto a
// lane to assign it. Hover styling tells you what'll happen.

import { useMemo, useState } from "react";

import * as bridge from "@/ipc/bridge";
import { useAgentsStore } from "@/state/agents";
import { useGraphStore } from "@/state/graph";

import { Pill } from "../common/Pill";

const DRAG_MIME = "application/x-brain-card";

export function startCardDrag(ev: React.DragEvent, cardId: string) {
  ev.dataTransfer.setData(DRAG_MIME, cardId);
  ev.dataTransfer.effectAllowed = "move";
}

export function AgentLane() {
  const descriptors = useAgentsStore((s) =>
    s.descriptors.filter((d) => d.supports_card_execution),
  );

  return (
    <aside
      style={{
        width: 184,
        background: "var(--bg-1)",
        borderLeft: "1px solid var(--line-0)",
        padding: 10,
        display: "flex",
        flexDirection: "column",
        gap: 8,
        overflowY: "auto",
      }}
    >
      <div
        style={{
          fontSize: 10,
          letterSpacing: 0.5,
          color: "var(--fg-3)",
          textTransform: "uppercase",
          marginBottom: 4,
        }}
      >
        Drop on agent
      </div>
      {descriptors.map((d) => (
        <AgentDropZone key={d.slug} slug={d.slug} label={d.label} model={d.default_model} models={d.available_models} ready={d.ready} readinessMessage={d.readiness_message} />
      ))}
    </aside>
  );
}

interface ZoneProps {
  slug: string;
  label: string;
  model: string | null;
  models: string[];
  ready: boolean;
  readinessMessage: string | null;
}

function AgentDropZone({ slug, label, model, models, ready, readinessMessage }: ZoneProps) {
  const [hover, setHover] = useState(false);
  const [chosenModel, setChosenModel] = useState(model);
  const select = useGraphStore.getState().selectCard;
  const assignedCount = useGraphStore((s) =>
    Object.values(s.cards).filter((c) => c.assigned_agent === slug).length,
  );

  const onDrop = useMemo(
    () => async (e: React.DragEvent) => {
      e.preventDefault();
      setHover(false);
      const id = e.dataTransfer.getData(DRAG_MIME);
      if (!id) return;
      const updated = await bridge.assignCard({
        id,
        agent: slug,
        model: chosenModel ?? undefined,
      });
      // Optimistic: update graph state immediately so the card pill
      // re-renders with the new agent without waiting for the event.
      useGraphStore.setState((s) => ({
        cards: { ...s.cards, [updated.id]: updated },
      }));
      select(updated.id);
    },
    [slug, chosenModel, select],
  );

  return (
    <div
      onDragOver={(e) => {
        if (e.dataTransfer.types.includes(DRAG_MIME)) {
          e.preventDefault();
          e.dataTransfer.dropEffect = "move";
          setHover(true);
        }
      }}
      onDragLeave={() => setHover(false)}
      onDrop={(e) => void onDrop(e)}
      style={{
        padding: 10,
        borderRadius: 10,
        background: hover ? "var(--accent-soft)" : "var(--bg-2)",
        border: hover
          ? "1px dashed var(--accent)"
          : "1px solid var(--line-1)",
        transition: "background 120ms, border-color 120ms",
        opacity: ready ? 1 : 0.6,
      }}
      title={readinessMessage ?? undefined}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <span
          style={{
            width: 8,
            height: 8,
            borderRadius: 4,
            background: ready ? "var(--green)" : "var(--orange)",
          }}
        />
        <span style={{ fontSize: 12, color: "var(--fg-0)", fontWeight: 600 }}>
          {label}
        </span>
        <span style={{ flex: 1 }} />
        <Pill color="var(--fg-3)" tone="ghost" size="xs">
          {assignedCount}
        </Pill>
      </div>
      {models.length > 0 && (
        <select
          value={chosenModel ?? ""}
          onChange={(e) => setChosenModel(e.target.value || null)}
          style={{
            marginTop: 6,
            width: "100%",
            background: "var(--bg-3)",
            color: "var(--fg-1)",
            padding: "3px 6px",
            borderRadius: 5,
            border: "1px solid var(--line-1)",
            fontSize: 10,
          }}
        >
          {models.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
      )}
      {!ready && readinessMessage && (
        <div
          style={{
            marginTop: 6,
            fontSize: 10,
            color: "var(--fg-3)",
            lineHeight: 1.35,
          }}
        >
          {readinessMessage}
        </div>
      )}
    </div>
  );
}
