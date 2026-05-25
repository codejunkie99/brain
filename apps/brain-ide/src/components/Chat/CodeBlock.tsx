// Shiki-highlighted code block for chat markdown. Loaded lazily — the
// first message with code pays the bundle cost, every subsequent block
// reuses the cached highlighter singleton.

import { useEffect, useRef, useState } from "react";
import type { HighlighterCore } from "shiki/core";

// One-shot highlighter. We use shiki/core + per-language fine-grained
// imports so the bundle only carries the grammars we list. Adding a
// new language is a one-line change here.
let highlighterPromise: Promise<HighlighterCore> | null = null;
const loaded = new Set<string>();

// Grammar loaders — lazy ESM imports keep them out of the main chunk
// and Vite ships them as small per-language files.
const LANG_LOADERS: Record<string, () => Promise<unknown>> = {
  typescript: () => import("shiki/langs/typescript.mjs"),
  tsx: () => import("shiki/langs/tsx.mjs"),
  javascript: () => import("shiki/langs/javascript.mjs"),
  jsx: () => import("shiki/langs/jsx.mjs"),
  rust: () => import("shiki/langs/rust.mjs"),
  python: () => import("shiki/langs/python.mjs"),
  go: () => import("shiki/langs/go.mjs"),
  json: () => import("shiki/langs/json.mjs"),
  bash: () => import("shiki/langs/bash.mjs"),
  shell: () => import("shiki/langs/shellscript.mjs"),
  yaml: () => import("shiki/langs/yaml.mjs"),
  markdown: () => import("shiki/langs/markdown.mjs"),
  html: () => import("shiki/langs/html.mjs"),
  css: () => import("shiki/langs/css.mjs"),
  sql: () => import("shiki/langs/sql.mjs"),
  diff: () => import("shiki/langs/diff.mjs"),
  toml: () => import("shiki/langs/toml.mjs"),
  ruby: () => import("shiki/langs/ruby.mjs"),
  java: () => import("shiki/langs/java.mjs"),
};

async function getHighlighter(): Promise<HighlighterCore> {
  if (!highlighterPromise) {
    highlighterPromise = (async () => {
      const [{ createHighlighterCore }, { createOnigurumaEngine }, theme] =
        await Promise.all([
          import("shiki/core"),
          import("shiki/engine/oniguruma"),
          import("shiki/themes/github-dark-dimmed.mjs"),
        ]);
      return createHighlighterCore({
        themes: [theme.default],
        langs: [],
        engine: createOnigurumaEngine(import("shiki/wasm")),
      });
    })();
  }
  return highlighterPromise;
}

async function ensureLanguage(lang: string): Promise<string> {
  const normalized = normalizeLang(lang);
  if (loaded.has(normalized)) return normalized;
  const loader = LANG_LOADERS[normalized];
  if (!loader) return "text";
  try {
    const h = await getHighlighter();
    const mod = (await loader()) as { default: Parameters<typeof h.loadLanguage>[0] };
    await h.loadLanguage(mod.default);
    loaded.add(normalized);
    return normalized;
  } catch {
    return "text";
  }
}

function normalizeLang(s: string): string {
  const lower = s.toLowerCase().trim();
  if (!lower) return "text";
  if (lower === "ts") return "typescript";
  if (lower === "js") return "javascript";
  if (lower === "rs") return "rust";
  if (lower === "py") return "python";
  if (lower === "sh") return "bash";
  if (lower === "zsh") return "bash";
  if (lower === "rb") return "ruby";
  if (lower === "yml") return "yaml";
  return lower;
}

interface Props {
  className?: string;
  children?: React.ReactNode;
}

/**
 * Drop-in replacement for ReactMarkdown's `code` renderer. Renders
 * inline code as a styled `<code>` and fenced code blocks as
 * Shiki-highlighted HTML.
 */
export function CodeBlock({ className, children }: Props) {
  const match = /language-(\w+)/.exec(className ?? "");
  const isBlock = !!match;
  const code = String(children ?? "").replace(/\n$/, "");
  const lang = match?.[1] ?? "";

  if (!isBlock) {
    return (
      <code
        style={{
          background: "var(--bg-3)",
          padding: "1px 5px",
          borderRadius: 4,
          fontSize: "0.92em",
          fontFamily: "var(--font-mono)",
          color: "var(--accent)",
        }}
      >
        {children}
      </code>
    );
  }

  return <FencedBlock code={code} lang={lang} />;
}

function FencedBlock({ code, lang }: { code: string; lang: string }) {
  const [html, setHtml] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const lastJob = useRef(0);

  useEffect(() => {
    const job = ++lastJob.current;
    void (async () => {
      const resolvedLang = await ensureLanguage(lang);
      try {
        const h = await getHighlighter();
        const out = h.codeToHtml(code, {
          lang: resolvedLang === "text" ? "text" : resolvedLang,
          theme: "github-dark-dimmed",
        });
        if (job === lastJob.current) setHtml(out);
      } catch {
        if (job === lastJob.current) setHtml(null);
      }
    })();
  }, [code, lang]);

  return (
    <div
      style={{
        position: "relative",
        margin: "8px 0",
        borderRadius: 8,
        overflow: "hidden",
        border: "1px solid var(--line-1)",
        background: "#0b0d13",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          padding: "4px 10px",
          background: "var(--bg-2)",
          borderBottom: "1px solid var(--line-1)",
          fontSize: 10,
          color: "var(--fg-3)",
          letterSpacing: 0.4,
          textTransform: "lowercase",
        }}
      >
        <span style={{ flex: 1 }}>{lang || "text"}</span>
        <button
          onClick={() => {
            void navigator.clipboard.writeText(code);
            setCopied(true);
            window.setTimeout(() => setCopied(false), 1500);
          }}
          style={{
            color: copied ? "var(--green)" : "var(--fg-2)",
            fontSize: 10,
          }}
        >
          {copied ? "copied" : "copy"}
        </button>
      </div>
      {html ? (
        <div
          className="shiki-block"
          style={{
            fontSize: 12,
            lineHeight: 1.55,
            padding: "10px 12px",
            overflowX: "auto",
            fontFamily: "var(--font-mono)",
          }}
          dangerouslySetInnerHTML={{ __html: html }}
        />
      ) : (
        <pre
          style={{
            margin: 0,
            padding: "10px 12px",
            fontSize: 12,
            lineHeight: 1.55,
            fontFamily: "var(--font-mono)",
            color: "var(--fg-1)",
            overflowX: "auto",
          }}
        >
          {code}
        </pre>
      )}
    </div>
  );
}
