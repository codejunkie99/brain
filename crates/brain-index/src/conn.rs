//! Connection setup: pragmas, schema version gating.

use std::path::Path;

use rusqlite::Connection;

use crate::error::IndexError;
use crate::schema;

/// Open (or create) the index database at `path`. On a brand-new file this
/// runs the full DDL; on an existing file it just verifies `user_version`
/// matches the schema this binary knows.
///
/// PRAGMAs applied on every open:
///   - `journal_mode = WAL`  — readers never block writers; multiple
///     `brain log`/`ask` processes can read concurrently while a single
///     `brain note` is writing.
///   - `synchronous = NORMAL` — in WAL this is safe (no corruption, only
///     "last transaction lost on power cut") and 3–5x faster than FULL
///     for many small writes.
///   - `busy_timeout = 5000` — SQLite handles lock-contention retry
///     internally for up to 5s. Matches the write-path retry deadline in
///     brain-store.
///   - `foreign_keys = ON` — belt and braces; none declared yet.
///   - `temp_store = MEMORY` — small, so keep it off disk.
pub fn open_connection(path: &Path) -> Result<Connection, IndexError> {
    let is_new = !path.exists();

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
            // Tighten the .brain/ parent dir so index contents aren't
            // readable by other local users. Codex F2 round 5.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;

    // On first create, lock the SQLite file to owner read/write only.
    // Subsequent opens don't touch perms so user-set modes stick.
    if is_new {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
    }

    let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 0 {
        schema::apply(&conn)?;
    } else if version != schema::SCHEMA_VERSION {
        return Err(IndexError::SchemaMismatch {
            path: path.display().to_string(),
            on_disk: version,
            expected: schema::SCHEMA_VERSION,
        });
    }
    Ok(conn)
}
