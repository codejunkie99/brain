// Inline SVG icons. Keeping the set small and curated so we don't drag
// in a full icon library. Each icon takes the current color via
// `currentColor`.

import { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

function base(props: IconProps) {
  const { size = 16, ...rest } = props;
  return {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.7,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    ...rest,
  };
}

export const Icon = {
  Chat: (p: IconProps) => (
    <svg {...base(p)}>
      <path d="M21 12a8 8 0 0 1-11.6 7.1L4 21l1.9-5.4A8 8 0 1 1 21 12Z" />
    </svg>
  ),
  Graph: (p: IconProps) => (
    <svg {...base(p)}>
      <circle cx="6" cy="6" r="2.5" />
      <circle cx="18" cy="6" r="2.5" />
      <circle cx="12" cy="18" r="2.5" />
      <path d="M8 7l4 9M16 7l-4 9" />
    </svg>
  ),
  Terminal: (p: IconProps) => (
    <svg {...base(p)}>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M7 9l3 3-3 3M13 15h4" />
    </svg>
  ),
  Files: (p: IconProps) => (
    <svg {...base(p)}>
      <path d="M4 5a2 2 0 0 1 2-2h6l4 4v10a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5Z" />
      <path d="M12 3v5h5" />
    </svg>
  ),
  Memory: (p: IconProps) => (
    <svg {...base(p)}>
      <path d="M12 3v18M3 12h18M5 5l14 14M19 5L5 19" />
    </svg>
  ),
  Settings: (p: IconProps) => (
    <svg {...base(p)}>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a2 2 0 0 0 .4 2.2l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a2 2 0 0 0-2.2-.4 2 2 0 0 0-1.2 1.8V22a2 2 0 1 1-4 0v-.1a2 2 0 0 0-1.2-1.8 2 2 0 0 0-2.2.4l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a2 2 0 0 0 .4-2.2 2 2 0 0 0-1.8-1.2H2a2 2 0 1 1 0-4h.1a2 2 0 0 0 1.8-1.2 2 2 0 0 0-.4-2.2l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a2 2 0 0 0 2.2.4h0a2 2 0 0 0 1.2-1.8V2a2 2 0 1 1 4 0v.1a2 2 0 0 0 1.2 1.8h0a2 2 0 0 0 2.2-.4l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a2 2 0 0 0-.4 2.2v0a2 2 0 0 0 1.8 1.2H22a2 2 0 1 1 0 4h-.1a2 2 0 0 0-1.8 1.2Z" />
    </svg>
  ),
  Play: (p: IconProps) => (
    <svg {...base(p)}>
      <path d="M5 4l14 8-14 8V4Z" fill="currentColor" />
    </svg>
  ),
  Stop: (p: IconProps) => (
    <svg {...base(p)}>
      <rect x="5" y="5" width="14" height="14" rx="2" fill="currentColor" />
    </svg>
  ),
  Plus: (p: IconProps) => (
    <svg {...base(p)}>
      <path d="M12 5v14M5 12h14" />
    </svg>
  ),
  Trash: (p: IconProps) => (
    <svg {...base(p)}>
      <path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M6 6l1 14a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-14" />
    </svg>
  ),
  Send: (p: IconProps) => (
    <svg {...base(p)}>
      <path d="M22 2L11 13M22 2l-7 20-4-9-9-4 20-7Z" />
    </svg>
  ),
  Brain: (p: IconProps) => (
    <svg {...base(p)}>
      <path d="M9 5a3 3 0 1 1 3 3v8a3 3 0 1 1-3 3M15 5a3 3 0 1 0-3 3M15 5v3M9 16v3" />
    </svg>
  ),
  Caret: (p: IconProps) => (
    <svg {...base(p)}>
      <path d="M6 9l6 6 6-6" />
    </svg>
  ),
  Reset: (p: IconProps) => (
    <svg {...base(p)}>
      <path d="M3 12a9 9 0 1 0 3-6.7L3 8" />
      <path d="M3 3v5h5" />
    </svg>
  ),
  Git: (p: IconProps) => (
    <svg {...base(p)}>
      <circle cx="6" cy="18" r="2.5" />
      <circle cx="18" cy="6" r="2.5" />
      <circle cx="18" cy="18" r="2.5" />
      <path d="M6 16V8a2 2 0 0 1 2-2h8M18 8v8" />
    </svg>
  ),
};
