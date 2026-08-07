import { useGraphStore } from "@/state/graph";
import { useProjectsStore } from "@/state/projects";
import { useChatStore } from "@/state/chat";
import { Pill } from "../common/Pill";
import { statusColor } from "@/theme/tokens";

export function StatusBar() {
  const cards = useGraphStore((s) => Object.values(s.cards));
  const project = useProjectsStore((s) =>
    s.projects.find((p) => p.id === s.activeId),
  );
  const streaming = useChatStore((s) => s.streamingMessageId !== null);

  const counts = {
    pending: cards.filter((c) => c.status === "pending").length,
    ready: cards.filter((c) => c.status === "ready").length,
    running: cards.filter((c) => c.status === "running").length,
    blocked: cards.filter((c) => c.status === "blocked").length,
    succeeded: cards.filter((c) => c.status === "succeeded").length,
    failed: cards.filter((c) => c.status === "failed").length,
  };

  return (
    <footer
      style={{
        height: 24,
        background: "var(--bg-1)",
        borderTop: "1px solid var(--line-0)",
        display: "flex",
        alignItems: "center",
        gap: 16,
        padding: "0 12px",
        color: "var(--fg-2)",
        fontSize: 11,
        letterSpacing: 0.2,
        flexShrink: 0,
      }}
    >
      <span title={project?.root}>
        <span style={{ color: "var(--fg-3)" }}>project </span>
        {project?.name ?? "—"}
      </span>
      <span style={{ color: "var(--line-1)" }}>·</span>
      <span title={project?.brain_dir}>
        <span style={{ color: "var(--fg-3)" }}>brain </span>
        {project?.brain_dir.replace(/^.*\//, "") ?? "—"}
      </span>
      <span style={{ flex: 1 }} />
      {streaming && (
        <Pill color="var(--accent)" tone="soft" size="xs">
          streaming
        </Pill>
      )}
      <span style={{ display: "flex", gap: 6 }}>
        {(["running", "ready", "blocked", "failed", "succeeded"] as const).map(
          (k) =>
            counts[k] > 0 && (
              <Pill key={k} color={statusColor[k]} tone="soft" size="xs">
                {counts[k]} {k}
              </Pill>
            ),
        )}
      </span>
    </footer>
  );
}
