import { useEffect, useMemo } from "react";

import { useAgentsStore } from "@/state/agents";
import { useChatStore } from "@/state/chat";
import { Pill } from "../common/Pill";

interface Props {
  compact?: boolean;
}

/**
 * Single select that picks (agent, model). The chat transcript persists
 * across changes — flipping from Claude Opus to Codex GPT-5 mid-thread
 * just changes who answers the *next* turn.
 */
export function ModelPicker({ compact }: Props) {
  const descriptors = useAgentsStore((s) =>
    s.descriptors.filter((d) => d.supports_chat),
  );
  const activeAgent = useChatStore((s) => s.activeAgent);
  const activeModel = useChatStore((s) => s.activeModel);
  const setActive = useChatStore((s) => s.setActiveAgent);

  // Auto-pick first ready chat agent on first load if the default isn't ready.
  useEffect(() => {
    const current = descriptors.find((d) => d.slug === activeAgent);
    if (!current || !current.ready) {
      const fallback = descriptors.find((d) => d.ready);
      if (fallback) {
        setActive(fallback.slug, fallback.default_model);
      }
    } else if (!activeModel && current.default_model) {
      setActive(current.slug, current.default_model);
    }
  }, [descriptors, activeAgent, activeModel, setActive]);

  const value = useMemo(() => `${activeAgent}::${activeModel ?? ""}`, [
    activeAgent,
    activeModel,
  ]);

  const options = descriptors.flatMap((d) => {
    const models = d.available_models.length ? d.available_models : [""];
    return models.map((m) => ({
      value: `${d.slug}::${m}`,
      label: m ? `${d.label} · ${m}` : d.label,
      agent: d.slug,
      model: m || null,
      ready: d.ready,
    }));
  });

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
      {!compact && <span style={{ fontSize: 11, color: "var(--fg-3)" }}>Model</span>}
      <select
        value={value}
        onChange={(e) => {
          const opt = options.find((o) => o.value === e.target.value);
          if (opt) setActive(opt.agent, opt.model);
        }}
        style={{
          background: "var(--bg-2)",
          color: "var(--fg-1)",
          padding: "4px 8px",
          borderRadius: 6,
          border: "1px solid var(--line-1)",
          fontSize: 11,
          maxWidth: 220,
        }}
      >
        {options.map((opt) => (
          <option key={opt.value} value={opt.value} disabled={!opt.ready}>
            {opt.label}
            {!opt.ready ? " (not configured)" : ""}
          </option>
        ))}
      </select>
      {descriptors.find((d) => d.slug === activeAgent)?.ready === false && (
        <Pill color="var(--red)" tone="soft" size="xs">
          configure key
        </Pill>
      )}
    </div>
  );
}
