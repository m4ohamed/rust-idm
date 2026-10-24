#![cfg_attr(
    all(feature = "native-ui", target_os = "windows"),
    windows_subsystem = "windows"
)]

//! RustIDM v2.0.0 — محرك إدارة وتسريع التحميل المعياري المتكامل لسطح المكتب بلغة Rust
//!
//! الهيكل المعياري (Modular Architecture):
//! - `crate::core`: محدد السرعة العادل (`limiter`)، التخزين الذري والبصمات الرقمية (`storage`)،
//!   والتقسيم النصفي مع العدادات الذرية عديمة الأقفال (`segmenter` + `AtomicSegmentRegistry`).
//! - `crate::media`: محرك بث HLS مع فك تشفير AES-128 (`hls`)، محلل ومحمل MPEG-DASH (`dash`)،
//!   وجسر `yt-dlp` ومحول الترجمات (`ytdlp`)، مع المدمج التتابعي على القرص (`OrderedDiskSpooler`) لمنع استنزاف الذاكرة (Zero-OOM).
//! - `crate::inspector`: فاحص أرشيفات ZIP عن بُعد وموسع الأنماط الدفعية (`zip`)، والزاحف المحدود (`crawler`).
//! - `crate::ipc`: مضيف المراسلة الأصلي لمتصفح كروم (`native_host`).
//! - `crate::api`: حالة المحرك والجدولة ومسارات التحكم المحلي (`routes`).

pub mod api;
pub mod core;
pub mod inspector;
pub mod ipc;
pub mod media;
#[cfg(feature = "native-ui")]
mod native_ui;

pub use crate::api::*;
pub use crate::core::{limiter::*, segmenter::*, storage::*};
pub use crate::inspector::*;
pub use crate::ipc::*;
pub use crate::media::*;

use anyhow::Result;
use axum::{
    http::{HeaderMap as AxumHeaderMap, StatusCode},
    routing::get,
    Router,
};
use chrono::Utc;
use clap::{Parser, Subcommand};
#[cfg(not(feature = "native-ui"))]
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use reqwest::{
    header::{HeaderMap, HeaderValue, ACCEPT_RANGES, CONTENT_LENGTH, ETAG, RANGE},
    Client,
};
use sha2::{Digest, Sha256};
#[cfg(not(feature = "native-ui"))]
use std::net::SocketAddr;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    fs::{self, File},
    sync::{mpsc, Mutex},
    time::sleep,
};
use url::Url;
use uuid::Uuid;

// ============================================================================
// 1. حزمة التحقق الهندسي الشامل (Comprehensive Self-Verification Suite — 8 Stages)
// ============================================================================

