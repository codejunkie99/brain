import { Icon } from "../common/Icon";

export type WorkspaceView =
  | "graph"
  | "files"
  | "git"
  | "memory"
  | "settings";

interface Props {
  view: WorkspaceView;
  onChangeView: (v: WorkspaceView) => void;
  chatOpen: boolean;
  onToggleChat: () => void;
  terminalOpen: boolean;
  onToggleTerminal: () => void;
}

const ITEMS: { view: WorkspaceView; label: string; icon: typeof Icon.Graph }[] = [
  { view: "graph", label: "Feature graph", icon: Icon.Graph },
  { view: "files", label: "Files", icon: Icon.Files },
  { view: "git", label: "Git", icon: Icon.Git },
  { view: "memory", label: "Memory log", icon: Icon.Memory },
  { view: "settings", label: "Settings", icon: Icon.Settings },
];

export function ActivityBar({
  view,
  onChangeView,
  chatOpen,
  onToggleChat,
  terminalOpen,
  onToggleTerminal,
}: Props) {
  return (
    <nav
      aria-label="Workspace"
      style={{
        width: 48,
        background: "var(--bg-1)",
        borderRight: "1px solid var(--line-0)",
        display: "flex",
        flexDirection: "column",
        alignItems: "stretch",
        padding: "30px 0 6px",
      }}
    >
      <ActivityIcon
        active={chatOpen}
        label={chatOpen ? "Hide chat" : "Show chat"}
        onClick={onToggleChat}
      >
        <Icon.Chat size={18} />
      </ActivityIcon>
      <div
        style={{
          height: 1,
          background: "var(--line-0)",
          margin: "6px 8px",
        }}
      />
      {ITEMS.map((item) => {
        const I = item.icon;
        return (
          <ActivityIcon
            key={item.view}
            active={view === item.view}
            label={item.label}
            onClick={() => onChangeView(item.view)}
          >
            <I size={18} />
          </ActivityIcon>
        );
      })}
      <div style={{ flex: 1 }} />
      <ActivityIcon
        active={terminalOpen}
        label={terminalOpen ? "Hide terminal" : "Show terminal"}
        onClick={onToggleTerminal}
      >
        <Icon.Terminal size={18} />
      </ActivityIcon>
    </nav>
  );
}

function ActivityIcon({
  children,
  active,
  label,
  onClick,
}: {
  children: React.ReactNode;
  active: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      title={label}
      aria-label={label}
      style={{
        height: 40,
        margin: "2px 4px",
        borderRadius: 6,
        background: active ? "var(--bg-3)" : "transparent",
        color: active ? "var(--fg-0)" : "var(--fg-2)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        position: "relative",
      }}
    >
      {active && (
        <span
          aria-hidden
          style={{
            position: "absolute",
            left: 0,
            top: 8,
            bottom: 8,
            width: 2,
            borderRadius: 2,
            background: "var(--accent)",
          }}
        />
      )}
      {children}
    </button>
  );
}
