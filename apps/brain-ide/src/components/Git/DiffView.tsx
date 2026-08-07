// A unified diff view. Renders hunks with line numbers + +/- markers
// and color-codes additions/deletions. Simple plain HTML so we don't
// need a second Monaco instance just for diffs.

import type { FileDiff } from "@/ipc/types";

interface Props {
  diff: FileDiff;
}

export function DiffView({ diff }: Props) {
  if (diff.binary) {
    return (
      <div
        style={{
          padding: 24,
          color: "var(--fg-3)",
          fontSize: 12,
        }}
      >
        Binary file — diff hidden.
      </div>
    );
  }
  if (diff.hunks.length === 0) {
    return (
      <div
        style={{
          padding: 24,
          color: "var(--fg-3)",
          fontSize: 12,
        }}
      >
        No changes for this view (try the other side of the stage).
      </div>
    );
  }

  return (
    <div
      style={{
        flex: 1,
        overflow: "auto",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        background: "var(--bg-0)",
        userSelect: "text",
      }}
    >
      <header
        style={{
          position: "sticky",
          top: 0,
          background: "var(--bg-1)",
          borderBottom: "1px solid var(--line-0)",
          padding: "6px 12px",
          color: "var(--fg-1)",
          fontSize: 12,
          fontFamily: "var(--font-ui)",
        }}
      >
        {diff.path}
      </header>
      {diff.hunks.map((h, i) => (
        <div key={i}>
          <div
            style={{
              padding: "4px 12px",
              color: "var(--fg-3)",
              background: "var(--bg-1)",
              borderTop: "1px solid var(--line-0)",
              borderBottom: "1px solid var(--line-0)",
              fontSize: 11,
            }}
          >
            {h.header.trim() ||
              `@@ -${h.old_start},${h.old_lines} +${h.new_start},${h.new_lines} @@`}
          </div>
          {h.lines.map((l, j) => {
            const bg =
              l.kind === "+"
                ? "rgba(60, 210, 139, 0.10)"
                : l.kind === "-"
                  ? "rgba(239, 94, 110, 0.10)"
                  : "transparent";
            const color =
              l.kind === "+"
                ? "var(--green)"
                : l.kind === "-"
                  ? "var(--red)"
                  : "var(--fg-1)";
            return (
              <div
                key={j}
                style={{
                  display: "grid",
                  gridTemplateColumns: "48px 48px 1ch 1fr",
                  alignItems: "stretch",
                  background: bg,
                  lineHeight: 1.5,
                }}
              >
                <span
                  style={{
                    textAlign: "right",
                    padding: "0 6px",
                    color: "var(--fg-3)",
                  }}
                >
                  {l.old_lineno ?? ""}
                </span>
                <span
                  style={{
                    textAlign: "right",
                    padding: "0 6px",
                    color: "var(--fg-3)",
                  }}
                >
                  {l.new_lineno ?? ""}
                </span>
                <span style={{ color, textAlign: "center" }}>{l.kind}</span>
                <pre
                  style={{
                    margin: 0,
                    padding: "0 6px",
                    color,
                    whiteSpace: "pre",
                    fontFamily: "inherit",
                    fontSize: "inherit",
                  }}
                >
                  {l.content}
                </pre>
              </div>
            );
          })}
        </div>
      ))}
    </div>
  );
}