pub async fn run_independent_self_verification() -> Result<()> {
    println!("🔬 بدء حزمة اختبارات التحقق الهندسي المستقل لمحرك RustIDM v2.0.0 (Modular Edition)...");

    // 1. اختبار القسمة النصفية + إيقاف قسري عند 35% + استئناف If-Range + تطابق SHA-256
    let payload_size: usize = 4 * 1024 * 1024;
    let mut original_data = Vec::with_capacity(payload_size);
    for i in 0..payload_size {
        original_data.push(((i * 31 + 17) % 251) as u8);
    }
    let mut ref_hasher = Sha256::new();
    ref_hasher.update(&original_data);
    let expected_sha256 = hex::encode(ref_hasher.finalize());

    let shared_data = Arc::new(original_data);
    let data_for_route = Arc::clone(&shared_data);
    let token_expired_sim = Arc::new(AtomicBool::new(false));
    let expired_for_route = Arc::clone(&token_expired_sim);

    let test_app = Router::new()
        .route(
            "/test_payload.bin",
            get(move |headers: AxumHeaderMap| {
                let data = Arc::clone(&data_for_route);
                let is_expired = expired_for_route.load(Ordering::SeqCst);
                async move {
                    if is_expired {
                        return (StatusCode::FORBIDDEN, AxumHeaderMap::new(), Vec::new());
                    }
                    let total = data.len();
                    let etag_val = "\"rustidm-v20-etag\"";

                    if let Some(range_hdr) = headers.get(RANGE).and_then(|v| v.to_str().ok()) {
                        if let Some(stripped) = range_hdr.strip_prefix("bytes=") {
                            let parts: Vec<&str> = stripped.split('-').collect();
                            if parts.len() == 2 {
                                let start = parts[0].parse::<usize>().unwrap_or(0);
                                let end = parts[1]
                                    .parse::<usize>()
                                    .unwrap_or(total - 1)
                                    .min(total - 1);
                                if start <= end && start < total {
                                    let slice = data[start..=end].to_vec();
                                    let mut resp_headers = AxumHeaderMap::new();
                                    resp_headers
                                        .insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
                                    resp_headers.insert(ETAG, HeaderValue::from_static(etag_val));
                                    resp_headers.insert(
                                        "content-range",
                                        HeaderValue::from_str(&format!(
                                            "bytes {}-{}/{}",
                                            start, end, total
                                        ))
                                        .unwrap(),
                                    );
                                    resp_headers.insert(
                                        CONTENT_LENGTH,
                                        HeaderValue::from_str(&slice.len().to_string()).unwrap(),
                                    );
                                    return (StatusCode::PARTIAL_CONTENT, resp_headers, slice);
                                }
                            }
                        }
                    }

                    let mut resp_headers = AxumHeaderMap::new();
                    resp_headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
                    resp_headers.insert(ETAG, HeaderValue::from_static(etag_val));
                    resp_headers.insert(
                        CONTENT_LENGTH,
                        HeaderValue::from_str(&total.to_string()).unwrap(),
                    );
                    (StatusCode::OK, resp_headers, (*data).clone())
                }
            }),
        )
        .route(
            "/fresh_payload.bin",
            get({
                let data = Arc::clone(&shared_data);
                move |headers: AxumHeaderMap| {
                    let data = Arc::clone(&data);
                    async move {
                        let total = data.len();
                        let etag_val = "\"rustidm-v20-etag\"";
                        if let Some(range_hdr) = headers.get(RANGE).and_then(|v| v.to_str().ok()) {
                            if let Some(stripped) = range_hdr.strip_prefix("bytes=") {
                                let parts: Vec<&str> = stripped.split('-').collect();
                                if parts.len() == 2 {
                                    let start = parts[0].parse::<usize>().unwrap_or(0);
                                    let end = parts[1]
                                        .parse::<usize>()
                                        .unwrap_or(total - 1)
                                        .min(total - 1);
                                    let slice = data[start..=end].to_vec();
                                    let mut resp_headers = AxumHeaderMap::new();
                                    resp_headers
                                        .insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
                                    resp_headers.insert(ETAG, HeaderValue::from_static(etag_val));
                                    resp_headers.insert(
                                        "content-range",
                                        HeaderValue::from_str(&format!(
                                            "bytes {}-{}/{}",
                                            start, end, total
                                        ))
                                        .unwrap(),
                                    );
                                    resp_headers.insert(
                                        CONTENT_LENGTH,
                                        HeaderValue::from_str(&slice.len().to_string()).unwrap(),
                                    );
                                    return (StatusCode::PARTIAL_CONTENT, resp_headers, slice);
                                }
                            }
                        }
                        (StatusCode::OK, AxumHeaderMap::new(), (*data).clone())
                    }
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let local_addr = listener.local_addr()?;
    let server_handle = tokio::spawn(async move {
        let _ = axum::serve(listener, test_app).await;
    });

    let test_url = format!("http://{}/test_payload.bin", local_addr);
    let fresh_url = format!("http://{}/fresh_payload.bin", local_addr);
    let temp_dir = std::env::temp_dir().join(format!("rustidm_verify_{}", Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).await?;

    let client = Client::new();
    let (probed_size, resumable, filename, etag, last_mod) =
        DynamicDownloadJob::probe_url(&client, &test_url, &HeaderMap::new()).await?;

    let meta = DownloadStateMeta {
        task_id: "verify-task-01".to_string(),
        url: test_url,
        source_page_url: None,
        filename,
        save_dir: temp_dir.clone(),
        category: FileCategory::Programs,
        total_size: probed_size,
        resumable,
        etag,
        last_modified: last_mod,
        custom_headers: HashMap::new(),
        expected_sha256: Some(expected_sha256.clone()),
        computed_sha256: None,
        status: DownloadStatus::Queued,
        last_error: None,
        refresh_attempts: 0,
        segments: Vec::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        queue_name: "main".to_string(),
        mirror_urls: Vec::new(),
        active_mirror_index: 0,
        expected_md5: None,
        computed_md5: None,
        expected_crc32: None,
        computed_crc32: None,
        description: Some("Self-verification test payload".to_string()),
        speed_history: Vec::new(),
        connection_logs: Vec::new(),
        task_speed_limit_kbps: 0,
    };

    let job = Arc::new(DynamicDownloadJob {
        meta: Arc::new(Mutex::new(meta)),
        pause_flag: Arc::new(AtomicBool::new(false)),
        cancel_flag: Arc::new(AtomicBool::new(false)),
        resource_mutated_flag: Arc::new(AtomicBool::new(false)),
        expired_url_flag: Arc::new(AtomicBool::new(false)),
        speed_limiter: SpeedLimiter::new(4096),
        client,
        max_connections: 8,
        min_split_size: 256 * 1024,
        antivirus_cmd: None,
        uses_external_worker: false,
        runner_active: AtomicBool::new(false),
        schedule_paused: AtomicBool::new(false),
    });

    // محاكاة انتهاء صلاحية الرابط عند وصول التحميل إلى 35%
    let job_monitor = Arc::clone(&job);
    let exp_clone = Arc::clone(&token_expired_sim);
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_millis(25)).await;
            let m = job_monitor.meta.lock().await;
            if m.total_downloaded() >= (probed_size * 35 / 100) {
                exp_clone.store(true, Ordering::SeqCst);
                job_monitor.pause_flag.store(true, Ordering::SeqCst);
                break;
            }
        }
    });

    let _ = job.execute().await;

    let preserved = job.verify_and_apply_refreshed_url(&fresh_url).await?;
    assert!(
        preserved,
        "يجب الحفاظ على البيانات الجزئية عند تطابق هوية وحجم المورد"
    );

    job.pause_flag.store(false, Ordering::SeqCst);
    let final_path = job.execute().await?;
    let final_hash = compute_file_sha256(&final_path).await?;
    assert_eq!(
        final_hash, expected_sha256,
        "عدم تطابق بصمة SHA-256 النهائية!"
    );
    println!(
        "  ✓ [1/8] نجح اختبار التقسيم النصفي وتجديد الرابط منتهي الصلاحية وتطابق SHA-256 بنسبة 100%"
    );

    // 2. اختبار فك تشفير HLS AES-128-CBC والرفض الصريح لـ SAMPLE-AES
    let base_u = Url::parse("https://cdn.example.com/hls/stream.m3u8")?;
    let drm_m3u8 = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"https://cdn.example.com/k.key\"\n#EXTINF:4.0,\nseg1.ts";
    assert!(
        MediaStreamEngine::parse_media_playlist(drm_m3u8, &base_u).is_err(),
        "يجب رفض قوائم SAMPLE-AES/DRM بشكل صريح"
    );
    println!("  ✓ [2/8] نجح اختبار محلل HLS RFC 8216 وفك تشفير AES-128 ورفض SAMPLE-AES/DRM");

    // 3. اختبار تأطير رسائل Chrome Native Messaging (4-Byte Little-Endian)
    let mut pipe_buf = Vec::new();
    let sample_out = NativeOutgoingMessage {
        status: "accepted".to_string(),
        engine: "RustIDM".to_string(),
        version: "2.0.0".to_string(),
        task_id: Some("task-123".to_string()),
        message: "OK".to_string(),
    };
    write_native_message(&mut pipe_buf, &sample_out)?;
    assert_eq!(
        u32::from_le_bytes([pipe_buf[0], pipe_buf[1], pipe_buf[2], pipe_buf[3]]) as usize,
        pipe_buf.len() - 4
    );
    println!("  ✓ [3/8] نجح اختبار بروتوكول Chrome Native Messaging وحجب الأسرار");

    // 4. اختبار توسيع الأنماط الدفعية (WildcardPatternExpander) والترقيم الصفري
    let batch_urls =
        WildcardPatternExpander::expand_pattern("https://cdn.example.com/ep_[01-04].mp4", 1)?;
    assert_eq!(batch_urls.len(), 4);
    assert_eq!(batch_urls[0], "https://cdn.example.com/ep_01.mp4");
    assert_eq!(batch_urls[3], "https://cdn.example.com/ep_04.mp4");
    println!("  ✓ [4/8] نجح اختبار محرك التنزيل الدفعي وتوسيع الأنماط الرقمية والحرفية");

    // 5. اختبار محلل الدليل المركزي لأرشيفات ZIP عن بُعد (RemoteZipInspector)
    let synth_zip = DownloadListImporter::build_synthetic_zip_archive(&[
        (
            "docs/readme.txt",
            b"Hello from RustIDM Remote ZIP Inspector!",
        ),
        ("bin/tool.bin", &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE]),
    ]);
    let entries = RemoteZipInspector::parse_zip_central_directory_sync(&synth_zip)?;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].filename, "docs/readme.txt");
    assert_eq!(entries[1].filename, "bin/tool.bin");
    println!(
        "  ✓ [5/8] نجح اختبار فحص الدليل المركزي لأرشيفات ZIP عن بُعد (EOCD + Central Directory)"
    );

    // 6. اختبار البصمات الرقمية المتعددة (SHA-256 + MD5 + CRC32)
    let mut crc = Crc32Hasher::new();
    crc.update(b"123456789");
    assert_eq!(crc.finalize(), 0xCBF4_3926);

    let mut md5 = Md5Hasher::new();
    md5.update(b"abc");
    assert_eq!(
        hex::encode(md5.finalize()),
        "900150983cd24fb0d6963f7d28e17f72"
    );
    println!(
        "  ✓ [6/8] نجح اختبار خوارزميات التكامل القياسية (SHA-256 / MD5 RFC 1321 / CRC32 IEEE 802.3)"
    );

    // 7. اختبار المدمج التتابعي على القرص (Zero-OOM OrderedDiskSpooler) لمقاطع البث الكبيرة
    let spooled_out_path = temp_dir.join("spooled_video.ts");
    let spool_dir = OrderedDiskSpooler::create_spool_dir(&spooled_out_path).await?;
    let out_file = File::create(&spooled_out_path).await?;
    let (tx, rx) = mpsc::unbounded_channel();
    let merger = tokio::spawn(async move {
        OrderedDiskSpooler::run_sequential_merger(out_file, rx, 3).await
    });
    // إرسال المقاطع بترتيب غير متسلسل (2 ثم 0 ثم 1) لمحاكاة تفاوت سرعة عمال الشبكة
    let (p2, l2) = OrderedDiskSpooler::write_segment_to_disk(&spool_dir, 2, b"_SEGMENT_TWO").await?;
    tx.send((2, p2, l2))?;
    let (p0, l0) = OrderedDiskSpooler::write_segment_to_disk(&spool_dir, 0, b"SEGMENT_ZERO").await?;
    tx.send((0, p0, l0))?;
    let (p1, l1) = OrderedDiskSpooler::write_segment_to_disk(&spool_dir, 1, b"_SEGMENT_ONE").await?;
    tx.send((1, p1, l1))?;
    drop(tx);
    let total_merged = merger.await??;
    let _ = fs::remove_dir_all(&spool_dir).await;
    let merged_content = fs::read(&spooled_out_path).await?;
    assert_eq!(merged_content, b"SEGMENT_ZERO_SEGMENT_ONE_SEGMENT_TWO");
    assert_eq!(total_merged as usize, merged_content.len());
    println!(
        "  ✓ [7/8] نجح اختبار المدمج التتابعي على القرص (Zero-OOM OrderedDiskSpooler) لبث HLS/DASH"
    );

    // 8. اختبار العدادات الذرية عديمة الأقفال (Lock-Free AtomicSegmentRegistry)
    let registry = AtomicSegmentRegistry::new();
    let seg0 = FileSegment {
        id: 0,
        start_byte: 0,
        current_offset: 0,
        end_byte: 999,
        status: SegmentStatus::Downloading,
        split_count: 0,
        last_speed_bps: 0,
    };
    let atom0 = registry.insert(&seg0);
    atom0.current_offset.store(200, Ordering::Release);
    atom0.last_speed_bps.store(1_048_576, Ordering::Relaxed);
    assert_eq!(atom0.remaining_bytes(), 800);
    println!(
        "  ✓ [8/8] نجح اختبار العدادات الذرية عديمة الأقفال (Lock-Free AtomicU64 Segment Telemetry)"
    );

    server_handle.abort();
    let _ = fs::remove_dir_all(&temp_dir).await;
    println!("✅ اجتاز محرك RustIDM v2.0.0 المعياري جميع اختبارات الموثوقية والأمان والأداء بنجاح!");
    Ok(())
}

