//! MCP host. The IDE can spawn a `brain-mcp` subprocess pointed at
//! the active brain so external agents (Claude Code, Cursor, Codex)
//! read and write the same memory the IDE chat does.
//!
//! `brain-mcp` speaks MCP over stdio. We don't *consume* that here —
//! the IDE is already wired directly to `brain-store` in-process.
//! What this module gives the user is:
//!
//!   1. one click to start/stop the subprocess so they don't have to
//!      remember `brain serve --mcp` from the terminal
//!   2. the exact JSON snippet to drop into each agent's mcp config
//!      so future invocations attach to *this brain*

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use tokio::process::{Child, Command};

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("no brain dir configured")]
    NoBrain,
    #[error("brain-mcp binary not found on PATH (or override mcp.binary_path in settings)")]
    NotFound,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("server is not running")]
    NotRunning,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub brain_dir: Option<String>,
    pub binary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpConfigSnippets {
    pub binary: String,
    pub brain_dir: String,
    /// `~/.claude/mcp_servers.json` shape
    pub claude_desktop: serde_json::Value,
    /// `<project>/.cursor/mcp.json` shape
    pub cursor: serde_json::Value,
    /// `~/.codex/config.toml` shape
    pub codex_toml: String,
}

#[derive(Clone)]
pub struct McpHost {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    binary: String,
    brain_dir: Option<PathBuf>,
    child: Option<Child>,
}

impl McpHost {
    pub fn new(binary_override: Option<String>) -> Self {
        let binary = binary_override.unwrap_or_else(|| "brain".to_string());
        Self {
            inner: Arc::new(Mutex::new(Inner {
                binary,
                brain_dir: None,
                child: None,
            })),
        }
    }

    pub fn set_brain_dir(&self, dir: PathBuf) {
        self.inner.lock().brain_dir = Some(dir);
    }

    pub fn set_binary(&self, binary: String) {
        self.inner.lock().binary = binary;
    }

    pub fn status(&self) -> McpStatus {
        let inner = self.inner.lock();
        let running = inner.child.is_some();
        let pid = inner.child.as_ref().and_then(|c| c.id());
        McpStatus {
            running,
            pid,
            brain_dir: inner.brain_dir.as_ref().map(|p| p.display().to_string()),
            binary: inner.binary.clone(),
        }
    }

    pub async fn start(&self) -> Result<McpStatus, McpError> {
        // Resolve everything without holding the lock across .await.
        let (binary, brain_dir) = {
            let inner = self.inner.lock();
            if inner.child.is_some() {
                return Ok(self.status());
            }
            let dir = inner.brain_dir.clone().ok_or(McpError::NoBrain)?;
            (inner.binary.clone(), dir)
        };

        if !crate::agents::claude_which(&binary) {
            return Err(McpError::NotFound);
        }

        let mut cmd = Command::new(&binary);
        cmd.arg("serve")
            .arg("--mcp")
            .arg("--brain-dir")
            .arg(&brain_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let child = cmd.spawn()?;

        self.inner.lock().child = Some(child);
        Ok(self.status())
    }

    pub async fn stop(&self) -> Result<(), McpError> {
        let mut child = {
            let mut inner = self.inner.lock();
            inner.child.take().ok_or(McpError::NotRunning)?
        };
        let _ = child.kill().await;
        Ok(())
    }

    pub fn config_snippets(&self) -> Result<McpConfigSnippets, McpError> {
        let (binary, brain_dir) = {
            let inner = self.inner.lock();
            let dir = inner.brain_dir.clone().ok_or(McpError::NoBrain)?;
            (inner.binary.clone(), dir)
        };
        let brain_dir_str = brain_dir.display().to_string();

        // Build the canonical `brain serve --mcp --brain-dir <dir>`
        // invocation each agent should run. The IDE doesn't need to
        // actually be running for this to work: each external agent
        // will spawn its own brain-mcp subprocess against the same
        // git-backed repo, and writes serialize through brain-store's
        // append lock.
        let claude_desktop = serde_json::json!({
            "mcpServers": {
                "brain": {
                    "command": binary,
                    "args": ["serve", "--mcp", "--brain-dir", brain_dir_str.clone()],
                }
            }
        });
        let cursor = serde_json::json!({
            "mcpServers": {
                "brain": {
                    "command": binary,
                    "args": ["serve", "--mcp", "--brain-dir", brain_dir_str.clone()],
                }
            }
        });
        let codex_toml = format!(
            "[mcp_servers.brain]\ncommand = \"{binary}\"\nargs = [\"serve\", \"--mcp\", \"--brain-dir\", \"{brain_dir_str}\"]\n"
        );
        Ok(McpConfigSnippets {
            binary,
            brain_dir: brain_dir_str,
            claude_desktop,
            cursor,
            codex_toml,
        })
    }
}
