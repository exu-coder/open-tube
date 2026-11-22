//! Durable task queue over the SQLite `downloads` table.
//!
//! Tasks persist across restarts: after app relaunch, [`Queue::open`] and
//! [`Queue::pop_next`] yield the oldest queued task again.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::open_database;
use crate::error::{SnapError, SnapResult};
use crate::models::{DownloadState, DownloadTask, MediaInfo, Stream};

/// Thread-safe queue over SQLite (single connection, mutex-guarded).
pub struct Queue {
    conn: Mutex<Connection>,
}

impl Queue {
    pub fn open(path: &Path) -> SnapResult<Self> {
        let conn = open_database(path)?;
        Ok(Queue {
            conn: Mutex::new(conn),
        })
    }

    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> SnapResult<T>) -> SnapResult<T> {
        let conn = self.conn.lock().map_err(|_| SnapError::Internal("db lock poisoned".into()))?;
        f(&conn)
    }

    /// Push a new task into the queue (state: queued).
    pub fn push(&self, task: &DownloadTask) -> SnapResult<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO downloads
                    (id, url, title, thumbnail, state, progress, output_path, created_at, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    task.id,
                    task.media_info.source_url,
                    task.media_info.title,
                    task.media_info.thumbnail,
                    task.state.as_str(),
                    task.progress,
                    task.output_path,
                    now_iso8601(),
                    task.error,
                ],
            )?;
            Ok(())
        })
    }

    /// Oldest queued task (FIFO). Does not change state.
    pub fn pop_next(&self) -> SnapResult<Option<DownloadTask>> {
        self.with_conn(|conn| {
            let row: Option<(String, String, Option<String>, String, f64)> = conn
                .query_row(
                    "SELECT id, url, thumbnail, state, progress FROM downloads
                     WHERE state = 'queued'
                     ORDER BY created_at ASC, rowid ASC
                     LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .optional()?;
            Ok(row.map(|(id, url, thumbnail, _state, progress)| DownloadTask {
                id,
                media_info: MediaInfo {
                    id: String::new(),
                    title: String::new(),
                    uploader: String::new(),
                    duration_s: None,
                    thumbnail,
                    source_url: url,
                    streams: Vec::new(),
                },
                chosen_streams: Vec::new(),
                state: DownloadState::Queued,
                progress,
                speed_bps: None,
                eta_s: None,
                output_path: None,
                error: None,
            }))
        })
    }

    /// Update mutable fields of an existing task (state, progress, output_path, error).
    pub fn update(&self, task: &DownloadTask) -> SnapResult<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE downloads SET state = ?1, progress = ?2, output_path = ?3, error = ?4
                 WHERE id = ?5",
                params![
                    task.state.as_str(),
                    task.progress,
                    task.output_path,
                    task.error,
                    task.id,
                ],
            )?;
            Ok(())
        })
    }

    /// All tasks, newest first.
    pub fn list(&self) -> SnapResult<Vec<DownloadTask>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, url, title, thumbnail, state, progress, output_path, error
                 FROM downloads
                 ORDER BY created_at DESC, rowid DESC",
            )?;
            let rows = stmt.query_map([], task_row_mapper)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    /// Tasks filtered by state, newest first.
    pub fn list_state(&self, state: DownloadState) -> SnapResult<Vec<DownloadTask>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, url, title, thumbnail, state, progress, output_path, error
                 FROM downloads
                 WHERE state = ?1
                 ORDER BY created_at DESC, rowid DESC",
            )?;
            let rows = stmt.query_map(params![state.as_str()], task_row_mapper)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    /// Delete a task by id (used when the user removes it from the UI).
    pub fn delete(&self, id: &str) -> SnapResult<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM downloads WHERE id = ?1", params![id])?;
            Ok(())
        })
    }
}

fn task_row_mapper(r: &rusqlite::Row<'_>) -> rusqlite::Result<DownloadTask> {
    let id: String = r.get(0)?;
    let url: String = r.get(1)?;
    let title: String = r.get(2)?;
    let thumbnail: Option<String> = r.get(3)?;
    let state_s: String = r.get(4)?;
    let progress: f64 = r.get(5)?;
    let output_path: Option<String> = r.get(6)?;
    let error: Option<String> = r.get(7)?;
    Ok(DownloadTask {
        id,
        media_info: MediaInfo {
            id: String::new(),
            title,
            uploader: String::new(),
            duration_s: None,
            thumbnail,
            source_url: url,
            streams: Vec::new(),
        },
        chosen_streams: Vec::new(),
        state: DownloadState::from_str(&state_s).unwrap_or(DownloadState::Failed),
        progress,
        speed_bps: None,
        eta_s: None,
        output_path,
        error,
    })
}

/// Monotonic-enough creation timestamp, ISO-8601 UTC.
fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    unix_to_iso8601(secs)
}

fn unix_to_iso8601(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant's civil_from_days: days since 1970-01-01 -> (year, month, day).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (y + i64::from(m <= 2), m, d)
}

#[allow(dead_code)]
fn unused(_: Stream) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_task(id: &str) -> DownloadTask {
        DownloadTask {
            id: id.into(),
            media_info: MediaInfo {
                id: String::new(),
                title: "Sample".into(),
                uploader: "Uploader".into(),
                duration_s: Some(42.0),
                thumbnail: Some("https://example.com/thumb.jpg".into()),
                source_url: "https://example.com/video".into(),
                streams: vec![Stream {
                    url: "https://example.com/stream".into(),
                    kind: crate::models::StreamKind::Muxed,
                    container: Some("mp4".into()),
                    codec: None,
                    width: Some(1920),
                    height: Some(1080),
                    bitrate_bps: None,
                    size_bytes: None,
                    headers: Default::default(),
                }],
            },
            chosen_streams: Vec::new(),
            state: DownloadState::Queued,
            progress: 0.0,
            speed_bps: None,
            eta_s: None,
            output_path: None,
            error: None,
        }
    }

    #[test]
    fn civil_dates() {
        assert_eq!(unix_to_iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(unix_to_iso8601(1_000_000_000), "2001-09-09T01:46:40Z");
    }

    #[test]
    fn queue_roundtrip() {
        let dir = tempfile_dir();
        let queue = Queue::open(&dir).unwrap();
        let task = sample_task("t1");
        queue.push(&task).unwrap();
        let popped = queue.pop_next().unwrap().expect("should pop t1");
        assert_eq!(popped.id, "t1");
        assert_eq!(popped.media_info.title, "Sample");
        // Mark running.
        let mut running = popped.clone();
        running.state = DownloadState::Running;
        running.progress = 0.5;
        queue.update(&running).unwrap();
        // Queue should now be empty.
        assert!(queue.pop_next().unwrap().is_none());
        let listed = queue.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].state, DownloadState::Running);
        assert_eq!(listed[0].progress, 0.5);
        queue.delete("t1").unwrap();
        assert!(queue.list().unwrap().is_empty());
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "snaptube-queue-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("queue.db")
    }
}
