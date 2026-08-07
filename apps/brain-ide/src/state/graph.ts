// Feature graph store. Mirrors the backend's `FeatureGraph` snapshot
// and applies in-process patches as `SchedulerEvent`s arrive so the
// UI never blocks on a roundtrip.

import { create } from "zustand";

import * as bridge from "@/ipc/bridge";
import type {
  Card,
  CardOutputChunk,
  Edge,
  SchedulerEvent,
  Uuid,
} from "@/ipc/types";

interface CardLog {
  status: string;
  logs: { kind: "log" | "status" | "discovered"; text: string; at: number }[];
}

interface GraphState {
  cards: Record<Uuid, Card>;
  edges: Edge[];
  selectedCardId: Uuid | null;
  cardLogs: Record<Uuid, CardLog>;
  load: () => Promise<void>;
  applyEvent: (ev: SchedulerEvent) => void;
  applyOutput: (cardId: Uuid, chunk: CardOutputChunk) => void;
  selectCard: (id: Uuid | null) => void;
  reset: () => Promise<void>;
}

export const useGraphStore = create<GraphState>((set) => ({
  cards: {},
  edges: [],
  selectedCardId: null,
  cardLogs: {},

  load: async () => {
    const snap = await bridge.graphSnapshot();
    const cards = Object.fromEntries(snap.cards.map((c) => [c.id, c]));
    set({ cards, edges: snap.edges });
  },

  applyEvent: (ev) => {
    if (ev.type === "card_added" || ev.type === "card_updated") {
      set((s) => ({ cards: { ...s.cards, [ev.card.id]: ev.card } }));
    } else if (ev.type === "card_removed") {
      set((s) => {
        const cards = { ...s.cards };
        delete cards[ev.id];
        return {
          cards,
          edges: s.edges.filter(
            (e) => e.from !== ev.id && e.to !== ev.id,
          ),
          selectedCardId:
            s.selectedCardId === ev.id ? null : s.selectedCardId,
        };
      });
    } else if (ev.type === "graph_rescanned") {
      // A bulk mutation happened (apply_breakdown, reset). Pull
      // a fresh snapshot; cheap enough for hundreds of cards.
      void useGraphStore.getState().load();
    }
    // dispatch_card and card_cancel_requested are informational here;
    // the actual status transition rides on card_updated.
  },

  applyOutput: (cardId, chunk) => {
    set((s) => {
      const entry = s.cardLogs[cardId] ?? { status: "", logs: [] };
      const next: CardLog = { ...entry };
      const now = Date.now();
      if (chunk.type === "status") {
        next.status = chunk.text;
        next.logs = [
          ...next.logs,
          { kind: "status", text: chunk.text, at: now },
        ];
      } else if (chunk.type === "log") {
        next.logs = [
          ...next.logs.slice(-499),
          { kind: "log", text: chunk.text, at: now },
        ];
      } else if (chunk.type === "discovered") {
        next.logs = [
          ...next.logs,
          {
            kind: "discovered",
            text: `${chunk.title} — ${chunk.summary}`,
            at: now,
          },
        ];
      }
      // bytes chunks go to the xterm via the pty event channel, not here.
      return { cardLogs: { ...s.cardLogs, [cardId]: next } };
    });
  },

  selectCard: (id) => set({ selectedCardId: id }),

  reset: async () => {
    await bridge.resetGraph();
    set({ cards: {}, edges: [], selectedCardId: null, cardLogs: {} });
  },
}));
