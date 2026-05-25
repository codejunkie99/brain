// Dagre-based auto-layout for the feature graph. Returns absolute
// positions per card id so React Flow can render them.

import dagre from "dagre";

import type { Card, Edge } from "@/ipc/types";

const NODE_W = 240;
const NODE_H = 96;

export function layoutCards(
  cards: Card[],
  edges: Edge[],
): Record<string, { x: number; y: number }> {
  const g = new dagre.graphlib.Graph();
  g.setGraph({ rankdir: "LR", nodesep: 32, ranksep: 64, marginx: 32, marginy: 32 });
  g.setDefaultEdgeLabel(() => ({}));
  for (const card of cards) {
    g.setNode(card.id, { width: NODE_W, height: NODE_H });
  }
  for (const e of edges) {
    if (e.kind === "depends_on") {
      g.setEdge(e.from, e.to);
    }
  }
  dagre.layout(g);
  const out: Record<string, { x: number; y: number }> = {};
  for (const card of cards) {
    const n = g.node(card.id) as { x: number; y: number } | undefined;
    if (n) {
      // Dagre's coords are centers; React Flow expects top-left.
      out[card.id] = { x: n.x - NODE_W / 2, y: n.y - NODE_H / 2 };
    } else {
      out[card.id] = { x: 0, y: 0 };
    }
  }
  return out;
}
