import { create } from "zustand";

import * as bridge from "@/ipc/bridge";
import type { Settings } from "@/ipc/types";

interface SettingsState {
  settings: Settings | null;
  load: () => Promise<void>;
  update: (patch: Partial<Settings>) => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: null,
  load: async () => {
    const settings = await bridge.getSettings();
    set({ settings });
  },
  update: async (patch) => {
    const current = get().settings;
    if (!current) return;
    const merged = { ...current, ...patch };
    const updated = await bridge.updateSettings(merged);
    set({ settings: updated });
  },
}));
