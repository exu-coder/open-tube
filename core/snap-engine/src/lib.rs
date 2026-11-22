//! Open tube download engine.
//!
//! Takes a [`MediaInfo`] + chosen streams, persists the task in the SQLite
//! queue, then downloads it: plain HTTP with streaming progress, or HLS with
//! playlist parsing, AES-128 decryption and segment concatenation.
//!
//! Progress is reported through a callback so the FFI layer can forward it to
//! Kotlin. Multiple streams are saved as separate files; container muxing
//! (video + audio into one file) is not implemented yet.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, RANGE};
use serde::Serialize;
use snap_core::{DownloadState, DownloadTask, MediaInfo, Queue, SnapError, SnapResult, Stream};
use tokio::io::AsyncWriteExt;
use url::Url;

/// Progress snapshot forwarded to the UI.
#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub task_id: String,
    pub state: DownloadState,
    pub progress: f64,
    pub speed_bps: f64,
    pub eta_s: u64,
    pub output_path: Option<String>,
    pub error: Option<String>,
}

/// Downloads queued tasks, at most one per `run_pending` call per task.
pub struct Engine {
    queue: Arc<Queue>,
    client: reqwest::Client,
    output_dir: PathBuf,
    cancelled: Arc<Mutex<HashSet<String>>>,
}

impl Engine {
    pub fn new(db_path: &Path, output_dir: &Path) -> SnapResult<Self> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("OpenTube/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| SnapError::Network(e.to_string()))?;
        std::fs::create_dir_all(output_dir)?;
        Ok(Self {
            queue: Arc::new(Queue::open(db_path)?),
            client,
            output_dir: output_dir.to_path_buf(),
            cancelled: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    /// Persist a new task and return it with its generated id.
    pub fn enqueue(&self, media: MediaInfo, chosen: Vec<Stream>) -> SnapResult<DownloadTask> {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| SnapError::Internal(e.to_string()))?
            .as_nanos();
        let task = DownloadTask {
            id: format!("dl-{n}"),
            media_info: media,
            chosen_streams: chosen,
            state: DownloadState::Queued,
            progress: 0.0,
            speed_bps: None,
            eta_s: None,
            output_path: None,
            error: None,
        };
        self.queue.push(&task)?;
        Ok(task)
    }

    pub fn list(&self) -> SnapResult<Vec<DownloadTask>> {
        self.queue.list()
    }

    /// Mark a task for cancellation. The engine checks this between chunks
    /// and between HLS segments.
    pub fn cancel(&self, id: &str) {
        self.cancelled
            .lock()
            .map_err(|_| ())
            .expect("cancel lock poisoned")
            .insert(id.to_string());
    }

    fn is_cancelled(&self, id: &str) -> bool {
        self.cancelled
            .lock()
            .map_err(|_| ())
            .expect("cancel lock poisoned")
            .contains(id)
    }

    /// Claim and run every queued task, reporting progress along the way.
    pub async fn run_pending<F>(&self, on_progress: F) -> SnapResult<()>
    where
        F: Fn(Progress) + Send + Sync,
    {
        while let Some(mut task) = self.queue.claim_next()? {
            if self.is_cancelled(&task.id) {
                task.state = DownloadState::Failed;
                task.error = Some("cancelled".into());
                let _ = self.queue.update(&task);
                self.cancelled.lock().expect("lock").remove(&task.id);
                continue;
            }

            let outcome = self.process(&task, &on_progress).await;
            match outcome {
                Ok(path) => {
                    task.state = DownloadState::Done;
                    task.progress = 1.0;
                    task.output_path = Some(path.clone());
                    task.error = None;
                    emit(&on_progress, &task, 1.0, 0.0, 0, Some(path), None);
                }
                Err(e) => {
                    let message = e.to_string();
                    tracing::warn!("task {} failed: {message}", task.id);
                    task.state = DownloadState::Failed;
                    task.error = Some(message.clone());
                    emit(
                        &on_progress,
                        &task,
                        task.progress,
                        0.0,
                        0,
                        None,
                        Some(message),
                    );
                }
            }
            let _ = self.queue.update(&task);
            self.cancelled.lock().expect("lock").remove(&task.id);
        }
        Ok(())
    }

    async fn process<F>(&self, task: &DownloadTask, on_progress: &F) -> SnapResult<String>
    where
        F: Fn(Progress) + Send + Sync,
    {
        let streams = if task.chosen_streams.is_empty() {
            &task.media_info.streams
        } else {
            &task.chosen_streams
        };
        if streams.is_empty() {
            return Err(SnapError::Parse("task has no streams to download".into()));
        }

        let base_name = safe_name(&task.media_info.title);
        let short_id = task.id.chars().rev().take(8).collect::<String>();
        let final_paths: Vec<PathBuf> = streams
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let ext = extension_of(s).unwrap_or_else(|| "bin".to_string());
                let name = if streams.len() == 1 {
                    format!("{base_name}-{short_id}.{ext}")
                } else {
                    format!("{base_name}-{short_id}-{i}.{ext}")
                };
                self.output_dir.join(name)
            })
            .collect();

        let known_total: u64 = streams.iter().filter_map(|s| s.size_bytes).sum();
        let mut downloaded_total = 0u64;
        let mut base = 0.0f64;

        for (i, stream) in streams.iter().enumerate() {
            let tmp = final_paths[i].with_extension(format!("part{i}"));
            let span = if known_total > 0 {
                stream.size_bytes.unwrap_or(0) as f64 / known_total as f64
            } else {
                1.0 / streams.len() as f64
            };
            let expected = if known_total > 0 {
                Some(stream.size_bytes.unwrap_or(0))
            } else {
                None
            };

            let result = if is_hls(&stream.url) {
                self.download_hls(stream, &tmp, &task.id, base, span, expected, on_progress)
                    .await
            } else {
                self.download_direct(stream, &tmp, &task.id, base, span, expected, on_progress)
                    .await
            };

            match result {
                Ok(bytes) => {
                    downloaded_total += bytes;
                    if self.is_cancelled(&task.id) {
                        let _ = tokio::fs::remove_file(&tmp).await;
                        return Err(SnapError::Internal("cancelled".into()));
                    }
                    tokio::fs::rename(&tmp, &final_paths[i]).await?;
                }
                Err(e) => {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    return Err(e);
                }
            }
            base += span;
        }

        // Avoid "unused" warnings while keeping a meaningful aggregate in logs.
        tracing::info!("task {} wrote {downloaded_total} bytes", task.id);
        Ok(final_paths[0].to_string_lossy().into_owned())
    }

