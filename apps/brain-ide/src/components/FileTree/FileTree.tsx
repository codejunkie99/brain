// Two-column file browser: tree on the left, the Monaco editor on the
// right with one tab per opened file.

import { useEffect, useState } from "react";

import * as bridge from "@/ipc/bridge";
import type { TreeNode } from "@/ipc/types";
import { useEditorStore } from "@/state/editor";

import { Button } from "../common/Button";
import { EditorTabs } from "../Editor/EditorTabs";
import { MonacoPane } from "../Editor/MonacoPane";

export function FileTree() {
  const [tree, setTree] = useState<TreeNode | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const activePath = useEditorStore((s) => s.activePath);
  const openFile = useEditorStore((s) => s.openFile);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      const t = await bridge.listTree({});
      setTree(t);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void refresh();
  }, []);

  return (
    <div style={{ flex: 1, display: "flex", minWidth: 0 }}>
      <aside
        style={{
          width: 280,
          background: "var(--bg-1)",
          borderRight: "1px solid var(--line-0)",
          display: "flex",
          flexDirection: "column",
          minHeight: 0,
        }}
      >
        <header
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "8px 10px",
            borderBottom: "1px solid var(--line-0)",
          }}
        >
          <span
            style={{
              fontSize: 11,
              color: "var(--fg-3)",
              letterSpacing: 0.4,
              textTransform: "uppercase",
              flex: 1,
            }}
          >
            Project files
          </span>
          <Button size="sm" variant="ghost" onClick={() => void refresh()}>
            ↻
          </Button>
        </header>
        <div style={{ flex: 1, overflowY: "auto", padding: 4, fontSize: 12 }}>
          {loading && <div style={{ padding: 10, color: "var(--fg-3)" }}>loading…</div>}
          {error && <div style={{ padding: 10, color: "var(--red)" }}>{error}</div>}
          {tree && (
            <TreeBranch
              node={tree}
              depth={0}
              onOpen={(p) => void openFile(p)}
              activePath={activePath}
            />
          )}
        </div>
      </aside>
      <section
        style={{
          flex: 1,
          minWidth: 0,
          display: "flex",
          flexDirection: "column",
          background: "var(--bg-0)",
        }}
      >
        <EditorTabs />
        <MonacoPane />
      </section>
    </div>
  );
}

function TreeBranch({
  node,
  depth,
  onOpen,
  activePath,
}: {
  node: TreeNode;
  depth: number;
  onOpen: (p: string) => void;
  activePath: string | null;
}) {
  const [open, setOpen] = useState(depth < 1);
  const isDir = node.kind === "dir";
  const isActive = !isDir && node.path === activePath;
  const indent = depth * 12;

  return (
    <div>
      <div
        onClick={() => (isDir ? setOpen((x) => !x) : onOpen(node.path))}
        style={{
          padding: `2px 6px 2px ${6 + indent}px`,
          background: isActive ? "var(--bg-3)" : "transparent",
          color: isActive ? "var(--fg-0)" : isDir ? "var(--fg-1)" : "var(--fg-2)",
          cursor: "pointer",
          borderRadius: 3,
          fontSize: 12,
          display: "flex",
          alignItems: "center",
          gap: 4,
          whiteSpace: "nowrap",
          overflow: "hidden",
          textOverflow: "ellipsis",
        }}
      >
        <span style={{ width: 12, color: "var(--fg-3)", fontSize: 9 }}>
          {isDir ? (open ? "▾" : "▸") : ""}
        </span>
        <span>{node.name}</span>
      </div>
      {isDir && open && node.children && (
        <div>
          {node.children.map((child) => (
            <TreeBranch
              key={child.path}
              node={child}
              depth={depth + 1}
              onOpen={onOpen}
              activePath={activePath}
            />
          ))}
        </div>
      )}
    </div>
  );
}
