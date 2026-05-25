// Two-column file browser: tree on the left, file content on the right.

import { useEffect, useState } from "react";

import * as bridge from "@/ipc/bridge";
import type { TreeNode } from "@/ipc/types";

import { Button } from "../common/Button";

export function FileTree() {
  const [tree, setTree] = useState<TreeNode | null>(null);
  const [active, setActive] = useState<{ path: string; content: string; truncated: boolean } | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

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

  async function openFile(path: string) {
    try {
      const res = await bridge.readFile(path);
      setActive({ path, content: res.content, truncated: res.truncated });
    } catch (e) {
      setError(String(e));
    }
  }

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
          <span style={{ fontSize: 11, color: "var(--fg-3)", letterSpacing: 0.4, textTransform: "uppercase", flex: 1 }}>
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
              onOpen={openFile}
              activePath={active?.path ?? null}
            />
          )}
        </div>
      </aside>
      <section style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", background: "var(--bg-0)" }}>
        {active ? (
          <>
            <header
              style={{
                padding: "8px 12px",
                borderBottom: "1px solid var(--line-0)",
                background: "var(--bg-1)",
                fontSize: 12,
                color: "var(--fg-1)",
                display: "flex",
                gap: 8,
              }}
            >
              <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {active.path}
              </span>
              {active.truncated && (
                <span style={{ color: "var(--yellow)" }}>truncated</span>
              )}
            </header>
            <pre
              style={{
                flex: 1,
                overflow: "auto",
                padding: 12,
                margin: 0,
                fontFamily: "var(--font-mono)",
                fontSize: 12,
                lineHeight: 1.5,
                userSelect: "text",
                whiteSpace: "pre",
              }}
            >
              {active.content}
            </pre>
          </>
        ) : (
          <div
            style={{
              flex: 1,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              color: "var(--fg-3)",
              fontSize: 12,
            }}
          >
            Pick a file to view it.
          </div>
        )}
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