    #[allow(clippy::too_many_arguments)]
    async fn download_direct<F>(
        &self,
        stream: &Stream,
        dest: &Path,
        task_id: &str,
        base: f64,
        span: f64,
        expected: Option<u64>,
        on_progress: &F,
    ) -> SnapResult<u64>
    where
        F: Fn(Progress) + Send + Sync,
    {
        let response = self
            .client
            .get(&stream.url)
            .headers(headers_of(stream))
            .send()
            .await
            .map_err(|e| network_error(e, &stream.url))?;
        let status = response.status();
        if !status.is_success() {
            return Err(SnapError::HttpStatus(status.as_u16()));
        }

        let total = expected.or_else(|| response.content_length());
        let mut file = tokio::fs::File::create(dest).await?;
        let mut chunks = response.bytes_stream();
        let started = Instant::now();
        let mut last_emit = Instant::now();
        let mut written = 0u64;
        let mut first_chunk = true;

        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(|e| SnapError::Network(e.to_string()))?;
            file.write_all(&chunk).await?;
            written += chunk.len() as u64;

            if self.is_cancelled(task_id) {
                file.flush().await?;
                return Err(SnapError::Internal("cancelled".into()));
            }
            if first_chunk || last_emit.elapsed() >= Duration::from_millis(200) {
                let (speed, eta) = speed_eta(written, total, started.elapsed());
                on_progress(progress(task_id, base, span, written, total, speed, eta));
                last_emit = Instant::now();
                first_chunk = false;
            }
        }
        file.flush().await?;
        Ok(written)
    }

    #[allow(clippy::too_many_arguments)]
    async fn download_hls<F>(
        &self,
        stream: &Stream,
        dest: &Path,
        task_id: &str,
        base: f64,
        span: f64,
        expected: Option<u64>,
        on_progress: &F,
    ) -> SnapResult<u64>
    where
        F: Fn(Progress) + Send + Sync,
    {
        let url =
            Url::parse(&stream.url).map_err(|e| SnapError::Parse(format!("bad HLS URL: {e}")))?;
        let playlist = self.fetch_text(&stream.url, &headers_of(stream)).await?;
        let playlist = snap_hls::playlist::parse(&playlist, &url)?;
        if playlist.segments.is_empty() {
            return Err(SnapError::Parse("HLS playlist has no segments".into()));
        }

        // EXT-X-MAP init section, prepended to the output for fMP4 streams.
        let init = match playlist.segments.first().and_then(|s| s.init.as_deref()) {
            Some(uri) => self.fetch_bytes(uri, &HeaderMap::new(), None).await?,
            None => Vec::new(),
        };

        // Fetch each distinct AES-128 key once.
        let mut keys: HashMap<String, Vec<u8>> = HashMap::new();
        for segment in &playlist.segments {
            if let Some(key) = &segment.key {
                if !keys.contains_key(&key.uri) {
                    let bytes = self.fetch_bytes(&key.uri, &HeaderMap::new(), None).await?;
                    if bytes.len() != 16 {
                        return Err(SnapError::Parse(format!(
                            "AES-128 key {} is {} bytes, expected 16",
                            key.uri,
                            bytes.len()
                        )));
                    }
                    keys.insert(key.uri.clone(), bytes);
                }
            }
        }

        let started = Instant::now();
        let mut file = tokio::fs::File::create(dest).await?;
        file.write_all(&init).await?;
        let mut written = init.len() as u64;
        let total = expected.map(|n| n.max(init.len() as u64));

        // Fetch up to 3 segments at a time, then write them in playlist order.
        let fetched = futures_util::stream::iter(playlist.segments.iter().enumerate())
            .map(|(index, segment)| {
                let client = &self.client;
                let cancelled = &self.cancelled;
                let task_id = task_id.to_string();
                let byte_range = segment.byte_range;
                async move {
                    if cancelled
                        .lock()
                        .map_err(|_| ())
                        .expect("lock")
                        .contains(&task_id)
                    {
                        return (index, Err(SnapError::Internal("cancelled".into())));
                    }
                    let mut request = client.get(&segment.uri);
                    // BYTERANGE without @offset is relative to the previous
                    // segment; we do not support that form, so fetch it whole.
                    if let Some(r) = byte_range.filter(|r| r.offset.is_some()) {
                        request = request.header(
                            RANGE,
                            range_header(Some((r.offset.expect("checked"), Some(r.length)))),
                        );
                    }
                    let bytes = match request.send().await {
                        Ok(response) if response.status().is_success() => {
                            match response.bytes().await {
                                Ok(b) => Ok(b.to_vec()),
                                Err(e) => Err(SnapError::Network(e.to_string())),
                            }
                        }
                        Ok(response) => Err(SnapError::HttpStatus(response.status().as_u16())),
                        Err(e) => Err(network_error(e, &segment.uri)),
                    };
                    (index, bytes)
                }
            })
            .buffer_unordered(3)
            .collect::<Vec<_>>()
            .await;

        let mut ordered: Vec<Option<Vec<u8>>> = vec![None; playlist.segments.len()];
        for (index, bytes) in fetched {
            ordered[index] = Some(bytes?);
        }

        for (segment, bytes) in playlist.segments.iter().zip(ordered) {
            let bytes = bytes.expect("all segments collected above");
            let plain = match &segment.key {
                Some(key) => {
                    let raw = keys
                        .get(&key.uri)
                        .ok_or_else(|| SnapError::Internal(format!("missing key {}", key.uri)))?;
                    snap_hls::aes::decrypt(
                        &bytes,
                        raw,
                        key.iv.as_ref().map(|iv| iv.as_slice()),
                        segment.sequence,
                    )
                    .map_err(SnapError::Parse)?
                }
                None => bytes,
            };
            file.write_all(&plain).await?;
            written += plain.len() as u64;

            if self.is_cancelled(task_id) {
                return Err(SnapError::Internal("cancelled".into()));
            }
            let (speed, eta) = speed_eta(written, total, started.elapsed());
            on_progress(progress(task_id, base, span, written, total, speed, eta));
        }
        file.flush().await?;
        Ok(written)
    }

    async fn fetch_text(&self, url: &str, headers: &HeaderMap) -> SnapResult<String> {
        let bytes = self.fetch_bytes(url, headers, None).await?;
        String::from_utf8(bytes).map_err(|e| SnapError::Parse(format!("invalid playlist: {e}")))
    }

    async fn fetch_bytes(
        &self,
        url: &str,
        headers: &HeaderMap,
        range: Option<(u64, Option<u64>)>,
    ) -> SnapResult<Vec<u8>> {
        let mut request = self.client.get(url).headers(headers.clone());
        if range.is_some() {
            request = request.header(RANGE, range_header(range));
        }
        let response = request.send().await.map_err(|e| network_error(e, url))?;
        if !response.status().is_success() {
            return Err(SnapError::HttpStatus(response.status().as_u16()));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| SnapError::Network(e.to_string()))?;
        Ok(bytes.to_vec())
    }
}

