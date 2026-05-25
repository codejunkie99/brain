import { useEffect, useRef } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { subscribeChatStream } from "@/ipc/events";
import type { ChatMessage } from "@/ipc/types";
import { useChatStore } from "@/state/chat";
import { useTauriEvent } from "@/hooks/useTauriEvent";

export function MessageList() {
  const thread = useChatStore((s) =>
    s.threads.find((t) => t.id === s.activeThreadId),
  );
  const applyChunk = useChatStore((s) => s.applyChunk);
  const scrollRef = useRef<HTMLDivElement>(null);

  // Subscribe to stream events for the active thread.
  useTauriEvent<import("@/ipc/types").ChatStreamEvent>(
    (fn) => {
      if (!thread) return Promise.resolve(() => {});
      return subscribeChatStream(thread.id, (ev) => fn(ev));
    },
    (ev) => {
      applyChunk(ev.thread_id, ev.assistant_message_id, ev.chunk);
    },
    [thread?.id],
  );

  useEffect(() => {
    const node = scrollRef.current;
    if (node) node.scrollTop = node.scrollHeight;
  }, [thread?.messages.length, thread?.messages[thread.messages.length - 1]?.content]);

  if (!thread) {
    return (
      <div
        style={{
          flex: 1,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: "var(--fg-3)",
          padding: 16,
        }}
      >
        Start a chat to begin.
      </div>
    );
  }

  return (
    <div
      ref={scrollRef}
      style={{
        flex: 1,
        overflowY: "auto",
        padding: "16px 16px 24px",
        userSelect: "text",
      }}
    >
      {thread.messages.length === 0 && (
        <div
          style={{
            color: "var(--fg-3)",
            fontSize: 12,
            textAlign: "center",
            padding: 24,
          }}
        >
          Send a message or paste a spec. Drop bullet points and the
          IDE will turn them into a card graph.
        </div>
      )}
      {thread.messages.map((m) => (
        <MessageBubble key={m.id} m={m} />
      ))}
    </div>
  );
}

function MessageBubble({ m }: { m: ChatMessage }) {
  const isUser = m.role === "user";
  const isSystem = m.role === "system";
  return (
    <div
      style={{
        margin: "10px 0",
        display: "flex",
        flexDirection: "column",
        alignItems: isUser ? "flex-end" : "flex-start",
      }}
    >
      <div
        style={{
          fontSize: 10,
          letterSpacing: 0.4,
          color: "var(--fg-3)",
          marginBottom: 4,
          textTransform: "uppercase",
        }}
      >
        {isUser ? "You" : isSystem ? "System" : m.model ?? "Assistant"}
      </div>
      <div
        style={{
          maxWidth: "90%",
          background: isUser ? "var(--accent-soft)" : "var(--bg-2)",
          color: "var(--fg-0)",
          padding: "8px 12px",
          borderRadius: 10,
          fontSize: 13,
          lineHeight: 1.55,
          whiteSpace: "pre-wrap",
          wordBreak: "break-word",
          border: isUser ? "none" : "1px solid var(--line-0)",
        }}
      >
        {isUser ? (
          m.content
        ) : (
          <ReactMarkdown remarkPlugins={[remarkGfm]}>
            {m.content || "…"}
          </ReactMarkdown>
        )}
      </div>
    </div>
  );
}
