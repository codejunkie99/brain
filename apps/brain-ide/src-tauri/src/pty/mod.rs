//! PTY session manager.
//!
//! Each card optionally owns a PTY session; the IDE also lets the user
//! open free-floating shells in the bottom panel. We use `portable-pty`
//! (which delegates to forkpty on macOS/Linux and ConPTY on Windows) so
//! interactive tools — vim, lazygit, claude, codex — render exactly as
//! they would in Terminal.app.
//!
//! Output is fanned out to the frontend as `pty://<session_id>` events
//! carrying base64-decoded raw bytes. The frontend's xterm.js renders
//! them directly without re-interpreting ANSI; backend ships bytes
//! verbatim.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;

use parking_lot::Mutex;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    #[error("pty session not found: {0}")]
    UnknownSession(Uuid),
    #[error(transparent)]
    Pty(#[from] anyhow::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct PtySessionInfo {
    pub id: Uuid,
    pub cwd: String,
    pub program: String,
    pub label: String,
    pub card_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Event payload emitted as `pty://<id>/data`. Bytes are base64 so we
/// don't lose ANSI escapes through JSON.
#[derive(Debug, Clone, Serialize)]
pub struct PtyData {
    pub id: Uuid,
    /// base64-encoded raw bytes
    pub b64: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PtyExit {
    pub id: Uuid,
    pub status: Option<i32>,
}

struct Session {
    info: PtySessionInfo,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
}

#[derive(Clone)]
pub struct PtyManager {
    inner: Arc<Mutex<HashMap<Uuid, Session>>>,
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SpawnOptions {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
    pub label: String,
    pub card_id: Option<Uuid>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn list(&self) -> Vec<PtySessionInfo> {
        self.inner
            .lock()
            .values()
            .map(|s| s.info.clone())
            .collect()
    }

    pub fn spawn(&self, app: &AppHandle, opts: SpawnOptions) -> Result<PtySessionInfo, PtyError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: opts.rows,
                cols: opts.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Pty(e.into()))?;

        let mut cmd = CommandBuilder::new(&opts.program);
        for a in &opts.args {
            cmd.arg(a);
        }
        cmd.cwd(&opts.cwd);
        for (k, v) in &opts.env {
            cmd.env(k, v);
        }
        // Make sure xterm.js gets a sensible TERM
        cmd.env("TERM", "xterm-256color");
        // Surface that we're inside an IDE PTY so callees can tweak output
        cmd.env("BRAIN_IDE", "1");

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| PtyError::Pty(e.into()))?;
        drop(pair.slave);

        let id = Uuid::now_v7();
        let info = PtySessionInfo {
            id,
            cwd: opts.cwd,
            program: opts.program,
            label: opts.label,
            card_id: opts.card_id,
            created_at: chrono::Utc::now(),
        };

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::Pty(e.into()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| PtyError::Pty(e.into()))?;

        let session = Session {
            info: info.clone(),
            master: pair.master,
            writer,
        };
        self.inner.lock().insert(id, session);

        // Reader thread: pumps raw bytes to the frontend.
        {
            let app = app.clone();
            std::thread::Builder::new()
                .name(format!("pty-reader-{id}"))
                .spawn(move || {
                    use base64::Engine;
                    let mut buf = vec![0u8; 4096];
                    loop {
                        match reader.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                let b64 = base64::engine::general_purpose::STANDARD
                                    .encode(&buf[..n]);
                                let _ = app.emit(
                                    &format!("pty://{id}/data"),
                                    PtyData { id, b64 },
                                );
                            }
                            Err(_) => break,
                        }
                    }
                })
                .ok();
        }

        // Wait thread: emits exit status.
        {
            let app = app.clone();
            let manager = self.clone();
            std::thread::Builder::new()
                .name(format!("pty-waiter-{id}"))
                .spawn(move || {
                    let status = child.wait().ok().and_then(|s| s.exit_code().try_into().ok());
                    let _ = app.emit(&format!("pty://{id}/exit"), PtyExit { id, status });
                    manager.inner.lock().remove(&id);
                })
                .ok();
        }

        Ok(info)
    }

    pub fn write(&self, id: Uuid, data: &[u8]) -> Result<(), PtyError> {
        let mut guard = self.inner.lock();
        let session = guard.get_mut(&id).ok_or(PtyError::UnknownSession(id))?;
        session.writer.write_all(data)?;
        session.writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, id: Uuid, cols: u16, rows: u16) -> Result<(), PtyError> {
        let guard = self.inner.lock();
        let session = guard.get(&id).ok_or(PtyError::UnknownSession(id))?;
        session
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Pty(e.into()))?;
        Ok(())
    }

    pub fn close(&self, id: Uuid) -> Result<(), PtyError> {
        // Dropping the session closes the writer and master, which causes
        // the spawned child to receive SIGHUP.
        self.inner
            .lock()
            .remove(&id)
            .ok_or(PtyError::UnknownSession(id))
            .map(|_| ())
    }
}
