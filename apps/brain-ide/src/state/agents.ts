// Agent registry. Drives the model picker, drag targets, and the
// agents settings panel.

import { create } from "zustand";

import * as bridge from "@/ipc/bridge";
import type { AgentDescriptor, CredentialStatus } from "@/ipc/types";

interface AgentsState {
  descriptors: AgentDescriptor[];
  credentials: CredentialStatus[];
  load: () => Promise<void>;
  reload: () => Promise<void>;
}

export const useAgentsStore = create<AgentsState>((set) => ({
  descriptors: [],
  credentials: [],
  load: async () => {
    const [descriptors, credentials] = await Promise.all([
      bridge.listAgents(),
      bridge.credentialStatus(),
    ]);
    set({ descriptors, credentials });
  },
  reload: async () => {
    const [descriptors, credentials] = await Promise.all([
      bridge.listAgents(),
      bridge.credentialStatus(),
    ]);
    set({ descriptors, credentials });
  },
}));
