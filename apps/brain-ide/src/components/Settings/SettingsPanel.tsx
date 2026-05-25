import { useState } from "react";

import { ApiKeysPanel } from "./ApiKeysPanel";
import { GeneralPanel } from "./GeneralPanel";
import { AgentsPanel } from "./AgentsPanel";
import { ProjectsPanel } from "./ProjectsPanel";

type Tab = "general" | "agents" | "keys" | "projects";

export function SettingsPanel() {
  const [tab, setTab] = useState<Tab>("general");

  return (
    <div style={{ flex: 1, display: "flex", minWidth: 0 }}>
      <aside
        style={{
          width: 200,
          background: "var(--bg-1)",
          borderRight: "1px solid var(--line-0)",
          padding: "12px 0",
          display: "flex",
          flexDirection: "column",
        }}
      >
        <SectionTitle>Settings</SectionTitle>
        {(
          [
            ["general", "General"],
            ["projects", "Projects"],
            ["agents", "Agents"],
            ["keys", "API keys"],
          ] as [Tab, string][]
        ).map(([key, label]) => (
          <button
            key={key}
            onClick={() => setTab(key)}
            style={{
              textAlign: "left",
              padding: "6px 14px",
              fontSize: 12,
              color: tab === key ? "var(--fg-0)" : "var(--fg-2)",
              background: tab === key ? "var(--bg-3)" : "transparent",
              borderLeft: tab === key ? "2px solid var(--accent)" : "2px solid transparent",
            }}
          >
            {label}
          </button>
        ))}
      </aside>
      <section style={{ flex: 1, overflowY: "auto", padding: 24 }}>
        {tab === "general" && <GeneralPanel />}
        {tab === "projects" && <ProjectsPanel />}
        {tab === "agents" && <AgentsPanel />}
        {tab === "keys" && <ApiKeysPanel />}
      </section>
    </div>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        fontSize: 10,
        letterSpacing: 0.6,
        textTransform: "uppercase",
        color: "var(--fg-3)",
        padding: "6px 14px",
        marginBottom: 4,
      }}
    >
      {children}
    </div>
  );
}