fn emit<F: Fn(Progress) + Send + Sync>(
    on_progress: &F,
    task: &DownloadTask,
    progress_value: f64,
    speed_bps: f64,
    eta_s: u64,
    output_path: Option<String>,
    error: Option<String>,
) {
    on_progress(Progress {
        task_id: task.id.clone(),
        state: task.state,
        progress: progress_value,
        speed_bps,
        eta_s,
        output_path,
        error,
    });
}

fn progress(
    task_id: &str,
    base: f64,
    span: f64,
    written: u64,
    total: Option<u64>,
    speed_bps: f64,
    eta_s: u64,
) -> Progress {
    let fraction = match total {
        Some(n) if n > 0 => (written as f64 / n as f64).clamp(0.0, 1.0),
        _ => 0.5,
    };
    Progress {
        task_id: task_id.to_string(),
        state: DownloadState::Running,
        progress: (base + span * fraction).clamp(0.0, 1.0),
        speed_bps,
        eta_s,
        output_path: None,
        error: None,
    }
}

fn speed_eta(written: u64, total: Option<u64>, elapsed: Duration) -> (f64, u64) {
    let secs = elapsed.as_secs_f64().max(f64::EPSILON);
    let speed = written as f64 / secs;
    let eta = match total {
        Some(n) if n > written && speed > 0.0 => ((n - written) as f64 / speed) as u64,
        _ => 0,
    };
    (speed, eta)
}

