import { useSettingsStore } from "@/state/settings";

export function GeneralPanel() {
  const settings = useSettingsStore((s) => s.settings);
  const update = useSettingsStore((s) => s.update);
  if (!settings) return null;
  return (
    <div style={{ maxWidth: 640, display: "flex", flexDirection: "column", gap: 16 }}>
      <Heading title="Appearance" />
      <Row label="Theme">
        <select
          value={settings.theme}
          onChange={(e) => void update({ theme: e.target.value as typeof settings.theme })}
          style={selectStyle}
        >
          <option value="system">Match system</option>
          <option value="dark">Dark</option>
          <option value="light">Light</option>
          <option value="high_contrast">High contrast</option>
        </select>
      </Row>
      <Heading title="Defaults" />
      <Row label="Default chat model">
        <input
          value={settings.default_chat_model}
          onChange={(e) => void update({ default_chat_model: e.target.value })}
          style={inputStyle}
        />
      </Row>
      <Row label="Default card agent">
        <input
          value={settings.default_code_agent}
          onChange={(e) => void update({ default_code_agent: e.target.value })}
          style={inputStyle}
        />
      </Row>
      <Heading title="Editor" />
      <Row label="Font size">
        <input
          type="number"
          min={10}
          max={20}
          value={settings.editor.font_size}
          onChange={(e) =>
            void update({
              editor: { ...settings.editor, font_size: Number(e.target.value) },
            })
          }
          style={inputStyle}
        />
      </Row>
      <Row label="Tab size">
        <input
          type="number"
          min={1}
          max={8}
          value={settings.editor.tab_size}
          onChange={(e) =>
            void update({
              editor: { ...settings.editor, tab_size: Number(e.target.value) },
            })
          }
          style={inputStyle}
        />
      </Row>
      <Row label="Format on save">
        <input
          type="checkbox"
          checked={settings.editor.format_on_save}
          onChange={(e) =>
            void update({
              editor: {
                ...settings.editor,
                format_on_save: e.target.checked,
              },
            })
          }
        />
      </Row>
      <Heading title="Shell" />
      <Row label="Default shell">
        <input
          value={settings.shell.default_shell}
          onChange={(e) =>
            void update({
              shell: { ...settings.shell, default_shell: e.target.value },
            })
          }
          style={inputStyle}
        />
      </Row>
    </div>
  );
}

function Heading({ title }: { title: string }) {
  return (
    <h2
      style={{
        fontSize: 11,
        letterSpacing: 0.5,
        textTransform: "uppercase",
        color: "var(--fg-3)",
        margin: "8px 0 4px",
        fontWeight: 600,
      }}
    >
      {title}
    </h2>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "6px 0",
      }}
    >
      <span style={{ width: 180, color: "var(--fg-2)", fontSize: 12 }}>{label}</span>
      <span>{children}</span>
    </div>
  );
}

const inputStyle: React.CSSProperties = {
  background: "var(--bg-2)",
  color: "var(--fg-0)",
  border: "1px solid var(--line-1)",
  borderRadius: 6,
  padding: "5px 10px",
  fontSize: 12,
  minWidth: 220,
};

const selectStyle = inputStyle;
