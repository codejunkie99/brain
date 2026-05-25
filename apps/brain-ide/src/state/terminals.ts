// PTY session store. The terminal pane reads sessions from here and
// each xterm.js instance subscribes to its session's data channel.

import { create } from "zustand";

import * as bridge from "@/ipc/bridge";
import type { PtySessionInfo, Uuid } from "@/ipc/types";

interface TerminalsState {
  sessions: PtySessionInfo[];
  activeSessionId: Uuid | null;
  load: () => Promise<void>;
  spawn: (input?: bridge.PtySpawnInput) => Promise<PtySessionInfo>;
  close: (id: Uuid) => Promise<void>;
  setActive: (id: Uuid | null) => void;
  addLocal: (session: PtySessionInfo) => void;
  removeLocal: (id: Uuid) => void;
}

export const useTerminalsStore = create<TerminalsState>((set, get) => ({
  sessions: [],
  activeSessionId: null,
  load: async () => {
    const sessions = await bridge.ptyList();
    set({
      sessions,
      activeSessionId:
        get().activeSessionId ?? sessions[0]?.id ?? null,
    });
  },
  spawn: async (input = {}) => {
    const session = await bridge.ptySpawn(input);
    set((s) => ({
      sessions: [...s.sessions, session],
      activeSessionId: session.id,
    }));
    return session;
  },
  close: async (id) => {
    await bridge.ptyClose(id);
    set((s) => ({
      sessions: s.sessions.filter((x) => x.id !== id),
      activeSessionId:
        s.activeSessionId === id
          ? (s.sessions.find((x) => x.id !== id)?.id ?? null)
          : s.activeSessionId,
    }));
  },
  setActive: (id) => set({ activeSessionId: id }),
  addLocal: (session) =>
    set((s) => ({
      sessions: s.sessions.some((x) => x.id === session.id)
        ? s.sessions
        : [...s.sessions, session],
    })),
  removeLocal: (id) =>
    set((s) => ({
      sessions: s.sessions.filter((x) => x.id !== id),
    })),
}));