fn range_header(range: Option<(u64, Option<u64>)>) -> String {
    match range {
        Some((offset, Some(length))) => format!("bytes={offset}-{}", offset + length - 1),
        Some((offset, None)) => format!("bytes={offset}-"),
        None => "bytes=0-".to_string(),
    }
}

fn headers_of(stream: &Stream) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (key, value) in &stream.headers {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(key.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            headers.insert(name, value);
        }
    }
    headers
}

fn is_hls(url: &str) -> bool {
    url.split(['?', '#'])
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase()
        .ends_with(".m3u8")
}

fn extension_of(stream: &Stream) -> Option<String> {
    if let Some(container) = &stream.container {
        let ext = container.trim_start_matches('.').to_ascii_lowercase();
        if !ext.is_empty() {
            return Some(ext);
        }
    }
    let without_query = stream.url.split(['?', '#']).next().unwrap_or(&stream.url);
    without_query
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
}

fn safe_name(raw: &str) -> String {
    let name: String = raw
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    if name.is_empty() {
        "media".to_string()
    } else {
        name
    }
}

fn network_error(error: reqwest::Error, url: &str) -> SnapError {
    if error.is_connect() {
        SnapError::NoNetwork
    } else {
        SnapError::Network(format!("{url}: {error}"))
    }
}
