// Types mirroring the Rust serde shapes. Hand-maintained — the trade-
// off vs schema generation is one less build step at the cost of
// occasional drift; the surface is small enough to keep in sync.

export type Uuid = string;

// ----- Agents -----

export type AgentKind = "api" | "cli" | "human";

export interface AgentDescriptor {
  slug: string;
  label: string;
  kind: AgentKind;
  default_model: string | null;
  available_models: string[];
  supports_chat: boolean;
  supports_card_execution: boolean;
  ready: boolean;
  readiness_message: string | null;
}

export type CredentialKind = "anthropic" | "openai" | "custom";
export interface CredentialStatus {
  kind: CredentialKind;
  is_set: boolean;
}

// ----- Chat -----

export type ChatRole = "user" | "assistant" | "system" | "tool";

export interface ChatMessage {
  id: Uuid;
  role: ChatRole;
  content: string;
  model: string | null;
  meta: unknown;
  created_at: string;
}

export interface ChatThread {
  id: Uuid;
  title: string;
  messages: ChatMessage[];
  created_at: string;
  updated_at: string;
}

export type ChatChunk =
  | { type: "delta"; text: string }
  | { type: "thinking"; text: string }
  | { type: "tool_use"; id: string; name: string; input: unknown }
  | {
      type: "done";
      input_tokens: number | null;
      output_tokens: number | null;
      cached_tokens: number | null;
      stop_reason: string | null;
    }
  | { type: "error"; message: string };

export interface ChatStreamEvent {
  thread_id: Uuid;
  assistant_message_id: Uuid;
  chunk: ChatChunk;
}

// ----- Graph -----

export type CardKind =
  | "feature"
  | "refactor"
  | "bug_fix"
  | "research"
  | "test"
  | "docs"
  | "setup";

export type CardStatus =
  | "pending"
  | "ready"
  | "running"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "blocked";

export interface Card {
  id: Uuid;
  title: string;
  summary: string;
  kind: CardKind;
  status: CardStatus;
  assigned_agent: string | null;
  model: string | null;
  context: unknown;
  created_at: string;
  updated_at: string;
}

export type EdgeKind = "depends_on" | "discovered_by";

export interface Edge {
  from: Uuid;
  to: Uuid;
  kind: EdgeKind;
}

export interface GraphSnapshot {
  cards: Card[];
  edges: Edge[];
}

export type SchedulerEvent =
  | { type: "card_added"; card: Card }
  | { type: "card_updated"; card: Card }
  | { type: "card_removed"; id: Uuid }
  | { type: "dispatch_card"; card: Card }
  | { type: "card_cancel_requested"; id: Uuid }
  | { type: "graph_rescanned" };

export type CardOutputChunk =
  | { type: "bytes"; b64: string }
  | { type: "status"; text: string }
  | { type: "log"; text: string }
  | { type: "discovered"; title: string; summary: string };

export interface CardOutputEvent {
  card_id: Uuid;
  chunk: CardOutputChunk;
}

export type CardOutcome =
  | { Succeeded: { commit_oid: string | null } }
  | { Failed: { reason: string } }
  | "Cancelled";

export interface CardOutcomeEvent {
  card_id: Uuid;
  outcome: CardOutcome;
}

// ----- Spec -----

export interface SpecBreakdown {
  cards: Card[];
  /** [from_title, to_title] */
  deps: [string, string][];
  rationale: string | null;
}

// ----- PTY -----

export interface PtySessionInfo {
  id: Uuid;
  cwd: string;
  program: string;
  label: string;
  card_id: Uuid | null;
  created_at: string;
}

export interface PtyData {
  id: Uuid;
  b64: string;
}

export interface PtyExit {
  id: Uuid;
  status: number | null;
}

// ----- FS tree -----

export type TreeKind = "dir" | "file";
export interface TreeNode {
  name: string;
  path: string;
  kind: TreeKind;
  children: TreeNode[] | null;
}

// ----- Memory -----

export interface MemoryEntry {
  event_id: Uuid;
  kind: string;
  summary: string;
  actor: string;
  recorded_at: string;
  commit_oid: string;
}

// ----- Projects -----

export interface Project {
  id: Uuid;
  name: string;
  root: string;
  brain_dir: string;
  created_at: string;
  last_opened: string;
}

// ----- Settings -----

export type Theme = "system" | "dark" | "light" | "high_contrast";

export interface ClaudeSettings {
  binary_path: string | null;
  default_model: string;
  extra_args: string[];
}
export interface CodexSettings {
  binary_path: string | null;
  default_model: string;
  extra_args: string[];
}
export interface ShellSettings {
  default_shell: string;
  cols: number;
  rows: number;
}
export interface EditorSettings {
  font_family: string;
  font_size: number;
  tab_size: number;
  format_on_save: boolean;
  word_wrap: boolean;
}

export interface Settings {
  theme: Theme;
  default_chat_model: string;
  default_code_agent: string;
  default_project_path: string | null;
  brain_dir: string | null;
  telemetry_enabled: boolean;
  editor: EditorSettings;
  claude: ClaudeSettings;
  codex: CodexSettings;
  shell: ShellSettings;
}

// ----- Git -----

export type StageKind =
  | "none"
  | "added"
  | "modified"
  | "deleted"
  | "renamed"
  | "typechange";

export type WorkKind =
  | "none"
  | "modified"
  | "deleted"
  | "typechange"
  | "renamed"
  | "untracked"
  | "ignored";

export interface StatusEntry {
  path: string;
  stage: StageKind;
  work: WorkKind;
  conflict: boolean;
}

export interface HeadInfo {
  branch: string | null;
  detached: boolean;
  commit: string;
  ahead: number;
  behind: number;
  upstream: string | null;
}

export interface BranchInfo {
  name: string;
  is_head: boolean;
  is_remote: boolean;
}

export interface RemoteInfo {
  name: string;
  url: string | null;
}

export interface RepoStatus {
  root: string;
  head: HeadInfo | null;
  branches: BranchInfo[];
  files: StatusEntry[];
  remotes: RemoteInfo[];
}

export interface DiffLine {
  kind: " " | "+" | "-" | "B";
  content: string;
  old_lineno: number | null;
  new_lineno: number | null;
}

export interface DiffHunk {
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  header: string;
  lines: DiffLine[];
}

export interface FileDiff {
  path: string;
  binary: boolean;
  hunks: DiffHunk[];
}

export interface CommitInfo {
  oid: string;
  short_oid: string;
  summary: string;
  author: string;
  email: string;
  time: number;
}

// ----- MCP -----

export interface McpStatus {
  running: boolean;
  pid: number | null;
  brain_dir: string | null;
  binary: string;
}

export interface McpConfigSnippets {
  binary: string;
  brain_dir: string;
  claude_desktop: unknown;
  cursor: unknown;
  codex_toml: string;
}