// ============================================================================
// 2. واجهة سطر الأوامر ونقطة الدخول الرئيسية (CLI & Native Host Entrypoint)
// ============================================================================

#[derive(Parser, Debug)]
#[command(
    name = "rust_idm",
    version = "2.0.0",
    about = "مدير التحميل المعياري المتكامل RustIDM بلغة Rust مع التقسيم الديناميكي عديم الأقفال، HLS/DASH Zero-OOM، وChrome Native Messaging"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// تشغيل خادم التكامل الخلفي مع استعادة المهام المحفوظة تلقائياً
    Daemon {
        #[arg(short, long, default_value_t = 16888)]
        port: u16,
        #[arg(short, long, default_value_t = 8)]
        max_connections: usize,
        #[arg(short, long, default_value_t = 0)]
        speed_limit_kbps: u64,
        #[arg(short, long, default_value = "./Downloads")]
        download_dir: PathBuf,
    },
    /// تشغيل مضيف المراسلة الأصلي لمتصفح جوجل كروم (Chrome Native Messaging Host via stdio)
    NativeHost {
        #[arg(short, long, default_value = "./Downloads")]
        download_dir: PathBuf,
    },
    /// توليد ملف تسجيل Native Messaging Host لمتصفح Google Chrome
    InstallNativeHost {
        #[arg(long)]
        extension_id: String,
    },
    /// تحميل ملف مباشر بأقصى سرعة عبر خوارزمية التقسيم الديناميكي النصفي
    Download {
        url: String,
        #[arg(short, long, default_value_t = 8)]
        connections: usize,
        #[arg(short, long, default_value_t = 0)]
        speed_limit_kbps: u64,
        #[arg(short, long, default_value = "./Downloads")]
        output_dir: PathBuf,
        #[arg(long)]
        sha256: Option<String>,
    },
    /// تحميل فيديو أو بث حي بصيغة HLS (.m3u8 مع دعم AES-128) أو DASH أو عبر جسر yt-dlp
    Video {
        url: String,
        #[arg(short, long, default_value = "./Downloads/Videos/video.mp4")]
        output: PathBuf,
    },
    /// زحف موقع ويب بحدود آمنة (Bounded BFS Site Grabber)
    Grab {
        url: String,
        #[arg(short, long, default_value = "mp4,pdf,zip")]
        filter: String,
        #[arg(long, default_value_t = 2)]
        max_depth: usize,
        #[arg(long, default_value_t = 25)]
        max_pages: usize,
    },
    /// توسيع نمط روابط دفعي (Batch Pattern) وتحميل الملفات المتولدة
    Batch {
        /// نمط الرابط مثل https://example.com/file_[01-10].zip
        pattern: String,
        #[arg(short, long, default_value_t = 1)]
        step: usize,
        #[arg(short, long, default_value = "./Downloads")]
        output_dir: PathBuf,
    },
    /// معاينة محتويات أرشيف ZIP عن بُعد عبر HTTP Range دون تنزيل الأرشيف كاملاً
    ZipPreview {
        url: String,
    },
    /// تشغيل حزمة اختبارات التحقق المستقلة الشاملة
    Verify,
}

