// React Flow node component for a single feature card.

import { Handle, NodeProps, Position } from "reactflow";

import type { Card } from "@/ipc/types";
import { useGraphStore } from "@/state/graph";
import { kindAccent, statusColor } from "@/theme/tokens";

import { Pill } from "../common/Pill";
import { startCardDrag } from "./AgentLane";

export interface FeatureCardData {
  card: Card;
}

export function FeatureCardNode({ data, selected }: NodeProps<FeatureCardData>) {
  const card = data.card;
  const select = useGraphStore((s) => s.selectCard);
  const liveStatus = useGraphStore((s) => s.cardLogs[card.id]?.status);

  return (
    <div
      draggable
      onDragStart={(e) => startCardDrag(e, card.id)}
      onClick={() => select(card.id)}
      style={{
        width: 240,
        height: 96,
        background: "var(--bg-2)",
        border: `1px solid ${selected ? "var(--accent)" : "var(--line-1)"}`,
        borderLeft: `3px solid ${kindAccent[card.kind]}`,
        borderRadius: 10,
        boxShadow: selected
          ? "0 8px 28px rgba(109,93,252,0.20)"
          : "0 1px 0 rgba(0,0,0,0.4)",
        padding: 10,
        display: "flex",
        flexDirection: "column",
        gap: 6,
        cursor: "pointer",
        transition: "border-color 120ms, box-shadow 120ms",
      }}
    >
      <Handle
        type="target"
        position={Position.Left}
        style={{ background: "var(--line-2)", width: 8, height: 8 }}
      />
      <Handle
        type="source"
        position={Position.Right}
        style={{ background: "var(--line-2)", width: 8, height: 8 }}
      />
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "flex-start",
        }}
      >
        <div
          style={{
            fontSize: 12,
            fontWeight: 600,
            color: "var(--fg-0)",
            lineHeight: 1.3,
            display: "-webkit-box",
            WebkitLineClamp: 2,
            WebkitBoxOrient: "vertical",
            overflow: "hidden",
          }}
        >
          {card.title}
        </div>
        <Pill color={statusColor[card.status]} tone="soft" size="xs">
          {card.status}
        </Pill>
      </div>
      <div
        style={{
          fontSize: 11,
          color: "var(--fg-2)",
          display: "-webkit-box",
          WebkitLineClamp: 2,
          WebkitBoxOrient: "vertical",
          overflow: "hidden",
          minHeight: 26,
        }}
      >
        {liveStatus || card.summary}
      </div>
      <div style={{ display: "flex", gap: 4 }}>
        <Pill color={kindAccent[card.kind]} tone="ghost" size="xs">
          {card.kind.replace("_", " ")}
        </Pill>
        {card.assigned_agent ? (
          <Pill color="var(--accent)" tone="solid" size="xs">
            {card.assigned_agent}
          </Pill>
        ) : (
          <Pill color="var(--fg-3)" tone="ghost" size="xs">
            unassigned
          </Pill>
        )}
      </div>
    </div>
  );
}
