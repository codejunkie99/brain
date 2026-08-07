// Top-level graph view. Combines the React Flow canvas (cards + edges)
// with the agent drop lanes and the card inspector. The toolbar at the
// top lets the user add an empty card or reset the graph.

import { useEffect, useMemo } from "react";
import ReactFlow, {
  Background,
  BackgroundVariant,
  Connection,
  Controls,
  Edge as RfEdge,
  Node as RfNode,
  ReactFlowProvider,
} from "reactflow";

import * as bridge from "@/ipc/bridge";
import { useGraphStore } from "@/state/graph";

import { Button } from "../common/Button";
import { Icon } from "../common/Icon";
import { AgentLane } from "./AgentLane";
import { CardInspector } from "./CardInspector";
import { FeatureCardData, FeatureCardNode } from "./FeatureCard";
import { layoutCards } from "./cardLayout";

const nodeTypes = { card: FeatureCardNode };

export function GraphView() {
  return (
    <ReactFlowProvider>
      <GraphInner />
    </ReactFlowProvider>
  );
}

function GraphInner() {
  const cards = useGraphStore((s) => Object.values(s.cards));
  const edges = useGraphStore((s) => s.edges);
  const selectCard = useGraphStore((s) => s.selectCard);

  const nodes = useMemo<RfNode<FeatureCardData>[]>(() => {
    const positions = layoutCards(cards, edges);
    return cards.map((card) => ({
      id: card.id,
      type: "card",
      position: positions[card.id] ?? { x: 0, y: 0 },
      data: { card },
      draggable: true,
      // Allow native drag-and-drop on top of React Flow's dragging by
      // marking each node draggable=false at the DOM level and using
      // our own dragstart wrapper. React Flow handles pan/zoom.
    }));
  }, [cards, edges]);

  const rfEdges = useMemo<RfEdge[]>(
    () =>
      edges.map((e) => ({
        id: `${e.from}->${e.to}`,
        source: e.from,
        target: e.to,
        type: "smoothstep",
        className: e.kind,
        animated:
          useGraphStore.getState().cards[e.from]?.status === "running",
      })),
    [edges],
  );

  useEffect(() => {
    void useGraphStore.getState().load();
  }, []);

  async function handleAddCard() {
    const card = await bridge.addCard({
      title: "New card",
      summary: "",
      kind: "feature",
    });
    selectCard(card.id);
  }

  async function handleConnect(c: Connection) {
    if (!c.source || !c.target) return;
    await bridge.addEdge({
      from: c.source,
      to: c.target,
      kind: "depends_on",
    });
  }

  return (
    <div style={{ flex: 1, display: "flex", minWidth: 0, minHeight: 0 }}>
      <div style={{ flex: 1, position: "relative", minWidth: 0 }}>
        <Toolbar onAddCard={() => void handleAddCard()} />
        <ReactFlow
          nodes={nodes}
          edges={rfEdges}
          nodeTypes={nodeTypes}
          fitView
          fitViewOptions={{ padding: 0.2 }}
          minZoom={0.3}
          maxZoom={1.6}
          proOptions={{ hideAttribution: true }}
          onConnect={(c) => void handleConnect(c)}
          onNodeClick={(_, n) => selectCard(n.id)}
          onPaneClick={() => selectCard(null)}
        >
          <Background
            color="var(--line-1)"
            variant={BackgroundVariant.Dots}
            gap={20}
            size={1}
          />
          <Controls
            position="bottom-right"
            style={{ background: "var(--bg-2)", border: "1px solid var(--line-1)" }}
          />
        </ReactFlow>
      </div>
      <AgentLane />
      <CardInspector />
    </div>
  );
}

function Toolbar({ onAddCard }: { onAddCard: () => void }) {
  return (
    <div
      style={{
        position: "absolute",
        top: 12,
        left: 12,
        zIndex: 5,
        display: "flex",
        gap: 6,
      }}
    >
      <Button size="sm" leftIcon={<Icon.Plus size={12} />} onClick={onAddCard}>
        Add card
      </Button>
      <Button
        size="sm"
        variant="ghost"
        leftIcon={<Icon.Reset size={12} />}
        onClick={() => void useGraphStore.getState().reset()}
        title="Clear all cards"
      >
        Reset
      </Button>
    </div>
  );
}

