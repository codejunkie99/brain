// Theme application — currently a single dark theme. Light/high-contrast
// would override the CSS variables this writes into :root.

import { font, palette } from "./tokens";

import type { Theme } from "@/ipc/types";

export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  const effective: Exclude<Theme, "system"> =
    theme === "system"
      ? window.matchMedia("(prefers-color-scheme: light)").matches
        ? "light"
        : "dark"
      : theme;

  const tokens =
    effective === "light"
      ? lightTokens
      : effective === "high_contrast"
        ? highContrastTokens
        : darkTokens;

  for (const [k, v] of Object.entries(tokens)) {
    root.style.setProperty(`--${k}`, v as string);
  }
  root.style.setProperty("--font-ui", font.ui);
  root.style.setProperty("--font-mono", font.mono);
  root.dataset.theme = effective;
}

const darkTokens: Record<string, string> = {
  "bg-0": palette.bg0,
  "bg-1": palette.bg1,
  "bg-2": palette.bg2,
  "bg-3": palette.bg3,
  "bg-4": palette.bg4,
  "line-0": palette.line0,
  "line-1": palette.line1,
  "line-2": palette.line2,
  "fg-0": palette.fg0,
  "fg-1": palette.fg1,
  "fg-2": palette.fg2,
  "fg-3": palette.fg3,
  accent: palette.accent,
  "accent-soft": palette.accentSoft,
  "accent-dim": palette.accentDim,
  green: palette.green,
  yellow: palette.yellow,
  red: palette.red,
  blue: palette.blue,
  cyan: palette.cyan,
  purple: palette.purple,
  pink: palette.pink,
  orange: palette.orange,
};

const lightTokens: Record<string, string> = {
  "bg-0": "#f6f7fa",
  "bg-1": "#ffffff",
  "bg-2": "#f1f3f7",
  "bg-3": "#e6eaf0",
  "bg-4": "#d6dce6",
  "line-0": "#e0e3ea",
  "line-1": "#cdd3de",
  "line-2": "#b4bcc9",
  "fg-0": "#0b0d13",
  "fg-1": "#28324a",
  "fg-2": "#586378",
  "fg-3": "#7a8597",
  accent: "#5b4cf0",
  "accent-soft": "#dcd9ff",
  "accent-dim": "#efeefe",
  green: "#1a9d63",
  yellow: "#b48218",
  red: "#cc3d4e",
  blue: "#2c7be5",
  cyan: "#2aa8a3",
  purple: "#7857d8",
  pink: "#cf4c8e",
  orange: "#c66124",
};

const highContrastTokens: Record<string, string> = {
  ...darkTokens,
  "bg-0": "#000000",
  "bg-1": "#0c0f17",
  "bg-2": "#10131e",
  "fg-0": "#ffffff",
  "fg-1": "#dfe6f0",
  accent: "#9ab3ff",
  "line-1": "#3a4360",
};
