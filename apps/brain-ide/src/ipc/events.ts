// Typed wrappers over Tauri's event channel. Components mount a
// subscription via `useTauriEvent` (see hooks/) which under the hood
// calls these.

import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  CardOutcomeEvent,
  CardOutputEvent,
  ChatStreamEvent,
  PtyData,
  PtyExit,
  SchedulerEvent,
  Uuid,
} from "./types";

export type Listener<T> = (payload: T) => void;

export async function subscribeChatStream(
  threadId: Uuid,
  fn: Listener<ChatStreamEvent>,
): Promise<UnlistenFn> {
  return listen<ChatStreamEvent>(`chat://${threadId}/chunk`, (ev) =>
    fn(ev.payload),
  );
}

export async function subscribeOrchestrator(
  fn: Listener<SchedulerEvent>,
): Promise<UnlistenFn> {
  return listen<SchedulerEvent>("orchestrator://event", (ev) =>
    fn(ev.payload),
  );
}

export async function subscribeCardOutput(
  fn: Listener<CardOutputEvent>,
): Promise<UnlistenFn> {
  return listen<CardOutputEvent>("card://output", (ev) => fn(ev.payload));
}

export async function subscribeCardOutcome(
  fn: Listener<CardOutcomeEvent>,
): Promise<UnlistenFn> {
  return listen<CardOutcomeEvent>("card://outcome", (ev) => fn(ev.payload));
}

export async function subscribePtyData(
  sessionId: Uuid,
  fn: Listener<PtyData>,
): Promise<UnlistenFn> {
  return listen<PtyData>(`pty://${sessionId}/data`, (ev) => fn(ev.payload));
}

export async function subscribePtyExit(
  sessionId: Uuid,
  fn: Listener<PtyExit>,
): Promise<UnlistenFn> {
  return listen<PtyExit>(`pty://${sessionId}/exit`, (ev) => fn(ev.payload));
}
