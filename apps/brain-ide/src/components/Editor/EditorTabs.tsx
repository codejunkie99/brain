import { useEditorStore } from "@/state/editor";

export function EditorTabs() {
  const open = useEditorStore((s) => s.open);
  const active = useEditorStore((s) => s.activePath);
  const setActive = useEditorStore((s) => s.setActive);
  const close = useEditorStore((s) => s.closeFile);

  if (open.length === 0) return null;

  return (
    <div
      role="tablist"
      style={{
        display: "flex",
        alignItems: "stretch",
        height: 30,
        background: "var(--bg-1)",
        borderBottom: "1px solid var(--line-0)",
        overflowX: "auto",
        flexShrink: 0,
      }}
    >
      {open.map((f) => {
        const isActive = f.path === active;
        const dirty = f.content !== f.baseline;
        const name = f.path.split("/").pop() ?? f.path;
        return (
          <div
            key={f.path}
            role="tab"
            aria-selected={isActive}
            onClick={() => setActive(f.path)}
            title={f.path}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              padding: "0 10px",
              fontSize: 11,
              cursor: "pointer",
              color: isActive ? "var(--fg-0)" : "var(--fg-2)",
              background: isActive ? "var(--bg-0)" : "transparent",
              borderRight: "1px solid var(--line-0)",
              borderTop: isActive
                ? "1px solid var(--accent)"
                : "1px solid transparent",
              maxWidth: 220,
              whiteSpace: "nowrap",
            }}
          >
            <span
              style={{
                overflow: "hidden",
                textOverflow: "ellipsis",
              }}
            >
              {name}
            </span>
            <button
              onClick={(e) => {
                e.stopPropagation();
                if (
                  dirty &&
                  !window.confirm(`Discard unsaved changes to ${name}?`)
                )
                  return;
                close(f.path);
              }}
              title={dirty ? "Unsaved changes" : "Close"}
              style={{
                color: dirty ? "var(--yellow)" : "var(--fg-3)",
                marginLeft: 2,
                fontSize: 10,
                width: 14,
                textAlign: "center",
              }}
            >
              {dirty ? "●" : "✕"}
            </button>
          </div>
        );
      })}
    </div>
  );
}
