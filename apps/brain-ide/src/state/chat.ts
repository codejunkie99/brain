// Chat thread store. The active thread persists across model switches —
// the user can flip the model picker mid-conversation and the next
// turn goes to the new agent without losing transcript context.

import { create } from "zustand";

import * as bridge from "@/ipc/bridge";
import type { ChatChunk, ChatMessage, ChatThread, Uuid } from "@/ipc/types";

interface ChatState {
  threads: ChatThread[];
  activeThreadId: Uuid | null;
  activeAgent: string;
  activeModel: string | null;
  streamingMessageId: Uuid | null;
  thinkingByMessage: Record<Uuid, string>;
  load: () => Promise<void>;
  setActiveThread: (id: Uuid | null) => void;
  setActiveAgent: (agent: string, model: string | null) => void;
  createThread: (title?: string) => Promise<ChatThread>;
  deleteThread: (id: Uuid) => Promise<void>;
  renameThread: (id: Uuid, title: string) => Promise<void>;
  sendMessage: (text: string) => Promise<void>;
  cancelCurrent: () => Promise<void>;
  /**
   * Called by the chat stream subscription whenever the backend emits
   * a new chunk for a thread. Patches the assistant message in place.
   */
  applyChunk: (
    threadId: Uuid,
    messageId: Uuid,
    chunk: ChatChunk,
  ) => void;
  /** Refresh a thread from the backend (after a streaming run finalizes). */
  refreshThread: (id: Uuid) => Promise<void>;
}

export const useChatStore = create<ChatState>((set, get) => ({
  threads: [],
  activeThreadId: null,
  activeAgent: "claude",
  activeModel: null,
  streamingMessageId: null,
  thinkingByMessage: {},

  load: async () => {
    const threads = await bridge.listThreads();
    set({
      threads,
      activeThreadId: get().activeThreadId ?? threads[0]?.id ?? null,
    });
  },

  setActiveThread: (id) => set({ activeThreadId: id }),

  setActiveAgent: (agent, model) =>
    set({ activeAgent: agent, activeModel: model }),

  createThread: async (title) => {
    const thread = await bridge.createThread(title ?? "New chat");
    set((s) => ({
      threads: [thread, ...s.threads],
      activeThreadId: thread.id,
    }));
    return thread;
  },

  deleteThread: async (id) => {
    await bridge.deleteThread(id);
    set((s) => ({
      threads: s.threads.filter((t) => t.id !== id),
      activeThreadId:
        s.activeThreadId === id
          ? (s.threads.find((t) => t.id !== id)?.id ?? null)
          : s.activeThreadId,
    }));
  },

  renameThread: async (id, title) => {
    await bridge.renameThread(id, title);
    set((s) => ({
      threads: s.threads.map((t) =>
        t.id === id ? { ...t, title } : t,
      ),
    }));
  },

  sendMessage: async (text) => {
    const { activeThreadId, activeAgent, activeModel } = get();
    let threadId = activeThreadId;
    if (!threadId) {
      const t = await get().createThread();
      threadId = t.id;
    }
    // Optimistic: append a user message + placeholder assistant.
    const optimisticUser: ChatMessage = {
      id: `pending-user-${Date.now()}`,
      role: "user",
      content: text,
      model: null,
      meta: null,
      created_at: new Date().toISOString(),
    };
    set((s) => ({
      threads: s.threads.map((t) =>
        t.id === threadId
          ? { ...t, messages: [...t.messages, optimisticUser] }
          : t,
      ),
    }));

    const res = await bridge.sendMessage({
      thread_id: threadId,
      agent: activeAgent,
      model: activeModel ?? undefined,
      text,
    });
    set({ streamingMessageId: res.assistant_message_id });
    // Refresh from backend so the optimistic user id is replaced with
    // the real one. The streamed deltas will keep arriving on the
    // chat event channel.
    await get().refreshThread(threadId);
  },

  cancelCurrent: async () => {
    const id = get().streamingMessageId;
    if (id) {
      await bridge.cancelStream(id);
      set({ streamingMessageId: null });
    }
  },

  applyChunk: (threadId, messageId, chunk) => {
    set((s) => {
      if (chunk.type === "delta") {
        return {
          threads: s.threads.map((t) =>
            t.id === threadId
              ? {
                  ...t,
                  messages: t.messages.map((m) =>
                    m.id === messageId
                      ? { ...m, content: m.content + chunk.text }
                      : m,
                  ),
                }
              : t,
          ),
        };
      }
      if (chunk.type === "thinking") {
        return {
          thinkingByMessage: {
            ...s.thinkingByMessage,
            [messageId]:
              (s.thinkingByMessage[messageId] ?? "") + chunk.text,
          },
        };
      }
      if (chunk.type === "done" || chunk.type === "error") {
        return {
          streamingMessageId:
            s.streamingMessageId === messageId
              ? null
              : s.streamingMessageId,
        };
      }
      return s;
    });
  },

  refreshThread: async (id) => {
    const threads = await bridge.listThreads();
    set((s) => ({
      threads: threads.map((t) =>
        t.id === id
          ? t
          : (s.threads.find((x) => x.id === t.id) ?? t),
      ),
    }));
  },
}));