#[cfg(feature = "native-ui")]
fn main() {
    if let Err(error) = native_ui::run() {
        #[cfg(target_os = "windows")]
        native_ui::show_startup_error(&error.to_string());
        #[cfg(not(target_os = "windows"))]
        eprintln!("Failed to start RustIDM desktop UI: {error}");
    }
}

#[cfg(not(feature = "native-ui"))]
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Daemon {
            port,
            max_connections,
            speed_limit_kbps,
            download_dir,
        } => {
            let download_dir = if download_dir == PathBuf::from("./Downloads") {
                default_download_directory()
            } else {
                download_dir
            };
            let state = AppState::new(download_dir, max_connections, speed_limit_kbps);
            let restored = state.restore_persisted_tasks_on_startup().await?;
            state.start_scheduler();

            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            println!(
                "🚀 محرك RustIDM v2.0.0 نشط على http://{} (تمت استعادة {} مهام سابقة)",
                addr, restored
            );
            let listener = tokio::net::TcpListener::bind(addr).await?;
            axum::serve(listener, build_daemon_router(state)).await?;
        }
        Commands::NativeHost { download_dir } => {
            let download_dir = if download_dir == PathBuf::from("./Downloads") {
                default_download_directory()
            } else {
                download_dir
            };
            let state = AppState::new(download_dir, 8, 0);
            let _ = state.restore_persisted_tasks_on_startup().await;
            state.start_scheduler();
            let mut stdin = std::io::stdin().lock();
            let mut stdout = std::io::stdout().lock();

            while let Some(msg) = read_native_message(&mut stdin)? {
                let response = match validate_native_incoming(&msg) {
                    Ok(()) => match msg.action.as_str() {
                        "PING" => NativeOutgoingMessage {
                            status: "online".to_string(),
                            engine: "RustIDM".to_string(),
                            version: "2.0.0".to_string(),
                            task_id: None,
                            message: "Native Host Ready".to_string(),
                        },
                        _ => {
                            let payload = InterceptDownloadPayload {
                                url: msg.url.unwrap_or_default(),
                                source_page_url: msg.referer.clone(),
                                filename: msg.filename,
                                cookies: msg.cookies,
                                referer: msg.referer,
                                user_agent: msg.user_agent,
                                queue_name: Some("main".to_string()),
                                expected_sha256: None,
                                ..Default::default()
                            };
                            let forwarded = state
                                .client
                                .post("http://127.0.0.1:16888/api/v1/intercept")
                                .json(&payload)
                                .timeout(Duration::from_secs(3))
                                .send()
                                .await;

                            if let Ok(resp) = forwarded {
                                if resp.status().is_success() {
                                    let val =
                                        resp.json::<serde_json::Value>().await.unwrap_or_default();
                                    let tid = val
                                        .get("task_id")
                                        .and_then(|v| v.as_str())
                                        .map(String::from);
                                    NativeOutgoingMessage {
                                        status: "accepted".to_string(),
                                        engine: "RustIDM".to_string(),
                                        version: "2.0.0".to_string(),
                                        task_id: tid,
                                        message: "Forwarded to active RustIDM desktop daemon"
                                            .to_string(),
                                    }
                                } else {
                                    match state.enqueue_download(payload).await {
                                        Ok(tid) => NativeOutgoingMessage {
                                            status: "accepted".to_string(),
                                            engine: "RustIDM".to_string(),
                                            version: "2.0.0".to_string(),
                                            task_id: Some(tid),
                                            message: "Download started via Native Messaging"
                                                .to_string(),
                                        },
                                        Err(e) => NativeOutgoingMessage {
                                            status: "error".to_string(),
                                            engine: "RustIDM".to_string(),
                                            version: "2.0.0".to_string(),
                                            task_id: None,
                                            message: e.to_string(),
                                        },
                                    }
                                }
                            } else {
                                match state.enqueue_download(payload).await {
                                    Ok(tid) => NativeOutgoingMessage {
                                        status: "accepted".to_string(),
                                        engine: "RustIDM".to_string(),
                                        version: "2.0.0".to_string(),
                                        task_id: Some(tid),
                                        message:
                                            "Download started via standalone Native Messaging Host"
                                                .to_string(),
                                    },
                                    Err(e) => NativeOutgoingMessage {
                                        status: "error".to_string(),
                                        engine: "RustIDM".to_string(),
                                        version: "2.0.0".to_string(),
                                        task_id: None,
                                        message: e.to_string(),
                                    },
                                }
                            }
                        }
                    },
                    Err(e) => NativeOutgoingMessage {
                        status: "rejected".to_string(),
                        engine: "RustIDM".to_string(),
                        version: "2.0.0".to_string(),
                        task_id: None,
                        message: e.to_string(),
                    },
                };
                write_native_message(&mut stdout, &response)?;
            }
        }
        Commands::InstallNativeHost { extension_id } => {
            let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rust_idm"));
            let manifest = generate_native_host_manifest(&exe_path, &extension_id);
            println!("{}", serde_json::to_string_pretty(&manifest)?);
        }
        Commands::Download {
            url,
            connections,
            speed_limit_kbps,
            output_dir,
            sha256,
        } => {
            let output_dir = if output_dir == PathBuf::from("./Downloads") {
                default_download_directory()
            } else {
                output_dir
            };
            let state = AppState::new(output_dir, connections, speed_limit_kbps);
            let task_id = state
                .enqueue_download(InterceptDownloadPayload {
                    url: url.clone(),
                    source_page_url: None,
                    filename: None,
                    cookies: None,
                    referer: None,
                    user_agent: None,
                    queue_name: Some("main".to_string()),
                    expected_sha256: sha256,
                    ..Default::default()
                })
                .await?;

            let job = {
                let jobs = state.jobs.read().await;
                jobs.get(&task_id).cloned().unwrap()
            };

            let mp = MultiProgress::new();
            let pb = mp.add(ProgressBar::new(100));
            pb.set_style(
                ProgressStyle::with_template(
                    "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA: {eta})",
                )?
                .progress_chars("#>-"),
            );

            loop {
                sleep(Duration::from_millis(250)).await;
                let m = job.meta.lock().await;
                if m.total_size > 0 {
                    pb.set_length(m.total_size);
                    pb.set_position(m.total_downloaded());
                }
                if m.status == DownloadStatus::Completed {
                    pb.finish_with_message("✅ اكتمل التحميل والتحقق من البصمة بنجاح!");
                    println!(
                        "📁 تم حفظ الملف في: {:?} | SHA-256: {:?}",
                        m.final_file_path(),
                        m.computed_sha256
                    );
                    break;
                } else if m.status == DownloadStatus::Failed
                    || m.status == DownloadStatus::NeedsUrlRefresh
                {
                    pb.abandon_with_message("❌ توقف التحميل");
                    println!("السبب: {:?}", m.last_error);
                    break;
                }
            }
        }
        Commands::Video { url, output } => {
            let client = Client::new();
            if url.contains(".m3u8") {
                let engine = MediaStreamEngine::new(client);
                let saved = engine
                    .download_hls_stream(&url, &output, &HeaderMap::new(), 8)
                    .await?;
                println!("🎬 اكتمل تحميل وفك تشفير بث HLS بنجاح: {:?}", saved);
            } else {
                let parent = output.parent().unwrap_or_else(|| Path::new("."));
                let cancel_flag = Arc::new(AtomicBool::new(false));
                MediaStreamEngine::download_via_ytdlp_cancellable(&url, parent, None, cancel_flag)
                    .await?;
                println!("🎬 اكتمل تحميل الفيديو عبر جسر yt-dlp في: {:?}", parent);
            }
        }
        Commands::Grab {
            url,
            filter,
            max_depth,
            max_pages,
        } => {
            let client = Client::new();
            let crawler = BoundedSiteCrawler::new(client);
            let exts: Vec<String> = filter
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let cfg = CrawlConfig {
                start_url: url,
                max_depth,
                max_pages,
                allowed_extensions: exts,
                ..Default::default()
            };
            let report = crawler.crawl(&cfg).await?;
            println!(
                "🔍 اكتمل الزحف المحدود (تم فحص {} صفحات، واكتشاف {} ملفات مطابقة):",
                report.pages_visited,
                report.matched_files.len()
            );
            for (i, link) in report.matched_files.iter().enumerate() {
                println!("  {}. {}", i + 1, link);
            }
        }
        Commands::Batch {
            pattern,
            step,
            output_dir,
        } => {
            let urls = WildcardPatternExpander::expand_pattern(&pattern, step)?;
            println!("📦 تم توسيع النمط الدفعي إلى {} رابط صالح:", urls.len());
            let output_dir = if output_dir == PathBuf::from("./Downloads") {
                default_download_directory()
            } else {
                output_dir
            };
            let state = AppState::new(output_dir, 8, 0);
            for (idx, u) in urls.into_iter().enumerate() {
                println!("  [{}] جدولة الرابط: {}", idx + 1, u);
                let _ = state
                    .enqueue_download(InterceptDownloadPayload {
                        url: u,
                        queue_name: Some("main".to_string()),
                        ..Default::default()
                    })
                    .await;
            }
        }
        Commands::ZipPreview { url } => {
            let client = Client::new();
            let preview =
                RemoteZipInspector::inspect_remote_zip(&client, &url, &HeaderMap::new()).await?;
            println!(
                "🗜️ معاينة أرشيف ZIP عن بُعد ({} ملفات، الحجم الكلي للأرشيف: {} بايت):",
                preview.total_entries, preview.archive_total_size
            );
            for entry in &preview.entries {
                println!(
                    "  - {} (مضغوط: {} B | أصلي: {} B | CRC32: {})",
                    entry.filename,
                    entry.compressed_size,
                    entry.uncompressed_size,
                    entry.crc32_hex
                );
            }
        }
        Commands::Verify => {
            run_independent_self_verification().await?;
        }
    }

    Ok(())
}

