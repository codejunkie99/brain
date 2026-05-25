// The main IDE layout:
//
//   ┌────┬──────────────────────────────────────────────────┐
//   │    │                                                  │
//   │ A  │   center workspace (graph / files / memory)      │
//   │ c  │                                                  │
//   │ t  ├──────────────────────────────────────────────────┤
//   │ i  │   terminal tabs                                  │
//   │ v  ├──────────────────────────────────────────────────┤
//   │ B  │   status bar                                     │
//   │    │                                                  │
//   └────┴──────────────────────────────────────────────────┘
//
// The left chat sidebar is the *anchor* of the IDE — a persistent
// conversation that survives model switches. The right side flips
// between the feature graph, the file tree, and the memory log.

import { useState } from "react";

import { palette } from "@/theme/tokens";

import { ChatPanel } from "../Chat/ChatPanel";
import { FileTree } from "../FileTree/FileTree";
import { GitView } from "../Git/GitView";
import { GraphView } from "../Graph/GraphView";
import { MemoryLog } from "../Memory/MemoryLog";
import { SettingsPanel } from "../Settings/SettingsPanel";
import { TerminalTabs } from "../Terminal/TerminalTabs";
import { ActivityBar, type WorkspaceView } from "./ActivityBar";
import { StatusBar } from "./StatusBar";

const CHAT_WIDTH = 360;
const TERMINAL_HEIGHT = 280;

export function AppShell() {
  const [view, setView] = useState<WorkspaceView>("graph");
  const [chatOpen, setChatOpen] = useState(true);
  const [terminalOpen, setTerminalOpen] = useState(true);

  return (
    <div
      style={{
        flex: 1,
        display: "flex",
        flexDirection: "row",
        minHeight: 0,
        minWidth: 0,
        background: "var(--bg-0)",
      }}
    >
      <ActivityBar
        view={view}
        onChangeView={setView}
        chatOpen={chatOpen}
        onToggleChat={() => setChatOpen((x) => !x)}
        terminalOpen={terminalOpen}
        onToggleTerminal={() => setTerminalOpen((x) => !x)}
      />

      {chatOpen && (
        <aside
          style={{
            width: CHAT_WIDTH,
            borderRight: `1px solid ${palette.line0}`,
            display: "flex",
            flexDirection: "column",
            minHeight: 0,
          }}
        >
          <ChatPanel />
        </aside>
      )}

      <main
        style={{
          flex: 1,
          minWidth: 0,
          display: "flex",
          flexDirection: "column",
        }}
      >
        <div className="titlebar-drag" style={{ height: 28, flexShrink: 0 }} />
        <div
          style={{
            flex: 1,
            minHeight: 0,
            display: "flex",
            flexDirection: "column",
          }}
        >
          <section
            style={{ flex: 1, minHeight: 0, display: "flex", overflow: "hidden" }}
          >
            {view === "graph" && <GraphView />}
            {view === "files" && <FileTree />}
            {view === "git" && <GitView />}
            {view === "memory" && <MemoryLog />}
            {view === "settings" && <SettingsPanel />}
          </section>
          {terminalOpen && (
            <section
              style={{
                height: TERMINAL_HEIGHT,
                borderTop: `1px solid ${palette.line0}`,
                background: "var(--bg-1)",
                flexShrink: 0,
                display: "flex",
                minHeight: 0,
              }}
            >
              <TerminalTabs />
            </section>
          )}
          <StatusBar />
        </div>
      </main>
    </div>
  );
}
