//! SQLite schema for the download queue (PROJECT spec, `SQLITE SCHEMA`).

use std::path::Path;

use rusqlite::Connection;

use crate::error::SnapResult;

pub fn init_schema(conn: &Connection) -> SnapResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS downloads (
            id TEXT PRIMARY KEY,
            url TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            thumbnail TEXT,
            state TEXT NOT NULL DEFAULT 'queued',
            progress REAL NOT NULL DEFAULT 0.0,
            output_path TEXT,
            created_at TEXT NOT NULL,
            error TEXT
        );

        CREATE TABLE IF NOT EXISTS segments (
            download_id TEXT NOT NULL,
            \"index\" INTEGER NOT NULL,
            \"offset\" INTEGER NOT NULL,
            length INTEGER NOT NULL,
            done INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (download_id, \"index\")
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_downloads_state ON downloads(state, created_at);
        ",
    )?;
    Ok(())
}

pub fn open_database(path: &Path) -> SnapResult<Connection> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    init_schema(&conn)?;
    Ok(conn)
}
