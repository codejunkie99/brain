import { useEffect, useRef, useState } from "react";

import * as bridge from "@/ipc/bridge";
import { useChatStore } from "@/state/chat";
import { useGraphStore } from "@/state/graph";

import { Button } from "../common/Button";
import { Icon } from "../common/Icon";

export function Composer() {
  const [text, setText] = useState("");
  const ta = useRef<HTMLTextAreaElement>(null);
  const send = useChatStore((s) => s.sendMessage);
  const cancel = useChatStore((s) => s.cancelCurrent);
  const streaming = useChatStore((s) => s.streamingMessageId !== null);
  const agent = useChatStore((s) => s.activeAgent);
  const model = useChatStore((s) => s.activeModel);
  const [planning, setPlanning] = useState(false);

  useEffect(() => {
    autoresize();
  }, [text]);

  function autoresize() {
    const el = ta.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = Math.min(el.scrollHeight, 220) + "px";
  }

  async function handleSend() {
    const value = text.trim();
    if (!value) return;
    setText("");
    await send(value);
  }

  async function handlePlan() {
    const value = text.trim();
    if (!value) return;
    setPlanning(true);
    try {
      const breakdown = await bridge.parseSpec({
        spec: value,
        backend: agent,
        model: model ?? undefined,
      });
      const ids = await bridge.applyBreakdown({
        breakdown,
        default_agent: agent,
        default_model: model ?? undefined,
      });
      await useGraphStore.getState().load();
      setText("");
      if (ids.length > 0) {
        useGraphStore.getState().selectCard(ids[0]);
      }
    } finally {
      setPlanning(false);
    }
  }

  return (
    <div
      style={{
        padding: 12,
        borderTop: "1px solid var(--line-0)",
        background: "var(--bg-1)",
      }}
    >
      <textarea
        ref={ta}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            void handleSend();
          }
        }}
        placeholder="Message the model, or paste a spec and click Plan."
        rows={2}
        style={{
          width: "100%",
          background: "var(--bg-2)",
          color: "var(--fg-0)",
          fontSize: 13,
          padding: 10,
          borderRadius: 8,
          border: "1px solid var(--line-1)",
          resize: "none",
          fontFamily: "var(--font-ui)",
          lineHeight: 1.5,
          minHeight: 60,
          maxHeight: 220,
          overflowY: "auto",
        }}
      />
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginTop: 8,
          gap: 8,
        }}
      >
        <span style={{ color: "var(--fg-3)", fontSize: 11 }}>
          ⌘↩ to send · Plan turns this into a card graph
        </span>
        <div style={{ display: "flex", gap: 6 }}>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void handlePlan()}
            disabled={planning || streaming || !text.trim()}
          >
            {planning ? "Planning…" : "Plan ▾"}
          </Button>
          {streaming ? (
            <Button
              variant="danger"
              size="sm"
              leftIcon={<Icon.Stop size={12} />}
              onClick={() => void cancel()}
            >
              Stop
            </Button>
          ) : (
            <Button
              variant="primary"
              size="sm"
              leftIcon={<Icon.Send size={12} />}
              onClick={() => void handleSend()}
              disabled={!text.trim()}
            >
              Send
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
