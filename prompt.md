PROJECT
Build "Open tube", an ad-free Android video and music downloader (SnapTube alternative). Everything runs on the device with no backend server, no ads, no analytics, no account. The core is written in Rust, and the UI is a thin Kotlin/Jetpack Compose layer. Distributed via GitHub Releases / F-Droid, not Google Play.

TECH STACK
- Rust (stable), cargo workspace, cargo-ndk, UniFFI for Kotlin bindings
- Rust crates: tokio, reqwest (rustls), serde, serde_json, rusqlite, m3u8-rs, aes + cbc, url, regex, tracing, thiserror, anyhow
- Kotlin, Jetpack Compose (Material 3), Hilt, Navigation Compose, Coil, Media3 (ExoPlayer), WorkManager, OkHttp
- Bundled: NewPipe Extractor (JitPack), yt-dlp + Python + FFmpeg (youtubedl-android library and its ffmpeg module)
- Android WebView (Chromium-based) for the in-app browser
- Min SDK 26, target latest SDK, ABI splits for arm64-v8a and armeabi-v7a, no universal APK
- jniLibs useLegacyPackaging = true so bundled binaries can run

WORKSPACE LAYOUT
/core
  snap-core   (models, Provider trait, queue, download engine, SQLite)
  snap-hls    (playlist parsing, segment download, AES decrypt, merge)
  snap-ffi    (UniFFI interface, callbacks, runtime setup)
/app (Kotlin)
  ui/ (home, search, browser, detail, downloads, library, player, settings)
  bridge/ (UniFFI glue, NewPipeProvider, YtDlpProvider, SniffBridge)
  service/ (DownloadService foreground service)
  update/ (UpdateManager, UpdateWorker, SignatureVerifier)
  storage/ (MediaStoreSaver)
  util/ (ClipboardWatcher, LinkExtractor)

INITIALIZATION (Application.onCreate)
YoutubeDL.getInstance().init(this); FFmpeg.getInstance().init(this); NewPipe.init(NewPipeDownloader(okHttpClient)); start the Rust runtime through UniFFI.

DATA MODELS (Rust, exposed via UniFFI)
MediaInfo { id, title, uploader, duration, thumbnail, source_url, streams: Vec<Stream> }
Stream { url, kind (Video | Audio | Muxed), container, codec, width, height, bitrate, size_bytes, headers: Map }
DownloadTask { id, media_info, chosen_streams, state (Queued | Running | Paused | Merging | Done | Failed), progress, speed_bps, eta_s, output_path, error }

PROVIDERS (one trait, resolved in order)
trait Provider { fn name(); fn can_handle(url) -> bool; async fn resolve(url) -> Result<MediaInfo>; }
1. NewPipeProvider (Kotlin, registered into Rust via a UniFFI callback interface): YouTube, YouTube Music, SoundCloud, Bandcamp, PeerTube, media.ccc.de, plus search, channels, playlists
2. YtDlpProvider: runs the bundled yt-dlp with --dump-json and parses the output into MediaInfo (covers the long tail of other sites)
3. SniffProvider: receives media URLs (mp4, webm, m4a, mp3, m3u8) with headers and page URL from the WebView and wraps them as MediaInfo
Fallback chain: NewPipe, then yt-dlp, then sniffed URLs. A failure in one provider falls through to the next.

DOWNLOAD ENGINE (Rust)
- reqwest streaming, HTTP Range resume, N-segment parallel download for large files
- Pause, resume, cancel, retry with exponential backoff, resume after network change
- Forward Referer, User-Agent, and Cookie headers on every request
- Queue with max 3 concurrent tasks, persisted in SQLite, restored after app restart
- HLS: parse master and media playlists, choose a variant by quality, download segments in parallel, AES-128 decrypt, handle fMP4 init segments, then concatenate or mux
- Separate audio and video streams merged through FFmpeg; MP3/M4A conversion and audio extraction through FFmpeg
- Progress, speed, and ETA reported to Kotlin through a UniFFI callback
- If a URL returns 403 or 410 (expired), re-resolve it through the provider chain and continue

