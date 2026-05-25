// Design tokens. Single source of truth for colors, spacing, radii.
// Themes (themes.ts) just remap the semantic names onto these.

export const palette = {
  // Backgrounds
  bg0: "#0b0d13", // app canvas
  bg1: "#11141c", // panels
  bg2: "#171b25", // raised
  bg3: "#1e2330", // hover
  bg4: "#262c3b", // pressed

  // Borders / separators
  line0: "#1c2230",
  line1: "#272e3e",
  line2: "#3b4256",

  // Foreground
  fg0: "#e7ecf3",
  fg1: "#c6cbd5",
  fg2: "#94a0b3",
  fg3: "#6b768c",

  // Accents
  accent: "#6d5dfc",
  accentSoft: "#3a3380",
  accentDim: "#2a2660",

  // Status
  green: "#3cd28b",
  yellow: "#f5c451",
  red: "#ef5e6e",
  orange: "#f08a3e",
  blue: "#4ea3ff",
  cyan: "#5ad1cd",
  purple: "#a780ff",
  pink: "#ef7fb4",
} as const;

export const spacing = {
  xs: 4,
  sm: 8,
  md: 12,
  lg: 16,
  xl: 24,
  xxl: 32,
} as const;

export const radii = {
  sm: 4,
  md: 6,
  lg: 10,
  pill: 999,
} as const;

export const font = {
  ui: "-apple-system, BlinkMacSystemFont, 'SF Pro Text', 'Helvetica Neue', Arial, sans-serif",
  mono: "ui-monospace, SF Mono, Menlo, monospace",
} as const;

export const z = {
  base: 0,
  panel: 1,
  overlay: 10,
  modal: 100,
  toast: 200,
  dragLayer: 300,
} as const;

export const transitions = {
  fast: "120ms ease-out",
  med: "200ms ease-out",
} as const;

// Map card status -> color slot. Surface this from one place because
// the same mapping is used by the graph node, the card pill in the
// terminal tab strip, and the activity-bar badge.
export const statusColor = {
  pending: palette.fg3,
  ready: palette.blue,
  running: palette.yellow,
  succeeded: palette.green,
  failed: palette.red,
  cancelled: palette.fg3,
  blocked: palette.orange,
} as const;

export const kindAccent = {
  feature: palette.accent,
  refactor: palette.cyan,
  bug_fix: palette.red,
  research: palette.purple,
  test: palette.green,
  docs: palette.pink,
  setup: palette.yellow,
} as const;
