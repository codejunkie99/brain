import { useEffect, useState } from "react";

import { AppShell } from "./components/Layout/AppShell";
import { OnboardingFlow } from "./components/Onboarding/OnboardingFlow";
import {
  subscribeCardOutcome,
  subscribeCardOutput,
  subscribeOrchestrator,
} from "./ipc/events";
import { useAgentsStore } from "./state/agents";
import { useChatStore } from "./state/chat";
import { useGraphStore } from "./state/graph";
import { useProjectsStore } from "./state/projects";
import { useSettingsStore } from "./state/settings";
import { useTerminalsStore } from "./state/terminals";
import { applyTheme } from "./theme/themes";

export function App() {
  const [booted, setBooted] = useState(false);
  const settings = useSettingsStore((s) => s.settings);
  const loadSettings = useSettingsStore((s) => s.load);
  const loadProjects = useProjectsStore((s) => s.load);
  const projects = useProjectsStore((s) => s.projects);
  const loadAgents = useAgentsStore((s) => s.load);
  const loadChat = useChatStore((s) => s.load);
  const loadGraph = useGraphStore((s) => s.load);
  const loadTerminals = useTerminalsStore((s) => s.load);

  useEffect(() => {
    void (async () => {
      await Promise.all([
        loadSettings(),
        loadProjects(),
        loadAgents(),
        loadChat(),
        loadGraph(),
        loadTerminals(),
      ]);
      setBooted(true);
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (settings) applyTheme(settings.theme);
  }, [settings?.theme, settings]);

  // Global subscriptions — orchestrator events drive graph state and
  // card output streams.
  useEffect(() => {
    const promises: Promise<() => void>[] = [
      subscribeOrchestrator((ev) => useGraphStore.getState().applyEvent(ev)),
      subscribeCardOutput((ev) =>
        useGraphStore.getState().applyOutput(ev.card_id, ev.chunk),
      ),
      subscribeCardOutcome(() => {
        // Outcome events are informational here; the actual status
        // transition rides on the orchestrator's card_updated event.
      }),
    ];
    const unlisteners: (() => void)[] = [];
    Promise.all(promises).then((fns) => unlisteners.push(...fns));
    return () => {
      unlisteners.forEach((f) => f());
    };
  }, []);

  if (!booted || !settings) return <BootScreen />;

  // First-run onboarding: no projects, no API keys, no brain.
  const needsOnboarding = projects.length === 0;
  if (needsOnboarding) {
    return <OnboardingFlow onDone={() => loadProjects()} />;
  }

  return <AppShell />;
}

function BootScreen() {
  return (
    <div
      style={{
        flex: 1,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        color: "var(--fg-3)",
        fontSize: 13,
        letterSpacing: 0.3,
      }}
    >
      booting brain…
    </div>
  );
}