KOTLIN UI AND FEATURES
1. Home: search bar, site shortcuts, recent downloads
2. Search: NewPipe search with paging, thumbnails, duration, channel
3. Detail + quality picker: video (144p to 4K), audio-only, size and format shown
4. In-app browser (Chromium WebView): tabs, address bar, back/forward/refresh, desktop-mode toggle, cookies and DOM storage on, fullscreen video support, host-based ad blocking, popup and redirect blocking, shouldInterceptRequest media detection, floating download button with a list of detected media and qualities
5. 18+ confirmation screen before the browser first opens
6. Clipboard auto-detect: on ON_RESUME read the clipboard (foreground only, Android 10+ rule), extract the first supported URL with LinkExtractor, ignore the last handled link, show a "Link detected, Download?" bottom sheet, with a settings toggle to disable
7. Share intent: ACTION_SEND text/plain, singleTop launch mode, handle onCreate and onNewIntent, go straight to the quality picker
8. Downloads screen: active, queued, completed, failed, with progress, speed, ETA, pause, resume, cancel
9. Library: finished files, play, share, rename, delete
10. Player: Media3, background audio, speed control, picture-in-picture
11. Settings: download folder, parallel downloads, Wi-Fi only, theme (light/dark/system), clipboard toggle, auto-update controls, clear history
12. DownloadService: foreground service that owns the Rust engine, with notifications (progress, pause, cancel)
13. Save finished files through MediaStore so they appear in gallery and music apps

UPDATE CORE (UpdateManager)
- Runs through WorkManager every 12 hours on Wi-Fi, and immediately when a provider fails with an "extractor outdated" or parsing error
- yt-dlp: use the library's update function or check the latest GitHub release, compare versions, download to a temp file, verify the SHA-256 checksum, replace atomically, keep the previous version for rollback
- NewPipe Extractor: a GitHub Action builds a plugin .dex/.jar from the latest TeamNewPipe/NewPipeExtractor release and publishes it with an Ed25519 signature; the app verifies the signature with an embedded public key, saves the file read-only in private storage, and loads it with DexClassLoader
- Health check after every update: resolve a known test video; on failure, roll back to the last-known-good version automatically
- The bundled versions of yt-dlp and NewPipe are the permanent fallback
- Settings: auto-update toggle, Wi-Fi only, "Update now" button, current versions shown, update log
- Never load unsigned or unverified code, use HTTPS only, no custom update server (GitHub Releases only)
- Prompt for a full APK update if the plugin API version changes

SQLITE SCHEMA
downloads(id, url, title, thumbnail, state, progress, output_path, created_at, error)
segments(download_id, index, offset, length, done)
settings(key, value)

ERROR HANDLING
- Typed errors in Rust (thiserror), shown as readable messages in Kotlin
- Handle: no network, 403/410, storage full, unsupported site, extractor outdated, update failure
- No panics across the FFI boundary

SIZE OPTIMIZATION
- R8 shrinking and resource shrinking, ABI splits
- Cargo release profile: opt-level = "z", lto = true, strip = true, panic = "abort"
- Target size: roughly 60 to 85 MB per device with everything bundled

TESTING
- Rust unit tests: HLS parsing, Range resume, queue persistence, header forwarding
- Kotlin tests: LinkExtractor, ClipboardWatcher, UpdateManager rollback, ViewModels
- Manual test checklist after every build stage

CONSTRAINTS
- No ads or tracking SDKs, no Google Play Services dependency
- No unsafe Rust outside the FFI layer
- Do not circumvent DRM-protected content
- Compile-ready code only, no placeholders or TODOs

BUILD ORDER (finish and test each step before moving on)
1. Workspace, cargo-ndk, UniFFI hello-world callable from Kotlin
2. Models and Provider trait
3. Bundled libraries init (NewPipe, yt-dlp, FFmpeg) and NewPipeProvider with the callback bridge
4. Download engine (HTTP, Range, resume)
5. SQLite queue and persistence
6. DownloadService, notifications, MediaStore saving
7. HLS module
8. YtDlpProvider
9. WebView browser and SniffProvider
10. Search, detail, quality picker, downloads UI
11. Clipboard auto-detect and Share intent
12. FFmpeg merge and convert, library, player, settings
13. UpdateManager, signing, GitHub Action, health check and rollback
After each step, list the files created and what to test.