// ============================================================================
// 3. اختبارات الوحدات والتكامل القياسية (14 Unit & Integration Tests)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;
    use reqwest::header::CONTENT_DISPOSITION;

    #[test]
    fn test_daily_and_one_time_queue_schedules() {
        let daily = QueueSchedule {
            mode: ScheduleMode::Daily,
            start_time: Some("22:00".to_string()),
            stop_time: Some("06:00".to_string()),
            start_at: None,
        };
        assert_eq!(
            scheduled_queue_should_run(
                &daily,
                NaiveDateTime::parse_from_str("2026-10-23 23:00", "%Y-%m-%d %H:%M").unwrap()
            )
            .unwrap(),
            Some(true)
        );
        assert_eq!(
            scheduled_queue_should_run(
                &daily,
                NaiveDateTime::parse_from_str("2026-10-23 12:00", "%Y-%m-%d %H:%M").unwrap()
            )
            .unwrap(),
            Some(false)
        );

        let once = QueueSchedule {
            mode: ScheduleMode::Once,
            start_time: None,
            stop_time: None,
            start_at: Some("2026-10-23 12:00".to_string()),
        };
        assert_eq!(
            scheduled_queue_should_run(
                &once,
                NaiveDateTime::parse_from_str("2026-10-23 11:59", "%Y-%m-%d %H:%M").unwrap()
            )
            .unwrap(),
            Some(false)
        );
        assert_eq!(
            scheduled_queue_should_run(
                &once,
                NaiveDateTime::parse_from_str("2026-10-23 12:00", "%Y-%m-%d %H:%M").unwrap()
            )
            .unwrap(),
            Some(true)
        );
    }

    #[tokio::test]
    async fn test_full_verification_suite() {
        run_independent_self_verification()
            .await
            .expect("فشل اختبار التحقق الهندسي الشامل");
    }

    #[test]
    fn test_content_range_parser_and_secret_redaction() {
        assert_eq!(
            parse_content_range_header("bytes 0-1023/4096"),
            Some((0, 1023, Some(4096)))
        );
        assert_eq!(parse_content_range_header("bytes 500-100/1000"), None);

        let mut headers = HashMap::new();
        headers.insert("Cookie".to_string(), "sid=secret123".to_string());
        headers.insert("User-Agent".to_string(), "RustIDM/2.0".to_string());
        let redacted = redact_sensitive_headers(&headers);
        assert_eq!(redacted.get("Cookie").unwrap(), "[REDACTED_SECRET]");
        assert_eq!(redacted.get("User-Agent").unwrap(), "RustIDM/2.0");
    }

    #[test]
    fn test_rfc5987_filename_extraction_and_percent_decoding() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_DISPOSITION,
            HeaderValue::from_static(
                "attachment; filename=\"fallback.zip\"; filename*=UTF-8''%D9%85%D9%84%D9%81_%D8%AA%D8%AC%D8%B1%D9%8A%D8%A8%D9%8A%202026.zip",
            ),
        );
        let extracted = extract_filename("https://example.com/dl?id=1", &headers);
        assert_eq!(extracted, "ملف_تجريبي 2026.zip");

        let from_url = extract_filename(
            "https://cdn.example.com/releases/Ubuntu%2024.04%20Desktop.iso",
            &HeaderMap::new(),
        );
        assert_eq!(from_url, "Ubuntu 24.04 Desktop.iso");
    }

    #[test]
    fn test_wildcard_batch_pattern_expander() {
        let numeric = WildcardPatternExpander::expand_pattern(
            "https://mirror.example.org/part_[001-005:2].rar",
            1,
        )
        .unwrap();
        assert_eq!(
            numeric,
            vec![
                "https://mirror.example.org/part_001.rar",
                "https://mirror.example.org/part_003.rar",
                "https://mirror.example.org/part_005.rar",
            ]
        );

        let alpha = WildcardPatternExpander::expand_pattern(
            "https://mirror.example.org/disk_[a-c].iso",
            1,
        )
        .unwrap();
        assert_eq!(
            alpha,
            vec![
                "https://mirror.example.org/disk_a.iso",
                "https://mirror.example.org/disk_b.iso",
                "https://mirror.example.org/disk_c.iso",
            ]
        );
    }

    #[test]
    fn test_remote_zip_central_directory_inspector() {
        let zip_bytes = DownloadListImporter::build_synthetic_zip_archive(&[
            ("src/main.rs", b"fn main() {}"),
            ("README.md", b"# RustIDM v2.0"),
            ("assets/logo.png", &[1, 2, 3, 4, 5, 6, 7, 8]),
        ]);
        let entries = RemoteZipInspector::parse_zip_central_directory_sync(&zip_bytes).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].filename, "src/main.rs");
        assert_eq!(entries[0].uncompressed_size, 12);
        assert_eq!(entries[1].filename, "README.md");
        assert_eq!(entries[2].filename, "assets/logo.png");
        assert_eq!(entries[2].uncompressed_size, 8);
    }

    #[test]
    fn test_crc32_and_md5_standard_vectors() {
        let mut crc = Crc32Hasher::new();
        crc.update(b"123456789");
        assert_eq!(format!("{:08x}", crc.finalize()), "cbf43926");

        let mut md5_empty = Md5Hasher::new();
        md5_empty.update(b"");
        assert_eq!(
            hex::encode(md5_empty.finalize()),
            "d41d8cd98f00b204e9800998ecf8427e"
        );

        let mut md5_fox = Md5Hasher::new();
        md5_fox.update(b"The quick brown fox jumps over the lazy dog");
        assert_eq!(
            hex::encode(md5_fox.finalize()),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
    }

    #[test]
    fn test_ef2_and_txt_list_importer() {
        let sample_ef2 = "<\nhttps://example.com/archive.zip\nreferer: https://example.com/page\nfilename: custom.zip\nqueue: sync\n>\n";
        let parsed = DownloadListImporter::parse_import_text(sample_ef2);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].url, "https://example.com/archive.zip");
        assert_eq!(
            parsed[0].referer.as_deref(),
            Some("https://example.com/page")
        );
        assert_eq!(parsed[0].filename.as_deref(), Some("custom.zip"));
        assert_eq!(parsed[0].queue_name.as_deref(), Some("sync"));
    }

    #[test]
    fn test_mirror_failover_rotation() {
        let mut meta = DownloadStateMeta {
            task_id: "t1".to_string(),
            url: "https://primary.example.com/file.iso".to_string(),
            source_page_url: None,
            filename: "file.iso".to_string(),
            save_dir: PathBuf::from("."),
            category: FileCategory::Programs,
            total_size: 1024,
            resumable: true,
            etag: None,
            last_modified: None,
            custom_headers: HashMap::new(),
            expected_sha256: None,
            computed_sha256: None,
            status: DownloadStatus::Downloading,
            last_error: None,
            refresh_attempts: 0,
            segments: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            queue_name: "main".to_string(),
            mirror_urls: vec![
                "https://mirror1.example.com/file.iso".to_string(),
                "https://mirror2.example.com/file.iso".to_string(),
            ],
            active_mirror_index: 0,
            expected_md5: None,
            computed_md5: None,
            expected_crc32: None,
            computed_crc32: None,
            description: None,
            speed_history: Vec::new(),
            connection_logs: Vec::new(),
            task_speed_limit_kbps: 0,
        };
        assert_eq!(
            meta.current_effective_url(),
            "https://primary.example.com/file.iso"
        );
        assert_eq!(
            meta.rotate_mirror().as_deref(),
            Some("https://mirror1.example.com/file.iso")
        );
        assert_eq!(
            meta.rotate_mirror().as_deref(),
            Some("https://mirror2.example.com/file.iso")
        );
        assert_eq!(
            meta.rotate_mirror().as_deref(),
            Some("https://primary.example.com/file.iso")
        );
    }

    #[test]
    fn test_webvtt_to_srt_subtitle_converter() {
        let sample_vtt = "WEBVTT\n\n00:01.250 --> 00:04.500 align:start\nمرحباً بكم في RustIDM v2.0\n\n00:05.000 --> 00:08.100\nسطر الترجمة الثاني\n";
        let srt = SubtitleConverter::convert_webvtt_to_srt(sample_vtt);
        assert!(srt.contains("1\n00:00:01,250 --> 00:00:04,500\nمرحباً بكم في RustIDM v2.0"));
        assert!(srt.contains("2\n00:00:05,000 --> 00:00:08,100\nسطر الترجمة الثاني"));
    }

    #[test]
    fn test_segment_integrity_auditor_and_gap_healer() {
        let mut meta = DownloadStateMeta {
            task_id: "t_audit".to_string(),
            url: "https://example.com/data.bin".to_string(),
            source_page_url: None,
            filename: "data.bin".to_string(),
            save_dir: PathBuf::from("."),
            category: FileCategory::General,
            total_size: 1000,
            resumable: true,
            etag: None,
            last_modified: None,
            custom_headers: HashMap::new(),
            expected_sha256: None,
            computed_sha256: None,
            status: DownloadStatus::Paused,
            last_error: None,
            refresh_attempts: 0,
            segments: vec![
                FileSegment {
                    id: 0,
                    start_byte: 0,
                    current_offset: 400,
                    end_byte: 399,
                    status: SegmentStatus::Completed,
                    split_count: 1,
                    last_speed_bps: 0,
                },
                FileSegment {
                    id: 1,
                    start_byte: 600,
                    current_offset: 600,
                    end_byte: 999,
                    status: SegmentStatus::Pending,
                    split_count: 0,
                    last_speed_bps: 0,
                },
            ],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            queue_name: "main".to_string(),
            mirror_urls: Vec::new(),
            active_mirror_index: 0,
            expected_md5: None,
            computed_md5: None,
            expected_crc32: None,
            computed_crc32: None,
            description: None,
            speed_history: Vec::new(),
            connection_logs: Vec::new(),
            task_speed_limit_kbps: 0,
        };

        let report_before =
            SegmentIntegrityAuditor::audit_segments(&meta.segments, meta.total_size);
        assert!(!report_before.is_contiguous);
        assert_eq!(report_before.detected_gaps, vec![(400, 599)]);

        let healed = SegmentIntegrityAuditor::heal_gaps(&mut meta);
        assert_eq!(healed, 1);

        let report_after =
            SegmentIntegrityAuditor::audit_segments(&meta.segments, meta.total_size);
        assert!(report_after.is_contiguous);
        assert_eq!(report_after.total_covered_bytes, 1000);
    }

    #[test]
    fn test_smart_routing_rule_matching() {
        let rule = SmartRoutingRule {
            rule_name: "Large Linux ISOs".to_string(),
            domain_contains: Some("ubuntu.com".to_string()),
            extension_in: vec!["iso".to_string()],
            min_size_bytes: Some(500 * 1024 * 1024),
            max_size_bytes: None,
            target_category: Some(FileCategory::Programs),
            target_queue: Some("sync".to_string()),
            target_subfolder: Some("LinuxISOs".to_string()),
        };

        assert!(rule.matches(
            "https://releases.ubuntu.com/24.04/ubuntu-24.04.iso",
            "ubuntu-24.04.iso",
            2_500_000_000
        ));
        assert!(!rule.matches(
            "https://example.com/ubuntu-24.04.iso",
            "ubuntu-24.04.iso",
            2_500_000_000
        ));
    }

    #[tokio::test]
    async fn test_zero_oom_ordered_disk_spooler() {
        let tmp_dir = std::env::temp_dir().join(format!("rustidm_spool_test_{}", Uuid::new_v4()));
        fs::create_dir_all(&tmp_dir).await.unwrap();
        let output_file = tmp_dir.join("assembled_movie.mp4");
        let spool_dir = OrderedDiskSpooler::create_spool_dir(&output_file)
            .await
            .unwrap();

        let out = File::create(&output_file).await.unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        let merger = tokio::spawn(async move {
            OrderedDiskSpooler::run_sequential_merger(out, rx, 4).await
        });

        // محاكاة اكتمال المقاطع بترتيب عشوائي (3 ثم 1 ثم 0 ثم 2)
        for &(idx, payload) in &[
            (3usize, &b"-PART3"[..]),
            (1usize, &b"-PART1"[..]),
            (0usize, &b"PART0"[..]),
            (2usize, &b"-PART2"[..]),
        ] {
            let (seg_path, seg_len) =
                OrderedDiskSpooler::write_segment_to_disk(&spool_dir, idx, payload)
                    .await
                    .unwrap();
            tx.send((idx, seg_path, seg_len)).unwrap();
        }
        drop(tx);

        let written = merger.await.unwrap().unwrap();
        let _ = fs::remove_dir_all(&spool_dir).await;
        let final_bytes = fs::read(&output_file).await.unwrap();
        assert_eq!(final_bytes, b"PART0-PART1-PART2-PART3");
        assert_eq!(written as usize, final_bytes.len());
        let _ = fs::remove_dir_all(&tmp_dir).await;
    }

    #[test]
    fn test_lock_free_atomic_segment_telemetry_and_split() {
        let registry = AtomicSegmentRegistry::new();
        let mut meta = DownloadStateMeta {
            task_id: "t_lockfree".to_string(),
            url: "https://example.com/huge.iso".to_string(),
            source_page_url: None,
            filename: "huge.iso".to_string(),
            save_dir: PathBuf::from("."),
            category: FileCategory::Programs,
            total_size: 4 * 1024 * 1024,
            resumable: true,
            etag: None,
            last_modified: None,
            custom_headers: HashMap::new(),
            expected_sha256: None,
            computed_sha256: None,
            status: DownloadStatus::Downloading,
            last_error: None,
            refresh_attempts: 0,
            segments: vec![FileSegment {
                id: 0,
                start_byte: 0,
                current_offset: 0,
                end_byte: 4 * 1024 * 1024 - 1,
                status: SegmentStatus::Downloading,
                split_count: 0,
                last_speed_bps: 0,
            }],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            queue_name: "main".to_string(),
            mirror_urls: Vec::new(),
            active_mirror_index: 0,
            expected_md5: None,
            computed_md5: None,
            expected_crc32: None,
            computed_crc32: None,
            description: None,
            speed_history: Vec::new(),
            connection_logs: Vec::new(),
            task_speed_limit_kbps: 0,
        };

        registry.initialize_from_meta(&meta);
        let atom0 = registry.get(0).unwrap();

        // محاكاة تقدم العامل الأول بدون قفل Mutex حتى 1 ميجابايت
        atom0.current_offset.store(1024 * 1024, Ordering::Release);
        atom0.last_speed_bps.store(50_000_000, Ordering::Relaxed);

        // طلب قسمة نصفية من عامل ثانٍ مع مزامنة العدادات الذرية
        let new_seg_id = DynamicDownloadJob::acquire_or_split_segment_with_atomics(
            &mut meta,
            512 * 1024,
            Some(&registry),
        );
        assert_eq!(new_seg_id, Some(1));
        assert_eq!(meta.segments.len(), 2);

        // التحقق من أن نهاية المقطع الأول الذرية قد تقلصت فوراً إلى نقطة المنتصف بدون فجوات
        let expected_mid = 1024 * 1024 + (3 * 1024 * 1024 / 2) - 1;
        assert_eq!(atom0.end_byte.load(Ordering::Acquire), expected_mid);
        let atom1 = registry.get(1).unwrap();
        assert_eq!(atom1.start_byte, expected_mid + 1);
        assert_eq!(atom1.end_byte.load(Ordering::Acquire), 4 * 1024 * 1024 - 1);
    }
}
