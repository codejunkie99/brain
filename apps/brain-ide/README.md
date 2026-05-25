# brain-ide

A native macOS IDE for orchestrating Claude, Codex, and shell sessions
against a shared brain memory graph. Built with **Tauri 2 + Rust** and
**React + TypeScript**.

```
┌────┬───────────┬───────────────────────────────────────┬─────────┐
│ ▮  │           │                                       │         │
│ A  │   chat    │            feature graph              │ agents  │
│ c  │  (claude/ │                                       │  ───    │
│ t  │   codex/  │     drag any card onto an agent       │ claude  │
│ B  │   shell)  │                                       │ codex   │
│ a  │           │                                       │ shell   │
│ r  ├───────────┼───────────────────────────────────────┤ human   │
│    │           │   terminal tabs                       │         │
│    │           ├───────────────────────────────────────┤         │
│    │           │   status bar                          │         │
└────┴───────────┴───────────────────────────────────────┴─────────┘
```

## Why this exists

You're building features faster than any one model can keep up with.
You want one chat surface that survives switching between Claude and
Codex mid-conversation, one graph where you can see all the in-flight
work and how it's wired together, and one memory layer everyone reads
from and writes to. The IDE is that surface.

## Architecture

| Layer | Stack | Lives in |
|---|---|---|
| Memory | git-backed event log | `crates/brain-store`, `crates/brain-index` |
| Orchestration | task graph + scheduler | `crates/brain-orchestrator` |
| App backend | Tauri 2 commands, agent runners, PTY | `apps/brain-ide/src-tauri` |
| UI | React + Zustand + React Flow + xterm.js + Monaco | `apps/brain-ide/src` |

Agents:
- **Chat agents** stream from the Anthropic Messages API / OpenAI
  Responses API directly. Set keys in Settings → API keys (stored
  in the macOS Keychain).
- **Card agents** spawn the local `claude` / `codex` CLIs as
  subprocesses for actual code execution. Configure their paths and
  default models in Settings → Agents.
- **Shell** and **Human** agents are first-class assignees too.

## Prereqs

- macOS 11+
- Rust 1.85+ (`rustup toolchain install stable`)
- Node 22+ (`brew install node` or [Volta](https://volta.sh))
- (Optional) `claude` CLI and/or `codex` CLI on `$PATH` for card
  execution. The chat panel works with only API keys.

## Run

From `apps/brain-ide/`:

```bash
npm install
npm run dev   # opens the IDE window, hot-reloads on save
```

The first run prompts you to pick a project root and (optionally) add
API keys. Everything is local-first.

## Build a signed `.dmg`

```bash
npm run dist
```

Tauri 2 picks up code signing if `APPLE_CERTIFICATE`,
`APPLE_CERTIFICATE_PASSWORD`, `APPLE_ID`, `APPLE_PASSWORD`, and
`APPLE_TEAM_ID` are set in the environment. Without them you get an
unsigned `.app` and `.dmg` that still launch on your own machine.

## Layout

```
apps/brain-ide/
├── src/                     React + TypeScript frontend
│   ├── App.tsx              shell + boot logic
│   ├── ipc/                 typed Tauri command wrappers + event subs
│   ├── state/               zustand stores (chat, graph, terminals, …)
│   ├── components/
│   │   ├── Layout/          activity bar, status bar, shell
│   │   ├── Chat/            persistent chat with model switcher
│   │   ├── Graph/           React Flow card graph + drop lanes
│   │   ├── Terminal/        xterm.js bound to backend PTYs
│   │   ├── FileTree/        project file browser
│   │   ├── Memory/          live brain event log
│   │   ├── Settings/        general/agents/keys/projects panels
│   │   └── Onboarding/      first-run flow
│   └── theme/               dark theme + design tokens
└── src-tauri/               Tauri 2 backend
    ├── tauri.conf.json
    ├── capabilities/        permission grants per window
    ├── entitlements.plist   macOS sandbox entitlements
    └── src/
        ├── agents/          Claude/Codex API + CLI, shell, human
        ├── orchestrator/    bridges brain-orchestrator to Tauri events
        ├── memory/          wraps brain-store
        ├── pty/             portable-pty session manager
        ├── projects/        project store
        ├── settings/        settings store
        ├── auth/            keychain-backed API keys
        └── commands/        every #[tauri::command]
```

## Keyboard shortcuts

| Key | Action |
|---|---|
| ⌘↩ | Send message (chat) |
| Click on card | Open inspector |
| Drag card → agent lane | Assign card |
| Click any agent in lane → set model | Switch model for next dispatch |

## Privacy

- Memory is git on local disk under `<project>/.brain`. Nothing
  syncs unless you `brain remote add` it.
- API keys live in the macOS Keychain. The frontend never sees them.
- Telemetry is off by default.
