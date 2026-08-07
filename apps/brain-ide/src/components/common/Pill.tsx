import { CSSProperties, ReactNode } from "react";

import { palette, radii } from "@/theme/tokens";

interface Props {
  children: ReactNode;
  color?: string;
  tone?: "solid" | "soft" | "ghost";
  size?: "xs" | "sm" | "md";
  onClick?: () => void;
  style?: CSSProperties;
  title?: string;
}

export function Pill({
  children,
  color = palette.accent,
  tone = "soft",
  size = "sm",
  onClick,
  style,
  title,
}: Props) {
  const fontSize = size === "xs" ? 10 : size === "md" ? 12 : 11;
  const padY = size === "xs" ? 2 : size === "md" ? 4 : 3;
  const padX = size === "xs" ? 6 : size === "md" ? 10 : 8;
  const bg =
    tone === "solid" ? color : tone === "ghost" ? "transparent" : `${color}22`;
  const fg =
    tone === "solid" ? "#0b0d13" : tone === "ghost" ? "var(--fg-2)" : color;
  const border = tone === "ghost" ? `1px solid ${color}55` : "1px solid transparent";

  return (
    <span
      onClick={onClick}
      title={title}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 4,
        padding: `${padY}px ${padX}px`,
        borderRadius: radii.pill,
        background: bg,
        color: fg,
        fontSize,
        fontWeight: 600,
        letterSpacing: 0.2,
        border,
        cursor: onClick ? "pointer" : "default",
        whiteSpace: "nowrap",
        textTransform: tone === "solid" ? "uppercase" : "none",
        ...style,
      }}
    >
      {children}
    </span>
  );
}
