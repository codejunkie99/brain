// Editor store. Tracks open files across the IDE — the FileTree opens
// files here, EditorTabs renders the tab strip, MonacoPane edits the
// active file.

import { create } from "zustand";

import * as bridge from "@/ipc/bridge";

export interface OpenFile {
  path: string;
  /** Last content saved to disk. */
  baseline: string;
  /** Current buffer content. */
  content: string;
  language: string;
  truncated: boolean;
  loading: boolean;
  error: string | null;
}

interface EditorState {
  open: OpenFile[];
  activePath: string | null;
  openFile: (path: string) => Promise<void>;
  closeFile: (path: string) => void;
  setActive: (path: string | null) => void;
  setContent: (path: string, content: string) => void;
  save: (path: string) => Promise<void>;
  saveAll: () => Promise<void>;
}

export const useEditorStore = create<EditorState>((set, get) => ({
  open: [],
  activePath: null,

  openFile: async (path) => {
    const existing = get().open.find((f) => f.path === path);
    if (existing) {
      set({ activePath: path });
      return;
    }
    // Insert a placeholder loading entry so the tab strip updates immediately.
    set((s) => ({
      open: [
        ...s.open,
        {
          path,
          baseline: "",
          content: "",
          language: detectLanguage(path),
          truncated: false,
          loading: true,
          error: null,
        },
      ],
      activePath: path,
    }));
    try {
      const { content, truncated } = await bridge.readFile(path);
      set((s) => ({
        open: s.open.map((f) =>
          f.path === path
            ? { ...f, content, baseline: content, truncated, loading: false }
            : f,
        ),
      }));
    } catch (e) {
      set((s) => ({
        open: s.open.map((f) =>
          f.path === path
            ? { ...f, error: String(e), loading: false }
            : f,
        ),
      }));
    }
  },

  closeFile: (path) => {
    set((s) => {
      const open = s.open.filter((f) => f.path !== path);
      return {
        open,
        activePath:
          s.activePath === path
            ? (open[open.length - 1]?.path ?? null)
            : s.activePath,
      };
    });
  },

  setActive: (path) => set({ activePath: path }),

  setContent: (path, content) => {
    set((s) => ({
      open: s.open.map((f) => (f.path === path ? { ...f, content } : f)),
    }));
  },

  save: async (path) => {
    const file = get().open.find((f) => f.path === path);
    if (!file) return;
    if (file.content === file.baseline) return;
    await bridge.writeFile(path, file.content);
    set((s) => ({
      open: s.open.map((f) =>
        f.path === path ? { ...f, baseline: file.content } : f,
      ),
    }));
  },

  saveAll: async () => {
    const dirty = get().open.filter((f) => f.content !== f.baseline);
    for (const f of dirty) {
      await bridge.writeFile(f.path, f.content);
    }
    set((s) => ({
      open: s.open.map((f) => ({ ...f, baseline: f.content })),
    }));
  },
}));

// Hand-rolled extension → Monaco language id table. Monaco's bundled
// languages cover the common cases; unknown extensions fall back to
// plaintext (Monaco still gives you indent/folding heuristics).
const EXT: Record<string, string> = {
  ts: "typescript",
  tsx: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  json: "json",
  jsonc: "json",
  rs: "rust",
  py: "python",
  go: "go",
  rb: "ruby",
  java: "java",
  kt: "kotlin",
  swift: "swift",
  c: "c",
  h: "c",
  cpp: "cpp",
  hpp: "cpp",
  cc: "cpp",
  cs: "csharp",
  php: "php",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  fish: "shell",
  toml: "ini",
  ini: "ini",
  yaml: "yaml",
  yml: "yaml",
  md: "markdown",
  mdx: "markdown",
  html: "html",
  htm: "html",
  css: "css",
  scss: "scss",
  less: "less",
  sql: "sql",
  graphql: "graphql",
  gql: "graphql",
  dockerfile: "dockerfile",
  proto: "proto",
  lock: "yaml",
};

export function detectLanguage(path: string): string {
  const lower = path.toLowerCase();
  if (lower.endsWith("dockerfile") || lower.endsWith("/dockerfile")) {
    return "dockerfile";
  }
  const dot = lower.lastIndexOf(".");
  if (dot < 0) return "plaintext";
  const ext = lower.slice(dot + 1);
  return EXT[ext] ?? "plaintext";
}
