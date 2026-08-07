import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useState } from "react";

import { useProjectsStore } from "@/state/projects";

import { Button } from "../common/Button";
import { Icon } from "../common/Icon";
import { Pill } from "../common/Pill";

export function ProjectsPanel() {
  const projects = useProjectsStore((s) => s.projects);
  const activeId = useProjectsStore((s) => s.activeId);
  const add = useProjectsStore((s) => s.add);
  const remove = useProjectsStore((s) => s.remove);
  const setActive = useProjectsStore((s) => s.setActive);
  const [adding, setAdding] = useState(false);

  async function pickAndAdd() {
    setAdding(true);
    try {
      const root = await openDialog({
        directory: true,
        multiple: false,
        title: "Pick a project root",
      });
      if (typeof root !== "string") return;
      const name = root.split("/").filter(Boolean).pop() ?? "project";
      await add({ name, root });
    } finally {
      setAdding(false);
    }
  }

  return (
    <div style={{ maxWidth: 720 }}>
      <div style={{ display: "flex", alignItems: "center", marginBottom: 16 }}>
        <h2 style={{ fontSize: 16, margin: 0, flex: 1 }}>Projects</h2>
        <Button
          variant="primary"
          size="sm"
          leftIcon={<Icon.Plus size={12} />}
          onClick={() => void pickAndAdd()}
          disabled={adding}
        >
          {adding ? "…" : "Add project"}
        </Button>
      </div>
      {projects.length === 0 && (
        <div style={{ color: "var(--fg-3)", fontSize: 12 }}>
          No projects yet. Add one to point the IDE at a repo and its
          brain memory dir.
        </div>
      )}
      {projects.map((p) => (
        <div
          key={p.id}
          style={{
            padding: 12,
            border: "1px solid var(--line-1)",
            borderRadius: 10,
            background: "var(--bg-1)",
            marginBottom: 8,
            display: "flex",
            alignItems: "center",
            gap: 8,
          }}
        >
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <strong style={{ fontSize: 13 }}>{p.name}</strong>
              {p.id === activeId && (
                <Pill color="var(--accent)" tone="solid" size="xs">
                  active
                </Pill>
              )}
            </div>
            <div style={{ fontSize: 11, color: "var(--fg-3)", fontFamily: "var(--font-mono)" }}>
              {p.root}
            </div>
            <div style={{ fontSize: 11, color: "var(--fg-3)" }}>
              brain: {p.brain_dir}
            </div>
          </div>
          {p.id !== activeId && (
            <Button size="sm" onClick={() => void setActive(p.id)}>
              Open
            </Button>
          )}
          <Button
            size="sm"
            variant="danger"
            onClick={() => void remove(p.id)}
            title="Forget project"
          >
            <Icon.Trash size={12} />
          </Button>
        </div>
      ))}
    </div>
  );
}
