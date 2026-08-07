import { useAgentsStore } from "@/state/agents";
import { useSettingsStore } from "@/state/settings";

import { Pill } from "../common/Pill";

export function AgentsPanel() {
  const descriptors = useAgentsStore((s) => s.descriptors);
  const settings = useSettingsStore((s) => s.settings);
  const update = useSettingsStore((s) => s.update);
  if (!settings) return null;

  return (
    <div style={{ maxWidth: 720 }}>
      <h2 style={{ fontSize: 16, margin: "0 0 16px" }}>Agents</h2>
      <p style={{ fontSize: 12, color: "var(--fg-2)", lineHeight: 1.6, marginBottom: 20 }}>
        The IDE talks to the Claude / Codex CLIs for card execution and to their APIs for chat.
        Configure binary paths and default models here.
      </p>

      <Section
        descriptor={descriptors.find((d) => d.slug === "claude" && d.kind === "cli")}
        chatDescriptor={descriptors.find((d) => d.slug === "claude" && d.kind === "api")}
        label="Claude"
      >
        <Field label="Binary path">
          <input
            value={settings.claude.binary_path ?? ""}
            placeholder="claude (resolved from $PATH)"
            onChange={(e) =>
              void update({
                claude: { ...settings.claude, binary_path: e.target.value || null },
              })
            }
            style={inputStyle}
          />
        </Field>
        <Field label="Default model">
          <input
            value={settings.claude.default_model}
            onChange={(e) =>
              void update({
                claude: { ...settings.claude, default_model: e.target.value },
              })
            }
            style={inputStyle}
          />
        </Field>
        <Field label="Extra args">
          <input
            value={settings.claude.extra_args.join(" ")}
            placeholder="--verbose --max-turns 5"
            onChange={(e) =>
              void update({
                claude: {
                  ...settings.claude,
                  extra_args: e.target.value.split(/\s+/).filter(Boolean),
                },
              })
            }
            style={inputStyle}
          />
        </Field>
      </Section>

      <Section
        descriptor={descriptors.find((d) => d.slug === "codex" && d.kind === "cli")}
        chatDescriptor={descriptors.find((d) => d.slug === "codex" && d.kind === "api")}
        label="Codex"
      >
        <Field label="Binary path">
          <input
            value={settings.codex.binary_path ?? ""}
            placeholder="codex (resolved from $PATH)"
            onChange={(e) =>
              void update({
                codex: { ...settings.codex, binary_path: e.target.value || null },
              })
            }
            style={inputStyle}
          />
        </Field>
        <Field label="Default model">
          <input
            value={settings.codex.default_model}
            onChange={(e) =>
              void update({
                codex: { ...settings.codex, default_model: e.target.value },
              })
            }
            style={inputStyle}
          />
        </Field>
        <Field label="Extra args">
          <input
            value={settings.codex.extra_args.join(" ")}
            onChange={(e) =>
              void update({
                codex: {
                  ...settings.codex,
                  extra_args: e.target.value.split(/\s+/).filter(Boolean),
                },
              })
            }
            style={inputStyle}
          />
        </Field>
      </Section>
    </div>
  );
}

function Section({
  label,
  descriptor,
  chatDescriptor,
  children,
}: {
  label: string;
  descriptor?: { ready: boolean; readiness_message: string | null };
  chatDescriptor?: { ready: boolean; readiness_message: string | null };
  children: React.ReactNode;
}) {
  return (
    <div
      style={{
        padding: 16,
        border: "1px solid var(--line-1)",
        borderRadius: 10,
        background: "var(--bg-1)",
        marginBottom: 12,
      }}
    >
      <div style={{ display: "flex", gap: 6, alignItems: "center", marginBottom: 12 }}>
        <strong style={{ fontSize: 13 }}>{label}</strong>
        <Pill
          color={descriptor?.ready ? "var(--green)" : "var(--orange)"}
          tone="soft"
          size="xs"
          title={descriptor?.readiness_message ?? undefined}
        >
          CLI {descriptor?.ready ? "ready" : "missing"}
        </Pill>
        <Pill
          color={chatDescriptor?.ready ? "var(--green)" : "var(--orange)"}
          tone="soft"
          size="xs"
          title={chatDescriptor?.readiness_message ?? undefined}
        >
          API {chatDescriptor?.ready ? "ready" : "key needed"}
        </Pill>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>{children}</div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
      <span style={{ width: 130, color: "var(--fg-2)", fontSize: 12 }}>{label}</span>
      {children}
    </div>
  );
}

const inputStyle: React.CSSProperties = {
  flex: 1,
  background: "var(--bg-2)",
  color: "var(--fg-0)",
  border: "1px solid var(--line-1)",
  borderRadius: 6,
  padding: "5px 10px",
  fontSize: 12,
  fontFamily: "var(--font-mono)",
};
