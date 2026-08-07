import { ButtonHTMLAttributes, ReactNode } from "react";

import { palette, radii } from "@/theme/tokens";

interface Props extends ButtonHTMLAttributes<HTMLButtonElement> {
  children: ReactNode;
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md";
  leftIcon?: ReactNode;
}

export function Button({
  children,
  variant = "secondary",
  size = "md",
  leftIcon,
  style,
  ...rest
}: Props) {
  const padding = size === "sm" ? "5px 10px" : "8px 14px";
  const fontSize = size === "sm" ? 12 : 13;
  let bg = "var(--bg-2)";
  let color = "var(--fg-1)";
  let border = "1px solid var(--line-1)";
  if (variant === "primary") {
    bg = palette.accent;
    color = "#0b0d13";
    border = "1px solid transparent";
  } else if (variant === "danger") {
    bg = "transparent";
    color = palette.red;
    border = `1px solid ${palette.red}55`;
  } else if (variant === "ghost") {
    bg = "transparent";
    color = "var(--fg-2)";
    border = "1px solid transparent";
  }
  return (
    <button
      {...rest}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        padding,
        background: bg,
        color,
        border,
        borderRadius: radii.md,
        fontSize,
        fontWeight: 500,
        ...style,
      }}
    >
      {leftIcon}
      {children}
    </button>
  );
}
