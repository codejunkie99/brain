// Live view of the brain memory log. Backed by `memory_recent` /
// `memory_search` commands which read the underlying git-backed brain.

import { useEffect, useState } from "react";

import * as bridge from "@/ipc/bridge";
import type { MemoryEntry } from "@/ipc/types";

import { Button } from "../common/Button";
import { Pill } from "../common/Pill";

export function MemoryLog() {
  const [entries, setEntries] = useState<MemoryEntry[]>([]);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [note, setNote] = useState("");

  async function refresh() {
    setLoading(true);
    try {
      const list = query
        ? await bridge.memorySearch(query)
        : await bridge.memoryRecent(80);
      setEntries(list);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function commitNote() {
    const v = note.trim();
    if (!v) return;
    await bridge.memoryNote(v);
    setNote("");
    void refresh();
  }

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", background: "var(--bg-0)" }}>
      <header
        style={{
          display: "flex",
          gap: 8,
          padding: "10px 12px",
          borderBottom: "1px solid var(--line-0)",
          background: "var(--bg-1)",
        }}
      >
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void refresh();
          }}
          placeholder="Search memory…"
          style={{
            flex: 1,
            background: "var(--bg-2)",
            color: "var(--fg-0)",
            border: "1px solid var(--line-1)",
            borderRadius: 6,
            padding: "5px 10px",
            fontSize: 12,
          }}
        />
        <Button size="sm" onClick={() => void refresh()} disabled={loading}>
          Search
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            setQuery("");
            void refresh();
          }}
        >
          Recent
        </Button>
      </header>
      <div
        style={{
          padding: "8px 12px",
          borderBottom: "1px solid var(--line-0)",
          display: "flex",
          gap: 8,
          background: "var(--bg-1)",
        }}
      >
        <input
          value={note}
          onChange={(e) => setNote(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void commitNote();
          }}
          placeholder="Quick note (commits to brain)…"
          style={{
            flex: 1,
            background: "var(--bg-2)",
            color: "var(--fg-0)",
            border: "1px solid var(--line-1)",
            borderRadius: 6,
            padding: "5px 10px",
            fontSize: 12,
          }}
        />
        <Button size="sm" variant="primary" onClick={() => void commitNote()}>
          Note
        </Button>
      </div>
      <div style={{ flex: 1, overflowY: "auto", padding: 8 }}>
        {entries.length === 0 && (
          <div style={{ color: "var(--fg-3)", padding: 24, textAlign: "center", fontSize: 12 }}>
            {loading ? "loading…" : "no entries"}
          </div>
        )}
        {entries.map((e) => (
          <div
            key={e.event_id}
            style={{
              padding: 10,
              border: "1px solid var(--line-0)",
              borderRadius: 6,
              marginBottom: 6,
              background: "var(--bg-1)",
              userSelect: "text",
            }}
          >
            <div style={{ display: "flex", gap: 8, marginBottom: 4 }}>
              <Pill color="var(--accent)" tone="soft" size="xs">
                {e.kind}
              </Pill>
              <span style={{ color: "var(--fg-3)", fontSize: 10 }}>
                {e.actor}
              </span>
              <span style={{ flex: 1 }} />
              <span style={{ color: "var(--fg-3)", fontSize: 10 }}>
                {new Date(e.recorded_at).toLocaleString()}
              </span>
            </div>
            <div style={{ fontSize: 12, color: "var(--fg-1)" }}>{e.summary}</div>
            <div style={{ color: "var(--fg-3)", fontSize: 10, marginTop: 4, fontFamily: "var(--font-mono)" }}>
              {e.commit_oid.slice(0, 12)}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
