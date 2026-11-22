//! SQLite schema for the download queue (PROJECT spec, `SQLITE SCHEMA`).

use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use crate::error::{SnapError, SnapResult};

/// Bump when the schema changes; `init_schema` refuses newer databases.
pub const SCHEMA_VERSION: i64 = 1;

const CREATE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS downloads (
    id TEXT PRIMARY KEY,
    url TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    thumbnail TEXT,
    state TEXT NOT NULL DEFAULT 'queued'
        CHECK (state IN ('queued','running','paused','merging','done','failed')),
    progress REAL NOT NULL DEFAULT 0.0,
    output_path TEXT,
    created_at TEXT NOT NULL,
    error TEXT,
    streams TEXT NOT NULL DEFAULT '[]',
    chosen_streams TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE IF NOT EXISTS segments (
    download_id TEXT NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    "index" INTEGER NOT NULL,
    "offset" INTEGER NOT NULL,
    length INTEGER NOT NULL,
    done INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (download_id, "index")
);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_downloads_state ON downloads(state, created_at);
"#;

pub fn init_schema(conn: &Connection) -> SnapResult<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(SnapError::Internal(format!(
            "database schema v{version} is newer than supported v{SCHEMA_VERSION}"
        )));
    }
    if version < SCHEMA_VERSION {
        conn.execute_batch(CREATE_SQL)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    Ok(())
}

/// Open (or create) the queue database with the pragmas the download engine
/// relies on: WAL for concurrent reads, foreign keys for `segments` cascade,
/// a busy timeout so short write bursts retry instead of failing.
pub fn open_database(path: &Path) -> SnapResult<Connection> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    // WAL can be refused on some filesystems; the mode is read back so a
    // silent fallback is visible in logs instead of assumed.
    let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        tracing::warn!("sqlite journal_mode is {mode:?}, expected WAL");
    }
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    init_schema(&conn)?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "open-tube-db-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("queue.db")
    }

    #[test]
    fn creates_schema_and_is_idempotent() {
        let path = temp_path();
        let conn = open_database(&path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        // Re-opening must not fail or recreate anything.
        drop(conn);
        let conn = open_database(&path).unwrap();
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('downloads','segments','settings')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 3);
    }

    #[test]
    fn foreign_keys_cascade_segments() {
        let conn = open_database(&temp_path()).unwrap();
        conn.execute(
            "INSERT INTO downloads (id, url, created_at) VALUES ('d1', 'https://x/1', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO segments (download_id, \"index\", \"offset\", length) VALUES ('d1', 0, 0, 16)",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM downloads WHERE id = 'd1'", [])
            .unwrap();
        let left: i64 = conn
            .query_row("SELECT count(*) FROM segments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0, "segments must cascade");
    }

    #[test]
    fn rejects_invalid_state() {
        let conn = open_database(&temp_path()).unwrap();
        let err = conn.execute(
            "INSERT INTO downloads (id, url, created_at, state) VALUES ('d2', 'https://x/2', 'now', 'bogus')",
            [],
        );
        assert!(err.is_err(), "CHECK constraint must reject unknown states");
    }
}
