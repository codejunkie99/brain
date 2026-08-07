// Git surface: file status on the left (staged + unstaged groups),
// diff for the selected file on the right, commit composer at the
// bottom. The branch dropdown lives in the header.

import { useEffect, useMemo, useState } from "react";

import * as bridge from "@/ipc/bridge";
import type { FileDiff, RepoStatus, StatusEntry } from "@/ipc/types";

import { Button } from "../common/Button";
import { Icon } from "../common/Icon";
import { Pill } from "../common/Pill";
import { DiffView } from "./DiffView";
import { statusColors, statusLabel } from "./status";

export function GitView() {
  const [status, setStatus] = useState<RepoStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [selectedFromStaged, setSelectedFromStaged] = useState(false);
  const [diff, setDiff] = useState<FileDiff | null>(null);
  const [diffLoading, setDiffLoading] = useState(false);
  const [message, setMessage] = useState("");

  async function refresh() {
    setError(null);
    try {
      const s = await bridge.gitStatus();
      setStatus(s);
      // Preserve selection if the file is still in the list; otherwise
      // pick the first interesting one.
      if (!s.files.some((f) => f.path === selectedPath)) {
        const first = s.files.find((f) => f.stage !== "none") ?? s.files[0];
        setSelectedPath(first?.path ?? null);
        setSelectedFromStaged((first?.stage ?? "none") !== "none");
      }
    } catch (e) {
      setStatus(null);
      setError(String(e));
    }
  }

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Pull a diff whenever the selection changes.
  useEffect(() => {
    if (!selectedPath) {
      setDiff(null);
      return;
    }
    let cancelled = false;
    setDiffLoading(true);
    void bridge
      .gitDiff(selectedPath, selectedFromStaged)
      .then((d) => {
        if (!cancelled) setDiff(d);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setDiffLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedPath, selectedFromStaged]);

  const { staged, unstaged } = useMemo(() => {
    const staged: StatusEntry[] = [];
    const unstaged: StatusEntry[] = [];
    for (const f of status?.files ?? []) {
      if (f.stage !== "none") staged.push(f);
      if (f.work !== "none") unstaged.push(f);
    }
    return { staged, unstaged };
  }, [status]);

  async function withBusy(fn: () => Promise<unknown>) {
    setBusy(true);
    try {
      await fn();
    } finally {
      setBusy(false);
    }
  }

  if (error) {
    return (
      <div style={{ flex: 1, padding: 24, color: "var(--red)", fontSize: 12 }}>
        {error}
        <div style={{ marginTop: 12 }}>
          <Button size="sm" onClick={() => void refresh()}>
            Retry
          </Button>
        </div>
      </div>
    );
  }
  if (!status) {
    return (
      <div
        style={{
          flex: 1,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: "var(--fg-3)",
        }}
      >
        loading…
      </div>
    );
  }

  const onStageFile = (p: string) =>
    void withBusy(async () => {
      await bridge.gitStage([p]);
      await refresh();
    });
  const onUnstageFile = (p: string) =>
    void withBusy(async () => {
      await bridge.gitUnstage([p]);
      await refresh();
    });
  const onStageAll = () =>
    void withBusy(async () => {
      await bridge.gitStage(unstaged.map((f) => f.path));
      await refresh();
    });
  const onCommit = () =>
    void withBusy(async () => {
      if (!message.trim()) return;
      await bridge.gitCommit(message.trim());
      setMessage("");
      await refresh();
    });
  const onCheckout = (branch: string) =>
    void withBusy(async () => {
      await bridge.gitCheckout(branch);
      await refresh();
    });

  return (
    <div style={{ flex: 1, display: "flex", minWidth: 0, minHeight: 0 }}>
      <aside
        style={{
          width: 320,
          background: "var(--bg-1)",
          borderRight: "1px solid var(--line-0)",
          display: "flex",
          flexDirection: "column",
          minHeight: 0,
        }}
      >
        <header
          style={{
            padding: "8px 10px",
            display: "flex",
            alignItems: "center",
            gap: 8,
            borderBottom: "1px solid var(--line-0)",
          }}
        >
          <select
            value={status.head?.branch ?? ""}
            onChange={(e) => onCheckout(e.target.value)}
            style={{
              background: "var(--bg-2)",
              color: "var(--fg-0)",
              padding: "4px 8px",
              border: "1px solid var(--line-1)",
              borderRadius: 6,
              fontSize: 11,
              flex: 1,
              maxWidth: 220,
            }}
            disabled={busy}
          >
            {status.head?.branch && (
              <option value={status.head.branch}>{status.head.branch}</option>
            )}
            {status.branches
              .filter((b) => !b.is_head)
              .map((b) => (
                <option key={`${b.name}-${b.is_remote}`} value={b.name}>
                  {b.is_remote ? "remote/" : ""}
                  {b.name}
                </option>
              ))}
          </select>
          <Button size="sm" variant="ghost" onClick={() => void refresh()}>
            ↻
          </Button>
        </header>

        {status.head && (
          <div
            style={{
              padding: "6px 10px",
              fontSize: 11,
              color: "var(--fg-3)",
              borderBottom: "1px solid var(--line-0)",
              display: "flex",
              gap: 8,
              alignItems: "center",
            }}
          >
            <span style={{ fontFamily: "var(--font-mono)" }}>
              {status.head.commit.slice(0, 7)}
            </span>
            {status.head.ahead > 0 && (
              <Pill color="var(--accent)" tone="soft" size="xs">
                ↑{status.head.ahead}
              </Pill>
            )}
            {status.head.behind > 0 && (
              <Pill color="var(--orange)" tone="soft" size="xs">
                ↓{status.head.behind}
              </Pill>
            )}
            {status.head.detached && (
              <Pill color="var(--red)" tone="soft" size="xs">
                detached
              </Pill>
            )}
          </div>
        )}

        <div style={{ flex: 1, overflowY: "auto" }}>
          <FileGroup
            title={`Staged (${staged.length})`}
            action={
              staged.length > 0 ? (
                <button
                  onClick={() =>
                    void withBusy(async () => {
                      await bridge.gitUnstage(staged.map((f) => f.path));
                      await refresh();
                    })
                  }
                  style={miniBtn}
                  title="Unstage all"
                >
                  −
                </button>
              ) : null
            }
            files={staged}
            selectedPath={selectedFromStaged ? selectedPath : null}
            onSelect={(p) => {
              setSelectedPath(p);
              setSelectedFromStaged(true);
            }}
            onPrimary={(p) => onUnstageFile(p)}
            primaryGlyph="−"
            kind="staged"
          />
          <FileGroup
            title={`Changes (${unstaged.length})`}
            action={
              unstaged.length > 0 ? (
                <button onClick={onStageAll} style={miniBtn} title="Stage all">
                  +
                </button>
              ) : null
            }
            files={unstaged}
            selectedPath={!selectedFromStaged ? selectedPath : null}
            onSelect={(p) => {
              setSelectedPath(p);
              setSelectedFromStaged(false);
            }}
            onPrimary={(p) => onStageFile(p)}
            primaryGlyph="+"
            kind="unstaged"
          />
          {staged.length + unstaged.length === 0 && (
            <div
              style={{
                color: "var(--fg-3)",
                fontSize: 11,
                padding: 24,
                textAlign: "center",
              }}
            >
              Working tree clean.
            </div>
          )}
        </div>

        <div
          style={{
            borderTop: "1px solid var(--line-0)",
            padding: 10,
          }}
        >
          <textarea
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            placeholder={
              staged.length
                ? "Commit message…"
                : "Stage files to commit."
            }
            rows={3}
            style={{
              width: "100%",
              background: "var(--bg-2)",
              color: "var(--fg-0)",
              border: "1px solid var(--line-1)",
              borderRadius: 6,
              padding: "6px 8px",
              fontSize: 12,
              fontFamily: "var(--font-ui)",
              resize: "none",
              minHeight: 60,
            }}
          />
          <div
            style={{
              display: "flex",
              justifyContent: "flex-end",
              marginTop: 6,
            }}
          >
            <Button
              size="sm"
              variant="primary"
              disabled={!message.trim() || staged.length === 0 || busy}
              onClick={onCommit}
              leftIcon={<Icon.Brain size={12} />}
            >
              Commit {staged.length || ""}
            </Button>
          </div>
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
        {selectedPath ? (
          diff ? (
            <DiffView diff={diff} />
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
              {diffLoading ? "loading diff…" : "no diff"}
            </div>
          )
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
            Pick a file to see its diff.
          </div>
        )}
      </section>
    </div>
  );
}

function FileGroup({
  title,
  files,
  onSelect,
  onPrimary,
  primaryGlyph,
  selectedPath,
  action,
  kind,
}: {
  title: string;
  files: StatusEntry[];
  onSelect: (p: string) => void;
  onPrimary: (p: string) => void;
  primaryGlyph: string;
  selectedPath: string | null;
  action: React.ReactNode;
  kind: "staged" | "unstaged";
}) {
  return (
    <div style={{ marginBottom: 6 }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          padding: "6px 10px",
          fontSize: 10,
          textTransform: "uppercase",
          letterSpacing: 0.4,
          color: "var(--fg-3)",
          background: "var(--bg-1)",
        }}
      >
        <span style={{ flex: 1 }}>{title}</span>
        {action}
      </div>
      {files.map((f) => {
        const which = kind === "staged" ? f.stage : f.work;
        const color = statusColors[which] ?? "var(--fg-2)";
        const isActive = selectedPath === f.path;
        return (
          <div
            key={`${kind}-${f.path}`}
            onClick={() => onSelect(f.path)}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              padding: "3px 10px",
              cursor: "pointer",
              background: isActive ? "var(--bg-3)" : "transparent",
            }}
          >
            <span
              style={{
                color,
                width: 12,
                fontSize: 11,
                fontFamily: "var(--font-mono)",
              }}
              title={statusLabel(which)}
            >
              {glyphFor(which)}
            </span>
            <span
              style={{
                flex: 1,
                fontSize: 12,
                color: isActive ? "var(--fg-0)" : "var(--fg-1)",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {f.path}
            </span>
            <button
              onClick={(e) => {
                e.stopPropagation();
                onPrimary(f.path);
              }}
              style={miniBtn}
              title={
                kind === "staged"
                  ? `Unstage ${f.path}`
                  : `Stage ${f.path}`
              }
            >
              {primaryGlyph}
            </button>
          </div>
        );
      })}
    </div>
  );
}

function glyphFor(kind: string): string {
  switch (kind) {
    case "added":
      return "A";
    case "modified":
      return "M";
    case "deleted":
      return "D";
    case "renamed":
      return "R";
    case "typechange":
      return "T";
    case "untracked":
      return "?";
    case "ignored":
      return "!";
    default:
      return "·";
  }
}

const miniBtn: React.CSSProperties = {
  color: "var(--fg-2)",
  background: "transparent",
  border: "1px solid var(--line-1)",
  borderRadius: 4,
  width: 18,
  height: 18,
  fontSize: 12,
  lineHeight: 1,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
};
