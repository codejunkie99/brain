//! Persisted IDE settings. JSON file at
//! `~/Library/Application Support/brain-ide/settings.json` on macOS.
//!
//! Kept small and additive — every new field gets a `#[serde(default)]`
//! so older settings files keep loading.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default = "default_theme")]
    pub theme: Theme,
    #[serde(default = "default_chat_model")]
    pub default_chat_model: String,
    #[serde(default = "default_code_agent")]
    pub default_code_agent: String,
    #[serde(default)]
    pub default_project_path: Option<PathBuf>,
    #[serde(default = "default_brain_dir")]
    pub brain_dir: Option<PathBuf>,
    #[serde(default = "default_telemetry")]
    pub telemetry_enabled: bool,
    #[serde(default)]
    pub editor: EditorSettings,
    #[serde(default)]
    pub claude: ClaudeSettings,
    #[serde(default)]
    pub codex: CodexSettings,
    #[serde(default)]
    pub shell: ShellSettings,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
    HighContrast,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorSettings {
    pub font_family: String,
    pub font_size: u16,
    pub tab_size: u8,
    pub format_on_save: bool,
    pub word_wrap: bool,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_family: "SF Mono, Menlo, monospace".into(),
            font_size: 13,
            tab_size: 2,
            format_on_save: true,
            word_wrap: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeSettings {
    pub binary_path: Option<PathBuf>,
    pub default_model: String,
    /// Extra args forwarded to every invocation of the claude CLI.
    pub extra_args: Vec<String>,
}

impl Default for ClaudeSettings {
    fn default() -> Self {
        Self {
            binary_path: None,
            default_model: "claude-opus-4-7".into(),
            extra_args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSettings {
    pub binary_path: Option<PathBuf>,
    pub default_model: String,
    pub extra_args: Vec<String>,
}

impl Default for CodexSettings {
    fn default() -> Self {
        Self {
            binary_path: None,
            default_model: "gpt-5".into(),
            extra_args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSettings {
    pub default_shell: String,
    pub cols: u16,
    pub rows: u16,
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self {
            default_shell: default_shell_for_platform(),
            cols: 100,
            rows: 32,
        }
    }
}

fn default_shell_for_platform() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
}

fn default_theme() -> Theme {
    Theme::System
}

fn default_chat_model() -> String {
    "claude-opus-4-7".into()
}

fn default_code_agent() -> String {
    "claude".into()
}

fn default_telemetry() -> bool {
    false
}

fn default_brain_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".brain"))
}

/// Thread-safe handle around the in-memory settings. The IDE state owns
/// one of these; commands clone the inner `Settings` to read and call
/// `update` to write.
#[derive(Clone)]
pub struct SettingsStore {
    inner: Arc<RwLock<Settings>>,
    file: PathBuf,
}

impl SettingsStore {
    pub fn open_or_default(file: PathBuf) -> Self {
        let initial = std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            inner: Arc::new(RwLock::new(initial)),
            file,
        }
    }

    pub fn read(&self) -> Settings {
        self.inner.read().clone()
    }

    pub fn update(&self, mutate: impl FnOnce(&mut Settings)) -> anyhow::Result<Settings> {
        let updated = {
            let mut guard = self.inner.write();
            mutate(&mut guard);
            guard.clone()
        };
        self.persist(&updated)?;
        Ok(updated)
    }

    fn persist(&self, settings: &Settings) -> anyhow::Result<()> {
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(settings)?)?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }
}
