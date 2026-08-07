import Editor, { OnMount } from "@monaco-editor/react";
import { useEffect, useRef } from "react";

import { useEditorStore } from "@/state/editor";
import { useSettingsStore } from "@/state/settings";

import "./monacoLoader";

export default function MonacoPaneImpl() {
  const active = useEditorStore((s) =>
    s.activePath ? s.open.find((f) => f.path === s.activePath) ?? null : null,
  );
  const setContent = useEditorStore((s) => s.setContent);
  const save = useEditorStore((s) => s.save);
  const editorSettings = useSettingsStore((s) => s.settings?.editor);
  const editorRef = useRef<Parameters<OnMount>[0] | null>(null);

  // Cmd/Ctrl+S → save the active file.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        if (active) void save(active.path);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active, save]);

  if (!active) {
    return (
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
        Pick a file to edit.
      </div>
    );
  }

  if (active.loading) {
    return (
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
        Loading {active.path}…
      </div>
    );
  }

  if (active.error) {
    return (
      <div
        style={{
          flex: 1,
          padding: 24,
          color: "var(--red)",
          fontSize: 12,
        }}
      >
        Couldn't open: {active.error}
      </div>
    );
  }

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
      {active.truncated && (
        <div
          style={{
            background: "var(--accent-dim)",
            color: "var(--yellow)",
            padding: "4px 12px",
            fontSize: 11,
            borderBottom: "1px solid var(--line-1)",
          }}
        >
          File was truncated — only the first chunk is shown. Save will overwrite the whole file.
        </div>
      )}
      <div style={{ flex: 1, minHeight: 0 }}>
        <Editor
          theme="brain-dark"
          path={active.path}
          language={active.language}
          value={active.content}
          onChange={(value) => setContent(active.path, value ?? "")}
          onMount={(editor) => {
            editorRef.current = editor;
          }}
          options={{
            fontSize: editorSettings?.font_size ?? 13,
            fontFamily: editorSettings?.font_family ?? "var(--font-mono)",
            tabSize: editorSettings?.tab_size ?? 2,
            wordWrap: editorSettings?.word_wrap ? "on" : "off",
            minimap: { enabled: false },
            scrollBeyondLastLine: false,
            renderLineHighlight: "line",
            smoothScrolling: true,
            cursorBlinking: "smooth",
            padding: { top: 8, bottom: 24 },
            automaticLayout: true,
            // Match the rest of the IDE's selection model.
            mouseWheelZoom: true,
            bracketPairColorization: { enabled: true },
            guides: { indentation: true, bracketPairs: "active" },
          }}
        />
      </div>
    </div>
  );
}
