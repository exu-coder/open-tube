use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use snap_core::{DownloadState, MediaInfo, Stream, StreamKind};
use snap_engine::{Engine, Progress};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("open-tube-{tag}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

async fn spawn_server(routes: Vec<(&'static str, Vec<u8>)>) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            let mut buffer = [0u8; 4096];
            let read = socket.read(&mut buffer).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]);
            let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
            let body = routes
                .iter()
                .find(|(route, _)| *route == path.as_str())
                .map(|(_, bytes)| bytes.clone())
                .unwrap_or_default();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            if socket.write_all(head.as_bytes()).await.is_err() {
                continue;
            }
            let _ = socket.write_all(&body).await;
        }
    });
    (format!("http://{address}"), handle)
}

fn media(url: String, size: Option<u64>) -> MediaInfo {
    MediaInfo {
        id: "test".into(),
        title: "test media".into(),
        uploader: "test".into(),
        duration_s: None,
        thumbnail: None,
        source_url: url.clone(),
        streams: vec![Stream {
            url,
            kind: StreamKind::Muxed,
            container: Some("mp4".into()),
            codec: None,
            width: None,
            height: None,
            bitrate_bps: None,
            size_bytes: size,
            headers: Default::default(),
        }],
    }
}

#[tokio::test]
async fn downloads_direct_url_and_reports_progress() {
    let body = b"hello from the local server".to_vec();
    let (base, server) = spawn_server(vec![("/video.mp4", body.clone())]).await;
    let dir = temp_dir("engine-direct");
    let engine = Engine::new(&dir.join("queue.db"), &dir.join("out")).expect("engine");

    let task = engine
        .enqueue(
            media(format!("{base}/video.mp4"), Some(body.len() as u64)),
            vec![],
        )
        .expect("enqueue");

    let events: Arc<Mutex<Vec<Progress>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    engine
        .run_pending(move |event| sink.lock().expect("lock").push(event))
        .await
        .expect("run");
    server.abort();

    let stored = engine
        .list()
        .expect("list")
        .into_iter()
        .find(|t| t.id == task.id)
        .expect("stored task");
    assert_eq!(stored.state, DownloadState::Done);
    assert_eq!(stored.error, None);
    let path = stored.output_path.expect("output path");
    assert_eq!(std::fs::read(path).expect("output file"), body);

    let events = events.lock().expect("lock");
    assert!(
        events.iter().any(|e| e.state == DownloadState::Running),
        "at least one progress event must be emitted"
    );
    assert_eq!(
        events.last().expect("last event").state,
        DownloadState::Done
    );
}

#[tokio::test]
async fn downloads_and_concatenates_hls_segments() {
    let playlist = b"#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:4,\nseg0.ts\n#EXTINF:4,\nseg1.ts\n#EXT-X-ENDLIST\n".to_vec();
    let (base, server) = spawn_server(vec![
        ("/index.m3u8", playlist),
        ("/seg0.ts", b"aaaa".to_vec()),
        ("/seg1.ts", b"bbbb".to_vec()),
    ])
    .await;
    let dir = temp_dir("engine-hls");
    let engine = Engine::new(&dir.join("queue.db"), &dir.join("out")).expect("engine");

    let mut info = media(format!("{base}/index.m3u8"), None);
    info.streams[0].container = Some("m3u8".into());
    let task = engine.enqueue(info, vec![]).expect("enqueue");
    engine.run_pending(|_| {}).await.expect("run");
    server.abort();

    let stored = engine
        .list()
        .expect("list")
        .into_iter()
        .find(|t| t.id == task.id)
        .expect("stored task");
    assert_eq!(stored.state, DownloadState::Done, "{:?}", stored.error);
    let path = stored.output_path.expect("output path");
    assert_eq!(std::fs::read(path).expect("output file"), b"aaaabbbb");
}

#[tokio::test]
async fn cancelled_task_is_marked_failed_before_download() {
    let (_base, server) = spawn_server(vec![("/a.mp4", b"data".to_vec())]).await;
    let dir = temp_dir("engine-cancel");
    let engine = Engine::new(&dir.join("queue.db"), &dir.join("out")).expect("engine");
    let task = engine
        .enqueue(media("http://127.0.0.1:1/a.mp4".into(), None), vec![])
        .expect("enqueue");
    engine.cancel(&task.id);

    engine.run_pending(|_| {}).await.expect("run");
    server.abort();

    let stored = engine
        .list()
        .expect("list")
        .into_iter()
        .find(|t| t.id == task.id)
        .expect("stored task");
    assert_eq!(stored.state, DownloadState::Failed);
    assert_eq!(stored.error.as_deref(), Some("cancelled"));
}
