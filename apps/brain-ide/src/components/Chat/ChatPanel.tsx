import { useState } from "react";

import { useChatStore } from "@/state/chat";

import { Composer } from "./Composer";
import { MessageList } from "./MessageList";
import { ModelPicker } from "./ModelPicker";
import { ThreadList } from "./ThreadList";

export function ChatPanel() {
  const [threadsOpen, setThreadsOpen] = useState(false);
  const activeThread = useChatStore((s) =>
    s.threads.find((t) => t.id === s.activeThreadId),
  );

  return (
    <div
      style={{
        flex: 1,
        display: "flex",
        flexDirection: "column",
        background: "var(--bg-1)",
        minHeight: 0,
      }}
    >
      <header
        className="titlebar-drag"
        style={{
          height: 36,
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          padding: "0 12px",
          borderBottom: "1px solid var(--line-0)",
          flexShrink: 0,
        }}
      >
        <button
          onClick={() => setThreadsOpen((x) => !x)}
          style={{
            color: "var(--fg-1)",
            fontWeight: 600,
            fontSize: 13,
            display: "flex",
            alignItems: "center",
            gap: 6,
            maxWidth: "70%",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          <span aria-hidden style={{ color: "var(--fg-3)" }}>
            ▾
          </span>
          {activeThread?.title ?? "New chat"}
        </button>
        <ModelPicker compact />
      </header>

      {threadsOpen && (
        <ThreadList
          onPick={() => setThreadsOpen(false)}
        />
      )}

      <div
        style={{
          flex: 1,
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
        }}
      >
        <MessageList />
        <Composer />
      </div>
    </div>
  );
}
