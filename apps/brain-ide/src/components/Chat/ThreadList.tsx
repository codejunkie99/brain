import { useChatStore } from "@/state/chat";
import { Icon } from "../common/Icon";

interface Props {
  onPick: () => void;
}

export function ThreadList({ onPick }: Props) {
  const threads = useChatStore((s) => s.threads);
  const activeId = useChatStore((s) => s.activeThreadId);
  const setActive = useChatStore((s) => s.setActiveThread);
  const create = useChatStore((s) => s.createThread);
  const remove = useChatStore((s) => s.deleteThread);

  return (
    <div
      style={{
        borderBottom: "1px solid var(--line-0)",
        background: "var(--bg-2)",
        maxHeight: 240,
        overflowY: "auto",
      }}
    >
      <button
        onClick={() => {
          void create().then(() => onPick());
        }}
        style={{
          width: "100%",
          padding: "8px 12px",
          display: "flex",
          alignItems: "center",
          gap: 6,
          color: "var(--fg-1)",
          fontSize: 12,
          fontWeight: 500,
          borderBottom: "1px solid var(--line-0)",
        }}
      >
        <Icon.Plus size={14} /> New thread
      </button>
      {threads.map((t) => (
        <div
          key={t.id}
          style={{
            display: "flex",
            alignItems: "center",
            padding: "6px 12px",
            gap: 6,
            background: t.id === activeId ? "var(--bg-3)" : "transparent",
            color: t.id === activeId ? "var(--fg-0)" : "var(--fg-1)",
            fontSize: 12,
            cursor: "pointer",
          }}
          onClick={() => {
            setActive(t.id);
            onPick();
          }}
        >
          <span
            style={{
              flex: 1,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {t.title}
          </span>
          <span style={{ color: "var(--fg-3)", fontSize: 10 }}>
            {t.messages.length}
          </span>
          <button
            onClick={(e) => {
              e.stopPropagation();
              void remove(t.id);
            }}
            title="Delete thread"
            style={{
              color: "var(--fg-3)",
              opacity: 0.6,
            }}
          >
            <Icon.Trash size={12} />
          </button>
        </div>
      ))}
      {threads.length === 0 && (
        <div
          style={{
            padding: "12px 12px 16px",
            color: "var(--fg-3)",
            fontSize: 11,
            textAlign: "center",
          }}
        >
          No threads yet.
        </div>
      )}
    </div>
  );
}
