import { create } from "zustand";

import * as bridge from "@/ipc/bridge";
import type { Project, Uuid } from "@/ipc/types";

interface ProjectsState {
  projects: Project[];
  activeId: Uuid | null;
  load: () => Promise<void>;
  add: (input: { name: string; root: string; brain_dir?: string }) => Promise<Project>;
  remove: (id: Uuid) => Promise<void>;
  setActive: (id: Uuid) => Promise<Project>;
}

export const useProjectsStore = create<ProjectsState>((set, get) => ({
  projects: [],
  activeId: null,
  load: async () => {
    const projects = await bridge.listProjects();
    // No backend API to read active without a side effect, but the
    // first project is "active" by the backend's convention until the
    // user picks another.
    set({
      projects,
      activeId: get().activeId ?? projects[0]?.id ?? null,
    });
  },
  add: async (input) => {
    const project = await bridge.addProject(input);
    set((s) => ({
      projects: [...s.projects, project],
      activeId: s.activeId ?? project.id,
    }));
    return project;
  },
  remove: async (id) => {
    await bridge.removeProject(id);
    set((s) => ({
      projects: s.projects.filter((p) => p.id !== id),
      activeId:
        s.activeId === id
          ? (s.projects.find((p) => p.id !== id)?.id ?? null)
          : s.activeId,
    }));
  },
  setActive: async (id) => {
    const project = await bridge.setActiveProject(id);
    set({ activeId: id });
    return project;
  },
}));
