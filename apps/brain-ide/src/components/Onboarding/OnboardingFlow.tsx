// First-run onboarding. Three steps: pick project root, set API keys
// (optional), confirm.

import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useState } from "react";

import * as bridge from "@/ipc/bridge";
import { useAgentsStore } from "@/state/agents";

import { Button } from "../common/Button";
import { Icon } from "../common/Icon";

interface Props {
  onDone: () => void;
}

type Step = "intro" | "project" | "keys" | "done";

export function OnboardingFlow({ onDone }: Props) {
  const [step, setStep] = useState<Step>("intro");
  const [root, setRoot] = useState<string | null>(null);
  const [anthropic, setAnthropic] = useState("");
  const [openai, setOpenai] = useState("");

  return (
    <div
      style={{
        flex: 1,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        background: "var(--bg-0)",
        padding: 32,
      }}
    >
      <div
        style={{
          width: 520,
          background: "var(--bg-1)",
          border: "1px solid var(--line-1)",
          borderRadius: 14,
          padding: 28,
          boxShadow: "0 30px 80px rgba(0,0,0,0.35)",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 16 }}>
          <Icon.Brain size={22} />
          <h1 style={{ fontSize: 18, margin: 0 }}>Welcome to Brain IDE</h1>
        </div>

        {step === "intro" && (
          <Intro onNext={() => setStep("project")} />
        )}
        {step === "project" && (
          <PickProject
            root={root}
            onPick={async () => {
              const result = await openDialog({
                directory: true,
                multiple: false,
                title: "Pick a project root",
              });
              if (typeof result === "string") setRoot(result);
            }}
            onContinue={() => setStep("keys")}
            onSkip={() => setStep("keys")}
          />
        )}
        {step === "keys" && (
          <Keys
            anthropic={anthropic}
            openai={openai}
            setAnthropic={setAnthropic}
            setOpenai={setOpenai}
            onContinue={() => setStep("done")}
          />
        )}
        {step === "done" && (
          <Done
            onFinish={async () => {
              if (root) {
                const name = root.split("/").filter(Boolean).pop() ?? "project";
                await bridge.addProject({ name, root });
              }
              if (anthropic.trim()) {
                await bridge.setCredential("anthropic", anthropic.trim());
              }
              if (openai.trim()) {
                await bridge.setCredential("openai", openai.trim());
              }
              await useAgentsStore.getState().reload();
              onDone();
            }}
          />
        )}
      </div>
    </div>
  );
}

function Intro({ onNext }: { onNext: () => void }) {
  return (
    <div>
      <p style={{ color: "var(--fg-1)", lineHeight: 1.7 }}>
        Brain IDE is a multi-agent orchestrator. Drop a spec on the left,
        the IDE turns it into a graph of feature cards, you drag each one
        onto Claude, Codex, or a shell session. Every card shares one
        git-backed brain memory.
      </p>
      <ul
        style={{
          color: "var(--fg-2)",
          fontSize: 12,
          lineHeight: 1.8,
          paddingLeft: 18,
        }}
      >
        <li>Chat persists across model switches.</li>
        <li>Cards dispatch as soon as their dependencies succeed.</li>
        <li>Memory is local-first; nothing leaves your machine without you.</li>
      </ul>
      <div style={{ marginTop: 16, display: "flex", justifyContent: "flex-end" }}>
        <Button variant="primary" onClick={onNext}>
          Get started
        </Button>
      </div>
    </div>
  );
}

function PickProject({
  root,
  onPick,
  onContinue,
  onSkip,
}: {
  root: string | null;
  onPick: () => Promise<void>;
  onContinue: () => void;
  onSkip: () => void;
}) {
  return (
    <div>
      <Step n={1} of={3} title="Pick a project" />
      <p style={{ color: "var(--fg-2)", fontSize: 12, lineHeight: 1.6 }}>
        Point the IDE at a repo on disk. Cards execute with this as their
        cwd. The brain memory dir defaults to <code>{"<project>/.brain"}</code>.
      </p>
      <div
        style={{
          padding: 12,
          background: "var(--bg-2)",
          border: "1px dashed var(--line-2)",
          borderRadius: 8,
          marginTop: 12,
        }}
      >
        {root ? (
          <code style={{ fontSize: 12 }}>{root}</code>
        ) : (
          <span style={{ color: "var(--fg-3)", fontSize: 12 }}>
            No project picked.
          </span>
        )}
      </div>
      <div style={{ display: "flex", gap: 6, justifyContent: "space-between", marginTop: 16 }}>
        <Button variant="ghost" onClick={onSkip}>
          Skip
        </Button>
        <div style={{ display: "flex", gap: 6 }}>
          <Button onClick={() => void onPick()}>Choose folder</Button>
          <Button variant="primary" disabled={!root} onClick={onContinue}>
            Continue
          </Button>
        </div>
      </div>
    </div>
  );
}

function Keys({
  anthropic,
  openai,
  setAnthropic,
  setOpenai,
  onContinue,
}: {
  anthropic: string;
  openai: string;
  setAnthropic: (s: string) => void;
  setOpenai: (s: string) => void;
  onContinue: () => void;
}) {
  return (
    <div>
      <Step n={2} of={3} title="Add API keys (optional)" />
      <p style={{ color: "var(--fg-2)", fontSize: 12, lineHeight: 1.6 }}>
        Keys are stored in your macOS Keychain. You can also wire only
        the CLI binaries and skip this entirely.
      </p>
      <div style={{ display: "flex", flexDirection: "column", gap: 10, marginTop: 12 }}>
        <input
          type="password"
          placeholder="Anthropic API key (Claude)"
          value={anthropic}
          onChange={(e) => setAnthropic(e.target.value)}
          style={input}
        />
        <input
          type="password"
          placeholder="OpenAI API key (Codex)"
          value={openai}
          onChange={(e) => setOpenai(e.target.value)}
          style={input}
        />
      </div>
      <div style={{ display: "flex", justifyContent: "flex-end", marginTop: 16, gap: 6 }}>
        <Button variant="primary" onClick={onContinue}>
          Continue
        </Button>
      </div>
    </div>
  );
}

function Done({ onFinish }: { onFinish: () => Promise<void> }) {
  const [submitting, setSubmitting] = useState(false);
  return (
    <div>
      <Step n={3} of={3} title="Ready" />
      <p style={{ color: "var(--fg-2)", fontSize: 12, lineHeight: 1.6 }}>
        That's it. You can change everything from Settings later.
      </p>
      <div style={{ display: "flex", justifyContent: "flex-end", marginTop: 16 }}>
        <Button
          variant="primary"
          onClick={async () => {
            setSubmitting(true);
            try {
              await onFinish();
            } finally {
              setSubmitting(false);
            }
          }}
          disabled={submitting}
        >
          {submitting ? "…" : "Open IDE"}
        </Button>
      </div>
    </div>
  );
}

function Step({ n, of, title }: { n: number; of: number; title: string }) {
  return (
    <div style={{ marginBottom: 12 }}>
      <div
        style={{
          fontSize: 10,
          letterSpacing: 0.6,
          textTransform: "uppercase",
          color: "var(--fg-3)",
        }}
      >
        Step {n} of {of}
      </div>
      <h2 style={{ fontSize: 15, margin: "4px 0 0" }}>{title}</h2>
    </div>
  );
}

const input: React.CSSProperties = {
  background: "var(--bg-2)",
  color: "var(--fg-0)",
  border: "1px solid var(--line-1)",
  borderRadius: 6,
  padding: "7px 10px",
  fontSize: 12,
  fontFamily: "var(--font-mono)",
};
