// Typed wrappers over Tauri's `invoke`. Every backend command lives
// here exactly once so the rest of the app can import from a single,
// rename-safe surface.

import { invoke } from "@tauri-apps/api/core";

import type {
  AgentDescriptor,
  Card,
  CredentialKind,
  CredentialStatus,
  GraphSnapshot,
  MemoryEntry,
  Project,
  PtySessionInfo,
  Settings,
  SpecBreakdown,
  ChatThread,
  TreeNode,
  Uuid,
} from "./types";

// ---------- Chat ----------

export const listThreads = () => invoke<ChatThread[]>("list_threads");
export const createThread = (title: string) =>
  invoke<ChatThread>("create_thread", { title });
export const deleteThread = (id: Uuid) =>
  invoke<void>("delete_thread", { id });
export const renameThread = (id: Uuid, title: string) =>
  invoke<ChatThread | null>("rename_thread", { id, title });

export interface SendMessageInput {
  thread_id: Uuid;
  agent: string;
  model?: string | null;
  text: string;
  system?: string | null;
}
export interface SendMessageOutput {
  thread_id: Uuid;
  user_message_id: Uuid;
  assistant_message_id: Uuid;
}
export const sendMessage = (input: SendMessageInput) =>
  invoke<SendMessageOutput>("send_message", { input });
export const cancelStream = (assistantMessageId: Uuid) =>
  invoke<void>("cancel_stream", { assistantMessageId });
export const listAgents = () => invoke<AgentDescriptor[]>("list_agents");

// ---------- Graph ----------

export const graphSnapshot = () => invoke<GraphSnapshot>("graph_snapshot");

export interface AddCardInput {
  title: string;
  summary: string;
  kind?: string;
  assigned_agent?: string | null;
  model?: string | null;
  depends_on?: Uuid[];
}
export const addCard = (input: AddCardInput) =>
  invoke<Card>("add_card", { input });
export const removeCard = (id: Uuid) => invoke<void>("remove_card", { id });

export interface AddEdgeInput {
  from: Uuid;
  to: Uuid;
  kind?: "depends_on" | "discovered_by";
}
export const addEdge = (input: AddEdgeInput) =>
  invoke<void>("add_edge", { input });

export interface AssignCardInput {
  id: Uuid;
  agent?: string | null;
  model?: string | null;
}
export const assignCard = (input: AssignCardInput) =>
  invoke<Card>("assign_card", { input });

export interface TransitionInput {
  id: Uuid;
  status: "running" | "succeeded" | "failed" | "cancelled";
}
export const transitionCard = (input: TransitionInput) =>
  invoke<void>("transition_card", { input });

export const resetGraph = () => invoke<void>("reset_graph");

// ---------- Spec ----------

export interface ParseSpecInput {
  spec: string;
  backend?: "heuristic" | string;
  model?: string | null;
}
export const parseSpec = (input: ParseSpecInput) =>
  invoke<SpecBreakdown>("parse_spec", { input });

export interface ApplyBreakdownInput {
  breakdown: SpecBreakdown;
  default_agent?: string | null;
  default_model?: string | null;
}
export const applyBreakdown = (input: ApplyBreakdownInput) =>
  invoke<string[]>("apply_breakdown", { input });

// ---------- PTY ----------

export interface PtySpawnInput {
  cwd?: string | null;
  program?: string | null;
  args?: string[];
  env?: [string, string][];
  cols?: number;
  rows?: number;
  label?: string | null;
  card_id?: Uuid | null;
}
export const ptySpawn = (input: PtySpawnInput) =>
  invoke<PtySessionInfo>("pty_spawn", { input });
export const ptyWrite = (id: Uuid, data: string, base64 = false) =>
  invoke<void>("pty_write", { input: { id, data, base64 } });
export const ptyResize = (id: Uuid, cols: number, rows: number) =>
  invoke<void>("pty_resize", { input: { id, cols, rows } });
export const ptyClose = (id: Uuid) => invoke<void>("pty_close", { id });
export const ptyList = () => invoke<PtySessionInfo[]>("pty_list");

// ---------- FS ----------

export interface ListTreeInput {
  root?: string | null;
  depth?: number;
  max_nodes?: number;
}
export const listTree = (input: ListTreeInput) =>
  invoke<TreeNode>("list_tree", { input });
export const readFile = (path: string, maxBytes?: number) =>
  invoke<{ content: string; truncated: boolean }>("read_file", {
    input: { path, max_bytes: maxBytes },
  });
export const writeFile = (path: string, content: string) =>
  invoke<void>("write_file", { input: { path, content } });

// ---------- Memory ----------

export const memoryNote = (text: string, actor = "user", idempotencyKey?: string) =>
  invoke<Uuid>("memory_note", {
    input: { text, actor, idempotency_key: idempotencyKey ?? null },
  });
export const memoryRecent = (limit = 50) =>
  invoke<MemoryEntry[]>("memory_recent", { input: { limit } });
export const memorySearch = (query: string, limit = 25) =>
  invoke<MemoryEntry[]>("memory_search", { input: { query, limit } });

// ---------- Projects / Settings / Auth ----------

export const listProjects = () => invoke<Project[]>("list_projects");
export const addProject = (input: {
  name: string;
  root: string;
  brain_dir?: string | null;
}) => invoke<Project>("add_project", { input });
export const setActiveProject = (id: Uuid) =>
  invoke<Project>("set_active_project", { id });
export const removeProject = (id: Uuid) =>
  invoke<void>("remove_project", { id });

export const getSettings = () => invoke<Settings>("get_settings");
export const updateSettings = (settings: Settings) =>
  invoke<Settings>("update_settings", { settings });

export const setCredential = (kind: CredentialKind, secret: string) =>
  invoke<void>("set_credential", { input: { kind, secret } });
export const clearCredential = (kind: CredentialKind) =>
  invoke<void>("clear_credential", { kind });
export const credentialStatus = () =>
  invoke<CredentialStatus[]>("credential_status");

export const openBrain = (path: string) =>
  invoke<string>("open_brain", { input: { path } });
