//! Durable task queue over the SQLite `downloads` table.
//!
//! Tasks persist across restarts: [`Queue::open`] requeues anything that was
//! `running`/`merging` when the process died, and [`Queue::claim_next`] hands
//! the oldest queued task to exactly one worker.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::open_database;
use crate::error::{SnapError, SnapResult};
use crate::models::{DownloadState, DownloadTask, MediaInfo, Stream};

const TASK_COLUMNS: &str =
    "id, url, title, thumbnail, state, progress, output_path, error, streams, chosen_streams";

/// Thread-safe queue over SQLite (single connection, mutex-guarded).
pub struct Queue {
    conn: Mutex<Connection>,
}

impl Queue {
    pub fn open(path: &Path) -> SnapResult<Self> {
        let conn = open_database(path)?;
        // Interrupted work is resumed, not shown as stuck forever.
        conn.execute(
            "UPDATE downloads SET state = 'queued' WHERE state IN ('running', 'merging')",
            [],
        )?;
        Ok(Queue {
            conn: Mutex::new(conn),
        })
    }

    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> SnapResult<T>) -> SnapResult<T> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| SnapError::Internal("db lock poisoned".into()))?;
        f(&conn)
    }

    /// Insert a task, or update it in place without losing its FIFO position.
    pub fn push(&self, task: &DownloadTask) -> SnapResult<()> {
        let streams = serde_json::to_string(&task.media_info.streams)?;
        let chosen = serde_json::to_string(&task.chosen_streams)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO downloads
                    (id, url, title, thumbnail, state, progress, output_path, created_at, error,
                     streams, chosen_streams)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(id) DO UPDATE SET
                    url = excluded.url,
                    title = excluded.title,
                    thumbnail = excluded.thumbnail,
                    state = excluded.state,
                    progress = excluded.progress,
                    output_path = excluded.output_path,
                    error = excluded.error,
                    streams = excluded.streams,
                    chosen_streams = excluded.chosen_streams",
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
                    streams,
                    chosen,
                ],
            )?;
            Ok(())
        })
    }

    /// Atomically claim the oldest queued task and mark it `running`.
    ///
    /// Returns `None` when nothing is queued. Concurrent workers each get a
    /// distinct task because the state change happens in the same statement.
    pub fn claim_next(&self) -> SnapResult<Option<DownloadTask>> {
        self.with_conn(|conn| {
            let sql = format!(
                "UPDATE downloads SET state = 'running'
                 WHERE id = (SELECT id FROM downloads WHERE state = 'queued'
                             ORDER BY created_at ASC, rowid ASC LIMIT 1)
                 RETURNING {TASK_COLUMNS}"
            );
            let task = conn.query_row(&sql, [], task_row_mapper).optional()?;
            Ok(task)
        })
    }

    /// Oldest queued task without claiming it (diagnostics/tests only).
    pub fn peek_next(&self) -> SnapResult<Option<DownloadTask>> {
        self.with_conn(|conn| {
            let sql = format!(
                "SELECT {TASK_COLUMNS} FROM downloads
                 WHERE state = 'queued'
                 ORDER BY created_at ASC, rowid ASC
                 LIMIT 1"
            );
            let task = conn.query_row(&sql, [], task_row_mapper).optional()?;
            Ok(task)
        })
    }

    /// Update mutable fields of an existing task.
    pub fn update(&self, task: &DownloadTask) -> SnapResult<()> {
        let changed = self.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE downloads
                    SET state = ?1, progress = ?2, output_path = ?3, error = ?4
                  WHERE id = ?5",
                params![
                    task.state.as_str(),
                    task.progress,
                    task.output_path,
                    task.error,
                    task.id,
                ],
            )?)
        })?;
        if changed == 0 {
            return Err(SnapError::Internal(format!("no such task: {}", task.id)));
        }
        Ok(())
    }

    /// All tasks, newest first.
    pub fn list(&self) -> SnapResult<Vec<DownloadTask>> {
        self.with_conn(|conn| {
            let sql = format!(
                "SELECT {TASK_COLUMNS} FROM downloads ORDER BY created_at DESC, rowid DESC"
            );
            let mut stmt = conn.prepare(&sql)?;
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
            let sql = format!(
                "SELECT {TASK_COLUMNS} FROM downloads WHERE state = ?1
                 ORDER BY created_at DESC, rowid DESC"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![state.as_str()], task_row_mapper)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    /// Delete a task by id (used when the user removes it from the UI).
    /// `segments` rows cascade via the foreign key.
    pub fn delete(&self, id: &str) -> SnapResult<()> {
        let changed = self.with_conn(|conn| {
            Ok(conn.execute("DELETE FROM downloads WHERE id = ?1", params![id])?)
        })?;
        if changed == 0 {
            return Err(SnapError::Internal(format!("no such task: {id}")));
        }
        Ok(())
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
    let streams_json: String = r.get(8)?;
    let chosen_json: String = r.get(9)?;

    let json_err = |index: usize, e: serde_json::Error| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(e))
    };
    let streams: Vec<Stream> = serde_json::from_str(&streams_json).map_err(|e| json_err(8, e))?;
    let chosen_streams: Vec<Stream> =
        serde_json::from_str(&chosen_json).map_err(|e| json_err(9, e))?;

    Ok(DownloadTask {
        id,
        media_info: MediaInfo {
            id: String::new(),
            title,
            uploader: String::new(),
            duration_s: None,
            thumbnail,
            source_url: url,
            streams,
        },
        chosen_streams,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::StreamKind;

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
                    kind: StreamKind::Muxed,
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

    fn temp_db() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "open-tube-queue-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("queue.db")
    }

    #[test]
    fn civil_dates() {
        assert_eq!(unix_to_iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(unix_to_iso8601(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(unix_to_iso8601(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn queue_roundtrip_keeps_every_field() {
        let path = temp_db();
        let queue = Queue::open(&path).unwrap();
        queue.push(&sample_task("t1")).unwrap();

        let claimed = queue.claim_next().unwrap().expect("should claim t1");
        assert_eq!(claimed.id, "t1");
        assert_eq!(claimed.media_info.title, "Sample");
        assert_eq!(claimed.state, DownloadState::Running);
        assert_eq!(claimed.media_info.streams.len(), 1, "streams must survive");

        // Claimed tasks are no longer queued.
        assert!(queue.claim_next().unwrap().is_none());

        let mut running = claimed.clone();
        running.state = DownloadState::Done;
        running.progress = 1.0;
        queue.update(&running).unwrap();

        let listed = queue.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].state, DownloadState::Done);
        assert_eq!(listed[0].progress, 1.0);

        queue.delete("t1").unwrap();
        assert!(queue.list().unwrap().is_empty());
    }

    #[test]
    fn claim_is_fifo_and_atomic() {
        let path = temp_db();
        let queue = Queue::open(&path).unwrap();
        queue.push(&sample_task("first")).unwrap();
        queue.push(&sample_task("second")).unwrap();

        let a = queue.claim_next().unwrap().expect("first");
        let b = queue.claim_next().unwrap().expect("second");
        assert_eq!(a.id, "first");
        assert_eq!(b.id, "second");
        assert!(queue.claim_next().unwrap().is_none());
    }

    #[test]
    fn reopen_requeues_interrupted_tasks() {
        let path = temp_db();
        {
            let queue = Queue::open(&path).unwrap();
            queue.push(&sample_task("t1")).unwrap();
            queue.claim_next().unwrap().expect("claimed");
        }
        let queue = Queue::open(&path).unwrap();
        let task = queue.claim_next().unwrap().expect("requeued after restart");
        assert_eq!(task.id, "t1");
    }

    #[test]
    fn upsert_keeps_position_and_updates_fields() {
        let path = temp_db();
        let queue = Queue::open(&path).unwrap();
        queue.push(&sample_task("old")).unwrap();
        queue.push(&sample_task("new")).unwrap();

        let mut old = sample_task("old");
        old.media_info.title = "Renamed".into();
        queue.push(&old).unwrap();

        assert_eq!(queue.peek_next().unwrap().unwrap().id, "old");
        assert_eq!(
            queue.peek_next().unwrap().unwrap().media_info.title,
            "Renamed"
        );
    }

    #[test]
    fn update_and_delete_report_missing_tasks() {
        let queue = Queue::open(&temp_db()).unwrap();
        let mut ghost = sample_task("ghost");
        ghost.state = DownloadState::Done;
        assert!(queue.update(&ghost).is_err());
        assert!(queue.delete("ghost").is_err());
    }
}
