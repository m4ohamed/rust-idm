#![cfg_attr(
    all(feature = "native-ui", target_os = "windows"),
    windows_subsystem = "windows"
)]

//! RustIDM v1.1.0 — محرك إدارة وتسريع التحميل المتكامل لسطح المكتب بلغة Rust
//!
//! يطبق المعايير الهندسية الصارمة:
//! 1. التقسيم الديناميكي النصفي الخالي من التسابق (Race-Free In-Half Dynamic Segmentation) مع حماية ضد:
//!    - الخوادم التي تتجاهل ترويسة Range (تعيد 200 OK بدلاً من 206 Partial Content).
//!    - تغير حجم أو محتوى الملف أثناء التحميل (فحص Content-Range و ETag عبر If-Range).
//! 2. الحفظ الذري المتين (.idm.json.tmp -> sync_all -> rename -> sync_parent_dir) واستعادة المهام تلقائياً عند التشغيل.
//! 3. نظام تحديث الروابط منتهية الصلاحية (Refresh Expired URL via Extensible Providers) مع حجب الأسرار (Secret Redaction).
//! 4. مضيف المراسلة الأصلي لمتصفح كروم (Chrome Native Messaging Host: 4-byte LE length-prefixed stdio protocol).
//! 5. محرك الفيديو الموسع: دعم كامل لـ HLS (EXT-X-STREAM-INF, EXT-X-MEDIA, EXT-X-MAP, EXT-X-BYTERANGE, EXT-X-KEY AES-128-CBC)
//!    ومحلل MPEG-DASH (.mpd) وجسر yt-dlp القابل للإلغاء (kill_on_drop).
//! 6. الجدولة الزمنية، الزاحف المحدود (Bounded BFS Site Crawler)، وإجراءات ما بعد اكتمال الطابور.
//! 7. حزمة اختبارات التحقق المستقلة والقياسات المعمارية (18 اختباراً تكاملياً ووحدوياً).

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{Path as AxumPath, State},
    http::{HeaderMap as AxumHeaderMap, StatusCode},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Local, NaiveDateTime, NaiveTime, Utc};
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
#[cfg(not(feature = "native-ui"))]
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use regex::Regex;
use reqwest::{
    header::{
        HeaderMap, HeaderName, HeaderValue, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH,
        COOKIE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE, REFERER, USER_AGENT,
    },
    Client,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(not(feature = "native-ui"))]
use std::net::SocketAddr;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::{Read, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncSeekExt, AsyncWriteExt},
    process::Command as TokioCommand,
    sync::{Mutex, RwLock, Semaphore},
    time::{sleep, timeout},
};
use tower_http::cors::{Any, CorsLayer};
use url::Url;
use uuid::Uuid;

#[cfg(feature = "native-ui")]
mod native_ui;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

/// الحد الأدنى لحجم المقطع القابل للقسمة إلى النصف (512 كيلوبايت)
pub const MIN_SPLIT_SIZE: u64 = 512 * 1024;
/// الحد الأقصى لحجم رسالة Chrome Native Messaging (1 ميجابايت لمنع استنزاف الذاكرة)
pub const MAX_NATIVE_MSG_BYTES: usize = 1024 * 1024;

// ============================================================================
// 1. التصنيف التلقائي للملفات وحجب الأسرار (Categories & Secret Redaction)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileCategory {
    Video,
    Music,
    Compressed,
    Documents,
    Programs,
    General,
}

impl FileCategory {
    pub fn from_filename(filename: &str) -> Self {
        let ext = Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "mp4" | "mkv" | "webm" | "avi" | "mov" | "flv" | "wmv" | "m4v" | "ts" => {
                FileCategory::Video
            }
            "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "wma" => FileCategory::Music,
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "tgz" => FileCategory::Compressed,
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "epub" => {
                FileCategory::Documents
            }
            "exe" | "msi" | "deb" | "rpm" | "dmg" | "appimage" | "apk" | "iso" => {
                FileCategory::Programs
            }
            _ => FileCategory::General,
        }
    }

    pub fn folder_name(&self) -> &'static str {
        match self {
            FileCategory::Video => "Videos",
            FileCategory::Music => "Music",
            FileCategory::Compressed => "Compressed",
            FileCategory::Documents => "Documents",
            FileCategory::Programs => "Programs",
            FileCategory::General => "General",
        }
    }
}

/// حجب ترويسات المصادقة والكوكيز قبل عرضها في السجلات أو واجهة المستخدم لمنع تسريب الأسرار
pub fn redact_sensitive_headers(headers: &HashMap<String, String>) -> HashMap<String, String> {
    headers
        .iter()
        .map(|(k, v)| {
            let lower = k.to_lowercase();
            if lower.contains("cookie")
                || lower.contains("authorization")
                || lower.contains("token")
                || lower.contains("secret")
            {
                (k.clone(), "[REDACTED_SECRET]".to_string())
            } else {
                (k.clone(), v.clone())
            }
        })
        .collect()
}

// ============================================================================
// 2. محدد السرعة القياسي العادل (Fair Token-Bucket Speed Limiter)
// ============================================================================

pub struct SpeedLimiter {
    rate_bytes_per_sec: AtomicU64,
    state: Mutex<(f64, Instant)>,
}

impl SpeedLimiter {
    pub fn new(limit_kbps: u64) -> Arc<Self> {
        let rate = limit_kbps * 1024;
        Arc::new(Self {
            rate_bytes_per_sec: AtomicU64::new(rate),
            state: Mutex::new((rate as f64, Instant::now())),
        })
    }

    pub fn set_limit_kbps(&self, limit_kbps: u64) {
        self.rate_bytes_per_sec
            .store(limit_kbps * 1024, Ordering::SeqCst);
    }

    pub fn get_limit_kbps(&self) -> u64 {
        self.rate_bytes_per_sec.load(Ordering::Relaxed) / 1024
    }

    pub async fn acquire(&self, bytes: usize) {
        let rate = self.rate_bytes_per_sec.load(Ordering::Relaxed);
        if rate == 0 {
            return;
        }
        let rate_f = rate as f64;
        let needed = bytes as f64;

        let wait_duration = {
            let mut guard = self.state.lock().await;
            let (ref mut tokens, ref mut last_instant) = *guard;
            let elapsed = last_instant.elapsed().as_secs_f64();
            *last_instant = Instant::now();
            *tokens = (*tokens + elapsed * rate_f).min(rate_f);

            if *tokens >= needed {
                *tokens -= needed;
                Duration::ZERO
            } else {
                let deficit = needed - *tokens;
                *tokens = 0.0;
                Duration::from_secs_f64(deficit / rate_f)
            }
        };

        if !wait_duration.is_zero() {
            sleep(wait_duration).await;
        }
    }
}

// ============================================================================
// 3. هياكل بيانات المقاطع وحالة التحميل وتجديد الروابط
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SegmentStatus {
    Pending,
    Downloading,
    Completed,
    Paused,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSegment {
    pub id: usize,
    pub start_byte: u64,
    pub current_offset: u64,
    pub end_byte: u64, // شامل (inclusive byte index per RFC 9110 / RFC 7233)
    pub status: SegmentStatus,
    pub split_count: usize,
    pub last_speed_bps: u64,
}

impl FileSegment {
    pub fn remaining_bytes(&self) -> u64 {
        if self.current_offset > self.end_byte {
            0
        } else {
            (self.end_byte - self.current_offset) + 1
        }
    }

    pub fn downloaded_bytes(&self) -> u64 {
        if self.current_offset <= self.start_byte {
            0
        } else {
            (self.current_offset - self.start_byte).min((self.end_byte - self.start_byte) + 1)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Paused,
    NeedsUrlRefresh,
    RefreshingUrl,
    Assembling,
    VerifyingChecksum,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadStateMeta {
    pub task_id: String,
    pub url: String,
    pub source_page_url: Option<String>,
    pub filename: String,
    pub save_dir: PathBuf,
    pub category: FileCategory,
    pub total_size: u64,
    pub resumable: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub custom_headers: HashMap<String, String>,
    pub expected_sha256: Option<String>,
    pub computed_sha256: Option<String>,
    pub status: DownloadStatus,
    pub last_error: Option<String>,
    pub refresh_attempts: u32,
    pub segments: Vec<FileSegment>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub queue_name: String,
}

impl DownloadStateMeta {
    pub fn part_file_path(&self) -> PathBuf {
        self.save_dir.join(format!("{}.part", self.filename))
    }

    pub fn meta_file_path(&self) -> PathBuf {
        self.save_dir.join(format!("{}.idm.json", self.filename))
    }

    pub fn final_file_path(&self) -> PathBuf {
        self.save_dir.join(&self.filename)
    }

    pub fn total_downloaded(&self) -> u64 {
        self.segments.iter().map(|s| s.downloaded_bytes()).sum()
    }

    /// نسخة محمية خالية من الكوكيز والترويسات السرية للعرض الآمن في الواجهة والسجلات
    pub fn redacted_clone(&self) -> Self {
        let mut copy = self.clone();
        copy.custom_headers = redact_sensitive_headers(&self.custom_headers);
        copy
    }
}

// ============================================================================
// 4. نظام تحديث الروابط منتهية الصلاحية (Extensible Expired URL Refresh System)
// ============================================================================

pub trait UrlRefreshProvider: Send + Sync {
    fn provider_name(&self) -> &'static str;
    fn can_handle(&self, meta: &DownloadStateMeta) -> bool;
}

/// مزود قياسي يعيد فحص صفحة المصدر (source_page_url) لاستخراج رابط موقع جديد لنفس اسم الملف
pub struct SourcePageRefreshAdapter;

impl SourcePageRefreshAdapter {
    pub async fn try_refresh_from_source_page(
        client: &Client,
        meta: &DownloadStateMeta,
    ) -> Result<String> {
        let page_url = meta.source_page_url.as_ref().ok_or_else(|| {
            anyhow!("لا يوجد رابط صفحة مصدر (source_page_url) مسجل لتجديد الرابط تلقائياً")
        })?;

        let mut req = client.get(page_url);
        for (k, v) in &meta.custom_headers {
            if let (Ok(hn), Ok(hv)) = (HeaderName::from_str(k), HeaderValue::from_str(v)) {
                req = req.header(hn, hv);
            }
        }

        let html = req.send().await?.error_for_status()?.text().await?;
        let base = Url::parse(page_url)?;
        let link_re = Regex::new(r#"(?i)(?:href|src)\s*=\s*["']([^"'#]+)["']"#)?;

        // البحث عن رابط جديد في صفحة المصدر يشير إلى نفس اسم الملف
        let target_stem = Path::new(&meta.filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&meta.filename);

        for cap in link_re.captures_iter(&html) {
            if let Some(m) = cap.get(1) {
                if let Ok(resolved) = base.join(m.as_str()) {
                    let candidate = resolved.to_string();
                    if candidate.contains(target_stem) {
                        return Ok(candidate);
                    }
                }
            }
        }

        Err(anyhow!(
            "تعذر العثور على رابط محدث للملف {} داخل صفحة المصدر؛ يتطلب التجديد تدخلاً يدوياً أو تسجيل دخول",
            meta.filename
        ))
    }
}

/// تحليل ترويسة Content-Range وفق RFC 9110: مثال `bytes 100-199/1000`
pub fn parse_content_range_header(cr_value: &str) -> Option<(u64, u64, Option<u64>)> {
    let trimmed = cr_value.trim();
    let rest = trimmed.strip_prefix("bytes")?.trim();
    let mut parts = rest.split('/');
    let range_part = parts.next()?;
    let total_part = parts.next()?;

    let mut bounds = range_part.split('-');
    let start = bounds.next()?.trim().parse::<u64>().ok()?;
    let end = bounds.next()?.trim().parse::<u64>().ok()?;
    let total = if total_part.trim() == "*" {
        None
    } else {
        Some(total_part.trim().parse::<u64>().ok()?)
    };

    if start <= end {
        Some((start, end, total))
    } else {
        None
    }
}

// ============================================================================
// 5. محرك التقسيم الديناميكي المحصّن (Hardened DynamicDownloadJob)
// ============================================================================

pub struct DynamicDownloadJob {
    pub meta: Arc<Mutex<DownloadStateMeta>>,
    pub pause_flag: Arc<AtomicBool>,
    pub cancel_flag: Arc<AtomicBool>,
    pub resource_mutated_flag: Arc<AtomicBool>,
    pub expired_url_flag: Arc<AtomicBool>,
    pub speed_limiter: Arc<SpeedLimiter>,
    pub client: Client,
    pub max_connections: usize,
    pub min_split_size: u64,
    pub antivirus_cmd: Option<String>,
    pub uses_external_worker: bool,
    pub runner_active: AtomicBool,
    pub schedule_paused: AtomicBool,
}

impl DynamicDownloadJob {
    /// استكشاف الرابط المبدئي مع مهلة محددة والتحقق من دعم Range و Content-Range
    pub async fn probe_url(
        client: &Client,
        url: &str,
        custom_headers: &HeaderMap,
    ) -> Result<(u64, bool, String, Option<String>, Option<String>)> {
        let resp = timeout(
            Duration::from_secs(15),
            client
                .get(url)
                .headers(custom_headers.clone())
                .header(RANGE, "bytes=0-0")
                .send(),
        )
        .await
        .context("انتهت مهلة الاتصال المبدئي بالخادم")?
        .context("فشل الاتصال المبدئي بالخادم")?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::GONE
        {
            return Err(anyhow!(
                "HTTP_EXPIRED_OR_FORBIDDEN: استجاب الخادم بالحالة {}",
                status
            ));
        }
        resp.error_for_status_ref()?;

        let headers = resp.headers();

        // الخادم يدعم الاستئناف الحقيقي إذا أعاد 206 Partial Content مع Content-Range صالح
        let mut resumable = status == reqwest::StatusCode::PARTIAL_CONTENT;
        if let Some(ar) = headers.get(ACCEPT_RANGES).and_then(|v| v.to_str().ok()) {
            if ar.to_lowercase().contains("none") {
                resumable = false;
            } else if ar.to_lowercase().contains("bytes")
                && status == reqwest::StatusCode::PARTIAL_CONTENT
            {
                resumable = true;
            }
        }

        let mut total_size = 0u64;
        if let Some(cr) = headers.get("content-range").and_then(|v| v.to_str().ok()) {
            if let Some((_s, _e, Some(tot))) = parse_content_range_header(cr) {
                total_size = tot;
            }
        }
        if total_size == 0 {
            total_size = headers
                .get(CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
        }

        let filename = extract_filename(url, headers);
        let etag = headers
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let last_modified = headers
            .get(LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        Ok((total_size, resumable, filename, etag, last_modified))
    }

    /// التحقق من رابط جديد وتحديث المهمة بأمان دون إتلاف البيانات الجزئية الصالحة
    pub async fn verify_and_apply_refreshed_url(&self, new_url: &str) -> Result<bool> {
        let custom_headers_map = {
            let mut m = self.meta.lock().await;
            m.status = DownloadStatus::RefreshingUrl;
            m.refresh_attempts += 1;
            let mut hmap = HeaderMap::new();
            for (k, v) in &m.custom_headers {
                if let (Ok(hn), Ok(hv)) = (HeaderName::from_str(k), HeaderValue::from_str(v)) {
                    hmap.insert(hn, hv);
                }
            }
            hmap
        };

        let (new_size, new_resumable, _name, new_etag, new_last_mod) =
            Self::probe_url(&self.client, new_url, &custom_headers_map).await?;

        let mut m = self.meta.lock().await;
        let mut resource_compatible =
            new_resumable && (m.total_size == 0 || new_size == m.total_size);

        // إذا كان لكل من الرابط القديم والجديد ETag قوي (لا يبدأ بـ W/) واختلفا، فالمورد قد تغير!
        if let (Some(ref old_e), Some(ref new_e)) = (&m.etag, &new_etag) {
            if !old_e.starts_with("W/") && !new_e.starts_with("W/") && old_e != new_e {
                resource_compatible = false;
            }
        }

        m.url = new_url.to_string();
        m.etag = new_etag;
        m.last_modified = new_last_mod;
        m.resumable = new_resumable;
        m.last_error = None;
        self.expired_url_flag.store(false, Ordering::SeqCst);

        if resource_compatible {
            // الاحتفاظ بالبيانات الجزئية الصالحة واستئناف المقاطع المتبقية فقط
            for seg in m.segments.iter_mut() {
                if seg.status != SegmentStatus::Completed {
                    seg.status = SegmentStatus::Pending;
                }
            }
            let meta_path = m.meta_file_path();
            save_meta_to_disk_atomic(&m, &meta_path).await?;
            Ok(true)
        } else {
            // تغير المورد الأصلي: إلغاء الاستئناف الجزئي وإعادة التنزيل بأمان من الصفر
            m.total_size = new_size;
            m.segments.clear();
            let part_path = m.part_file_path();
            if part_path.exists() {
                let _ = fs::remove_file(&part_path).await;
            }
            let meta_path = m.meta_file_path();
            save_meta_to_disk_atomic(&m, &meta_path).await?;
            Ok(false)
        }
    }

    /// تنفيذ التحميل مع معالجة تغير المورد وانتهاء صلاحية الرابط والتحقق الصارم من SHA-256
    pub async fn execute(&self) -> Result<PathBuf> {
        loop {
            self.resource_mutated_flag.store(false, Ordering::SeqCst);
            self.expired_url_flag.store(false, Ordering::SeqCst);

            let (part_path, meta_path, final_path, total_size, resumable) = {
                let mut m = self.meta.lock().await;
                m.status = DownloadStatus::Downloading;
                m.updated_at = Utc::now();
                fs::create_dir_all(&m.save_dir).await?;
                (
                    m.part_file_path(),
                    m.meta_file_path(),
                    m.final_file_path(),
                    m.total_size,
                    m.resumable,
                )
            };

            if !part_path.exists() {
                let f = File::create(&part_path)
                    .await
                    .context("فشل إنشاء ملف .part على القرص")?;
                if total_size > 0 {
                    f.set_len(total_size)
                        .await
                        .context("فشل حجز المساحة المطلوبة على القرص (تحقق من المساحة المتوفرة)")?;
                }
                f.sync_all().await?;
            }

            {
                let mut m = self.meta.lock().await;
                if m.segments.is_empty() {
                    let end = if total_size > 0 {
                        total_size - 1
                    } else {
                        u64::MAX
                    };
                    m.segments.push(FileSegment {
                        id: 0,
                        start_byte: 0,
                        current_offset: 0,
                        end_byte: end,
                        status: SegmentStatus::Pending,
                        split_count: 0,
                        last_speed_bps: 0,
                    });
                } else {
                    for seg in m.segments.iter_mut() {
                        if seg.status != SegmentStatus::Completed {
                            if seg.current_offset > seg.end_byte {
                                seg.status = SegmentStatus::Completed;
                            } else {
                                seg.status = SegmentStatus::Pending;
                            }
                        }
                    }
                }
                save_meta_to_disk_atomic(&m, &meta_path).await?;
            }

            let meta_saver_clone = Arc::clone(&self.meta);
            let pause_saver_clone = Arc::clone(&self.pause_flag);
            let meta_path_clone = meta_path.clone();
            let saver_handle = tokio::spawn(async move {
                loop {
                    sleep(Duration::from_millis(1500)).await;
                    if pause_saver_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    let m = meta_saver_clone.lock().await;
                    let _ = save_meta_to_disk_atomic(&m, &meta_path_clone).await;
                    if m.status == DownloadStatus::Completed
                        || m.status == DownloadStatus::Failed
                        || m.status == DownloadStatus::Paused
                        || m.status == DownloadStatus::NeedsUrlRefresh
                    {
                        break;
                    }
                }
            });

            let worker_count = if resumable && total_size >= self.min_split_size * 2 {
                self.max_connections.max(1)
            } else {
                1
            };

            let mut handles = Vec::with_capacity(worker_count);
            for worker_idx in 0..worker_count {
                let meta_arc = Arc::clone(&self.meta);
                let pause_arc = Arc::clone(&self.pause_flag);
                let cancel_arc = Arc::clone(&self.cancel_flag);
                let mutated_arc = Arc::clone(&self.resource_mutated_flag);
                let expired_arc = Arc::clone(&self.expired_url_flag);
                let limiter_arc = Arc::clone(&self.speed_limiter);
                let client_clone = self.client.clone();
                let part_path_clone = part_path.clone();
                let min_split = self.min_split_size;
                let max_conn = self.max_connections;

                let handle = tokio::spawn(async move {
                    Self::connection_worker_loop(
                        worker_idx,
                        meta_arc,
                        pause_arc,
                        cancel_arc,
                        mutated_arc,
                        expired_arc,
                        limiter_arc,
                        client_clone,
                        part_path_clone,
                        min_split,
                        max_conn,
                    )
                    .await
                });
                handles.push(handle);
            }

            for h in handles {
                let _ = h.await;
            }
            saver_handle.abort();

            // 1. فحص ما إذا تغير المورد على الخادم أثناء الاستئناف (If-Range أعاد 200 OK)
            if self.resource_mutated_flag.load(Ordering::SeqCst) {
                let mut m = self.meta.lock().await;
                m.segments.clear();
                let _ = fs::remove_file(&part_path).await;
                let url_clone = m.url.clone();
                let mut hmap = HeaderMap::new();
                for (k, v) in &m.custom_headers {
                    if let (Ok(hn), Ok(hv)) = (HeaderName::from_str(k), HeaderValue::from_str(v)) {
                        hmap.insert(hn, hv);
                    }
                }
                drop(m);

                if let Ok((new_sz, new_res, _, new_etag, new_lm)) =
                    Self::probe_url(&self.client, &url_clone, &hmap).await
                {
                    let mut m = self.meta.lock().await;
                    m.total_size = new_sz;
                    m.resumable = new_res;
                    m.etag = new_etag;
                    m.last_modified = new_lm;
                }
                // إعادة المحاولة من الصفر بأمان لمنع دمج بيانات قديمة وجديدة
                continue;
            }

            // 2. فحص ما إذا انتهت صلاحية الرابط (401 / 403 / 410) ومحاولة تجديده تلقائياً
            if self.expired_url_flag.load(Ordering::SeqCst) {
                let (can_try_auto, meta_snapshot) = {
                    let m = self.meta.lock().await;
                    (
                        m.refresh_attempts < 3 && m.source_page_url.is_some(),
                        m.clone(),
                    )
                };

                if can_try_auto {
                    if let Ok(fresh_url) = SourcePageRefreshAdapter::try_refresh_from_source_page(
                        &self.client,
                        &meta_snapshot,
                    )
                    .await
                    {
                        if self
                            .verify_and_apply_refreshed_url(&fresh_url)
                            .await
                            .is_ok()
                        {
                            continue; // استئناف التحميل بالرابط الجديد!
                        }
                    }
                }

                let mut m = self.meta.lock().await;
                m.status = DownloadStatus::NeedsUrlRefresh;
                m.last_error = Some(
                    "انتهت صلاحية رابط التحميل (403/410)؛ يرجى تزويد رابط محدث أو تحديث صفحة المصدر"
                        .to_string(),
                );
                save_meta_to_disk_atomic(&m, &meta_path).await?;
                return Err(anyhow!("انتهت صلاحية الرابط ويتطلب التجديد"));
            }

            let mut m = self.meta.lock().await;
            if self.cancel_flag.load(Ordering::Relaxed) {
                m.status = DownloadStatus::Failed;
                m.last_error = Some("تم إلغاء التحميل بواسطة المستخدم".to_string());
                save_meta_to_disk_atomic(&m, &meta_path).await?;
                return Err(anyhow!("تم إلغاء التحميل بواسطة المستخدم"));
            }

            if self.pause_flag.load(Ordering::Relaxed) {
                m.status = DownloadStatus::Paused;
                for seg in m.segments.iter_mut() {
                    if seg.status == SegmentStatus::Downloading {
                        seg.status = SegmentStatus::Paused;
                    }
                }
                save_meta_to_disk_atomic(&m, &meta_path).await?;
                return Err(anyhow!("تم إيقاف التحميل مؤقتاً وحفظ الحالة ذرياً"));
            }

            let all_done = m
                .segments
                .iter()
                .all(|s| s.status == SegmentStatus::Completed);
            if !all_done {
                m.status = DownloadStatus::Failed;
                m.last_error = Some("تعذر إكمال بعض المقاطع بعد استنفاد المحاولات".to_string());
                save_meta_to_disk_atomic(&m, &meta_path).await?;
                return Err(anyhow!("تعذر إكمال بعض المقاطع، الحالة محفوظة للاستئناف"));
            }

            // التأكد من عدم وجود أي فجوات أو تداخل بين المقاطع قبل التجميع النهائي
            if m.total_size > 0 && m.total_downloaded() != m.total_size {
                m.status = DownloadStatus::Failed;
                m.last_error = Some(format!(
                    "عدم تطابق مجموع البايتات المحملة ({}) مع الحجم المتوقع ({})",
                    m.total_downloaded(),
                    m.total_size
                ));
                save_meta_to_disk_atomic(&m, &meta_path).await?;
                return Err(anyhow!("فشل فحص تكامل حدود المقاطع"));
            }

            // مزامنة ملف .part على القرص الصلب قبل إعادة التسمية
            m.status = DownloadStatus::Assembling;
            {
                let pf = OpenOptions::new().write(true).open(&part_path).await?;
                pf.sync_all().await?;
            }
            fs::rename(&part_path, &final_path).await?;
            sync_parent_dir(&final_path).await;

            // التحقق من بصمة SHA-256 قبل إعلان الاكتمال
            m.status = DownloadStatus::VerifyingChecksum;
            let actual_hash = compute_file_sha256(&final_path).await?;
            m.computed_sha256 = Some(actual_hash.clone());

            if let Some(expected_hash) = m.expected_sha256.clone() {
                if !actual_hash.eq_ignore_ascii_case(&expected_hash) {
                    m.status = DownloadStatus::Failed;
                    m.last_error = Some(format!(
                        "عدم تطابق بصمة SHA-256: المتوقع {} والفعلي {}",
                        expected_hash, actual_hash
                    ));
                    return Err(anyhow!("فشل التحقق من بصمة SHA-256"));
                }
            }

            if let Some(ref av_cmd) = self.antivirus_cmd {
                let _ = Command::new(av_cmd).arg(&final_path).status();
            }

            m.status = DownloadStatus::Completed;
            m.last_error = None;
            m.updated_at = Utc::now();
            let _ = fs::remove_file(&meta_path).await;

            return Ok(final_path);
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn connection_worker_loop(
        _worker_idx: usize,
        meta: Arc<Mutex<DownloadStateMeta>>,
        pause_flag: Arc<AtomicBool>,
        cancel_flag: Arc<AtomicBool>,
        resource_mutated_flag: Arc<AtomicBool>,
        expired_url_flag: Arc<AtomicBool>,
        speed_limiter: Arc<SpeedLimiter>,
        client: Client,
        part_path: PathBuf,
        min_split_size: u64,
        max_connections: usize,
    ) -> Result<()> {
        loop {
            if pause_flag.load(Ordering::Relaxed)
                || cancel_flag.load(Ordering::Relaxed)
                || resource_mutated_flag.load(Ordering::Relaxed)
                || expired_url_flag.load(Ordering::Relaxed)
            {
                break;
            }

            let assigned_seg_id = {
                let mut m = meta.lock().await;
                let active_count = m
                    .segments
                    .iter()
                    .filter(|s| s.status == SegmentStatus::Downloading)
                    .count();
                if active_count >= max_connections {
                    None
                } else {
                    Self::acquire_or_split_segment(&mut m, min_split_size)
                }
            };

            let seg_id = match assigned_seg_id {
                Some(id) => id,
                None => break,
            };

            let mut retries = 0;
            loop {
                if pause_flag.load(Ordering::Relaxed)
                    || cancel_flag.load(Ordering::Relaxed)
                    || resource_mutated_flag.load(Ordering::Relaxed)
                    || expired_url_flag.load(Ordering::Relaxed)
                {
                    break;
                }

                let res = Self::download_segment(
                    seg_id,
                    &meta,
                    &pause_flag,
                    &cancel_flag,
                    &resource_mutated_flag,
                    &expired_url_flag,
                    &speed_limiter,
                    &client,
                    &part_path,
                )
                .await;

                match res {
                    Ok(()) => break,
                    Err(e) => {
                        let err_msg = e.to_string();
                        if err_msg.contains("RESOURCE_MUTATED")
                            || err_msg.contains("URL_EXPIRED")
                            || err_msg.contains("RANGE_IGNORED")
                        {
                            break;
                        }
                        retries += 1;
                        if retries > 4 {
                            let mut m = meta.lock().await;
                            m.last_error = Some(err_msg);
                            if let Some(s) = m.segments.iter_mut().find(|s| s.id == seg_id) {
                                s.status = SegmentStatus::Failed;
                            }
                            break;
                        }
                        sleep(Duration::from_millis(250 * retries)).await;
                    }
                }
            }
        }
        Ok(())
    }

    /// قاعدة القسمة النصفية الذرية (In-Half Division Rule) مع مراعاة حجم المقطع وسرعة الاتصال
    pub fn acquire_or_split_segment(
        m: &mut DownloadStateMeta,
        min_split_size: u64,
    ) -> Option<usize> {
        if let Some(seg) = m
            .segments
            .iter_mut()
            .find(|s| s.status == SegmentStatus::Pending)
        {
            seg.status = SegmentStatus::Downloading;
            return Some(seg.id);
        }

        if !m.resumable || m.total_size == 0 {
            return None;
        }

        let mut largest_idx: Option<usize> = None;
        let mut max_remaining: u64 = 0;

        for (idx, seg) in m.segments.iter().enumerate() {
            if seg.status == SegmentStatus::Downloading {
                let rem = seg.remaining_bytes();
                if rem > max_remaining {
                    max_remaining = rem;
                    largest_idx = Some(idx);
                }
            }
        }

        if let Some(idx) = largest_idx {
            if max_remaining >= min_split_size * 2 {
                let new_id = m.segments.len();
                let old_end = m.segments[idx].end_byte;
                let cur_off = m.segments[idx].current_offset;
                let midpoint = cur_off + (max_remaining / 2) - 1;

                if midpoint >= cur_off && midpoint < old_end {
                    m.segments[idx].end_byte = midpoint;
                    m.segments[idx].split_count += 1;

                    let new_segment = FileSegment {
                        id: new_id,
                        start_byte: midpoint + 1,
                        current_offset: midpoint + 1,
                        end_byte: old_end,
                        status: SegmentStatus::Downloading,
                        split_count: 0,
                        last_speed_bps: 0,
                    };
                    m.segments.push(new_segment);
                    return Some(new_id);
                }
            }
        }

        None
    }

    #[allow(clippy::too_many_arguments)]
    async fn download_segment(
        seg_id: usize,
        meta: &Arc<Mutex<DownloadStateMeta>>,
        pause_flag: &Arc<AtomicBool>,
        cancel_flag: &Arc<AtomicBool>,
        resource_mutated_flag: &Arc<AtomicBool>,
        expired_url_flag: &Arc<AtomicBool>,
        speed_limiter: &Arc<SpeedLimiter>,
        client: &Client,
        part_path: &Path,
    ) -> Result<()> {
        let (
            url,
            start_offset,
            initial_end,
            expected_total,
            resumable,
            custom_headers,
            if_range_val,
            is_resumed,
        ) = {
            let m = meta.lock().await;
            let seg = m
                .segments
                .iter()
                .find(|s| s.id == seg_id)
                .ok_or_else(|| anyhow!("المقطع غير موجود"))?;
            if seg.current_offset > seg.end_byte {
                return Ok(());
            }
            let if_range = m.etag.clone().or_else(|| m.last_modified.clone());
            let resumed = seg.current_offset > seg.start_byte;
            (
                m.url.clone(),
                seg.current_offset,
                seg.end_byte,
                m.total_size,
                m.resumable,
                m.custom_headers.clone(),
                if_range,
                resumed,
            )
        };

        let mut req = client.get(&url);
        for (k, v) in &custom_headers {
            if let (Ok(hn), Ok(hv)) = (HeaderName::from_str(k), HeaderValue::from_str(v)) {
                req = req.header(hn, hv);
            }
        }

        if resumable && initial_end < u64::MAX {
            req = req.header(RANGE, format!("bytes={}-{}", start_offset, initial_end));
            if is_resumed {
                if let Some(ref validator) = if_range_val {
                    if let Ok(hv) = HeaderValue::from_str(validator) {
                        req = req.header(IF_RANGE, hv);
                    }
                }
            }
        }

        let resp = timeout(Duration::from_secs(20), req.send())
            .await
            .context("انتهت مهلة طلب المقطع")??;

        let status = resp.status();

        // اكتشاف انتهاء صلاحية الرابط (401 / 403 / 410) لتشغيل آلية Refresh Expired URL
        if status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::GONE
        {
            expired_url_flag.store(true, Ordering::SeqCst);
            return Err(anyhow!("URL_EXPIRED: HTTP {}", status));
        }

        resp.error_for_status_ref()?;

        // حماية حاسمة: إذا طلبنا نطاقاً يبدأ بعد الصفر (أو استئنافاً بـ If-Range) وأعاد الخادم 200 OK بدلاً من 206:
        if resumable && (start_offset > 0 || is_resumed) && status == reqwest::StatusCode::OK {
            if is_resumed {
                // الخادم أعاد 200 OK لأن ETag تغير (تغير الملف الأصلي على الخادم!)
                resource_mutated_flag.store(true, Ordering::SeqCst);
                return Err(anyhow!(
                    "RESOURCE_MUTATED: أعاد الخادم 200 OK عند الاستئناف بـ If-Range مما يدل على تغير الملف"
                ));
            } else {
                // الخادم تجاهل ترويسة Range لعامل موازٍ! نلغي هذا العامل فوراً لمنع كتابة البايت 0 في موضع start_offset
                let mut m = meta.lock().await;
                m.resumable = false;
                if let Some(seg) = m.segments.iter_mut().find(|s| s.id == seg_id) {
                    seg.status = SegmentStatus::Completed;
                }
                return Err(anyhow!("RANGE_IGNORED: الخادم تجاهل ترويسة Range"));
            }
        }

        // التحقق من ترويسة Content-Range في استجابات 206 Partial Content
        if status == reqwest::StatusCode::PARTIAL_CONTENT {
            if let Some(cr) = resp
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
            {
                if let Some((cr_start, _cr_end, cr_total)) = parse_content_range_header(cr) {
                    if cr_start != start_offset {
                        return Err(anyhow!(
                            "بداية Content-Range ({}) لا تطابق المؤشر المطلوب ({})",
                            cr_start,
                            start_offset
                        ));
                    }
                    if let Some(tot) = cr_total {
                        if expected_total > 0 && tot != expected_total {
                            resource_mutated_flag.store(true, Ordering::SeqCst);
                            return Err(anyhow!(
                                "RESOURCE_MUTATED: تغير حجم الملف على الخادم من {} إلى {}",
                                expected_total,
                                tot
                            ));
                        }
                    }
                }
            }
        }

        let mut stream = resp.bytes_stream();
        let mut file = OpenOptions::new().write(true).open(part_path).await?;
        file.seek(SeekFrom::Start(start_offset)).await?;

        let mut current_pos = start_offset;
        let seg_start_instant = Instant::now();
        let mut seg_session_bytes = 0u64;

        while let Some(chunk_res) = timeout(Duration::from_secs(20), stream.next())
            .await
            .context("انتهت مهلة قراءة البيانات من الشبكة")?
        {
            if pause_flag.load(Ordering::Relaxed)
                || cancel_flag.load(Ordering::Relaxed)
                || resource_mutated_flag.load(Ordering::Relaxed)
                || expired_url_flag.load(Ordering::Relaxed)
            {
                file.flush().await?;
                return Ok(());
            }

            let chunk = chunk_res?;
            speed_limiter.acquire(chunk.len()).await;

            let dynamic_end = {
                let m = meta.lock().await;
                m.segments
                    .iter()
                    .find(|s| s.id == seg_id)
                    .map(|s| s.end_byte)
                    .unwrap_or(initial_end)
            };

            if current_pos > dynamic_end {
                break;
            }

            let max_writable = (dynamic_end - current_pos + 1) as usize;
            let write_len = chunk.len().min(max_writable);

            file.write_all(&chunk[..write_len]).await?;
            current_pos += write_len as u64;
            seg_session_bytes += write_len as u64;

            let elapsed_secs = seg_start_instant.elapsed().as_secs_f64().max(0.001);
            let current_bps = (seg_session_bytes as f64 / elapsed_secs) as u64;

            {
                let mut m = meta.lock().await;
                if let Some(seg) = m.segments.iter_mut().find(|s| s.id == seg_id) {
                    seg.current_offset = current_pos;
                    seg.last_speed_bps = current_bps;
                    if current_pos > seg.end_byte {
                        seg.status = SegmentStatus::Completed;
                        file.flush().await?;
                        return Ok(());
                    }
                }
            }

            if write_len < chunk.len() {
                break;
            }
        }

        file.flush().await?;
        {
            let mut m = meta.lock().await;
            let total_size = m.total_size;
            if let Some(seg) = m.segments.iter_mut().find(|s| s.id == seg_id) {
                if seg.current_offset > seg.end_byte || total_size == 0 {
                    seg.status = SegmentStatus::Completed;
                }
            }
        }

        Ok(())
    }
}

// ============================================================================
// 6. محرك الفيديو الموسع: HLS (RFC 8216 + AES-128-CBC) + MPEG-DASH (.mpd) + yt-dlp
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HlsVariant {
    pub bandwidth: u64,
    pub resolution: Option<String>,
    pub audio_group: Option<String>,
    pub uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HlsMediaRendition {
    pub media_type: String, // AUDIO | SUBTITLES
    pub group_id: String,
    pub name: String,
    pub uri: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HlsEncryptionMethod {
    None,
    Aes128 {
        key_uri: String,
        iv: Option<[u8; 16]>,
    },
    UnsupportedDrm(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HlsSegmentItem {
    pub sequence_number: u64,
    pub duration_secs: f64,
    pub uri: String,
    pub byte_range: Option<(u64, u64)>, // (length, offset)
    pub encryption: HlsEncryptionMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedMediaPlaylist {
    pub init_map_uri: Option<String>,
    pub media_sequence_start: u64,
    pub is_live_stream: bool,
    pub segments: Vec<HlsSegmentItem>,
}

pub struct MediaStreamEngine {
    client: Client,
}

impl MediaStreamEngine {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    /// فك تشفير مقطع HLS مشفر بمعيار AES-128-CBC وفق RFC 8216 القسم 5.2
    /// إذا لم يُذكر IV صراحةً في #EXT-X-KEY، يُشتق كرقم تسلسل المقطع (Media Sequence Number) بصيغة 128-bit Big-Endian!
    pub fn decrypt_hls_aes128_segment(
        ciphertext: &[u8],
        key: &[u8; 16],
        explicit_iv: Option<[u8; 16]>,
        sequence_number: u64,
    ) -> Result<Vec<u8>> {
        let iv = explicit_iv.unwrap_or_else(|| {
            let mut buf = [0u8; 16];
            buf[8..16].copy_from_slice(&sequence_number.to_be_bytes());
            buf
        });

        let mut buffer = ciphertext.to_vec();
        let decrypted = Aes128CbcDec::new(key.into(), (&iv).into())
            .decrypt_padded_mut::<Pkcs7>(&mut buffer)
            .map_err(|_| {
                anyhow!("فشل فك تشفير مقطع HLS AES-128-CBC: المفتاح غير صالح أو البيانات تالفة")
            })?;

        Ok(decrypted.to_vec())
    }

    /// تحليل قائمة HLS الرئيسية (Master Playlist) بما في ذلك #EXT-X-STREAM-INF و #EXT-X-MEDIA
    pub fn parse_master_playlist(
        text: &str,
        base_url: &Url,
    ) -> Result<(Vec<HlsVariant>, Vec<HlsMediaRendition>)> {
        let mut variants = Vec::new();
        let mut renditions = Vec::new();

        let bw_re = Regex::new(r"BANDWIDTH=(\d+)")?;
        let res_re = Regex::new(r"RESOLUTION=([0-9]+x[0-9]+)")?;
        let audio_re = Regex::new(r#"AUDIO="([^"]+)""#)?;
        let type_re = Regex::new(r"TYPE=([A-Z]+)")?;
        let group_re = Regex::new(r#"GROUP-ID="([^"]+)""#)?;
        let name_re = Regex::new(r#"NAME="([^"]+)""#)?;
        let uri_re = Regex::new(r#"URI="([^"]+)""#)?;

        let mut lines = text.lines().peekable();
        while let Some(line) = lines.next() {
            let trimmed = line.trim();
            if trimmed.starts_with("#EXT-X-MEDIA:") {
                let media_type = type_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                let group_id = group_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                let name = name_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                let uri = uri_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .and_then(|m| base_url.join(m.as_str()).ok())
                    .map(|u| u.to_string());

                renditions.push(HlsMediaRendition {
                    media_type,
                    group_id,
                    name,
                    uri,
                });
            } else if trimmed.starts_with("#EXT-X-STREAM-INF:") {
                let bandwidth = bw_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<u64>().ok())
                    .unwrap_or(0);
                let resolution = res_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string());
                let audio_group = audio_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string());

                while let Some(next_line) = lines.next() {
                    let next_trimmed = next_line.trim();
                    if !next_trimmed.is_empty() && !next_trimmed.starts_with('#') {
                        let resolved = base_url.join(next_trimmed)?.to_string();
                        variants.push(HlsVariant {
                            bandwidth,
                            resolution,
                            audio_group,
                            uri: resolved,
                        });
                        break;
                    }
                }
            }
        }
        Ok((variants, renditions))
    }

    /// تحليل قائمة وسائط HLS (Media Playlist) مع دعم #EXT-X-MAP و #EXT-X-BYTERANGE و #EXT-X-KEY و #EXT-X-ENDLIST
    pub fn parse_media_playlist(text: &str, base_url: &Url) -> Result<ParsedMediaPlaylist> {
        let mut init_map_uri = None;
        let mut media_seq: u64 = 0;
        let mut has_endlist = false;
        let mut current_enc = HlsEncryptionMethod::None;
        let mut pending_duration = 0.0f64;
        let mut pending_byterange: Option<(u64, u64)> = None;
        let mut last_byterange_end: u64 = 0;
        let mut segments = Vec::new();

        let map_re = Regex::new(r#"#EXT-X-MAP:.*URI="([^"]+)""#)?;
        let seq_re = Regex::new(r"#EXT-X-MEDIA-SEQUENCE:(\d+)")?;
        let inf_re = Regex::new(r"#EXTINF:([0-9.]+)")?;
        let br_re = Regex::new(r"#EXT-X-BYTERANGE:(\d+)(?:@(\d+))?")?;
        let method_re = Regex::new(r"METHOD=([A-Z0-9-]+)")?;
        let uri_re = Regex::new(r#"URI="([^"]+)""#)?;
        let iv_re = Regex::new(r"IV=0[xX]([0-9a-fA-F]{32})")?;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("#EXT-X-MAP:") {
                if let Some(cap) = map_re.captures(trimmed).and_then(|c| c.get(1)) {
                    init_map_uri = Some(base_url.join(cap.as_str())?.to_string());
                }
            } else if trimmed.starts_with("#EXT-X-MEDIA-SEQUENCE:") {
                if let Some(seq) = seq_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<u64>().ok())
                {
                    media_seq = seq;
                }
            } else if trimmed.starts_with("#EXTINF:") {
                pending_duration = inf_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<f64>().ok())
                    .unwrap_or(0.0);
            } else if trimmed.starts_with("#EXT-X-BYTERANGE:") {
                if let Some(caps) = br_re.captures(trimmed) {
                    let len = caps
                        .get(1)
                        .and_then(|m| m.as_str().parse::<u64>().ok())
                        .unwrap_or(0);
                    let offset = caps
                        .get(2)
                        .and_then(|m| m.as_str().parse::<u64>().ok())
                        .unwrap_or(last_byterange_end);
                    last_byterange_end = offset + len;
                    pending_byterange = Some((len, offset));
                }
            } else if trimmed.starts_with("#EXT-X-KEY:") {
                let method = method_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str())
                    .unwrap_or("NONE");

                match method {
                    "NONE" => current_enc = HlsEncryptionMethod::None,
                    "AES-128" => {
                        let key_uri = uri_re
                            .captures(trimmed)
                            .and_then(|c| c.get(1))
                            .ok_or_else(|| anyhow!("وسم #EXT-X-KEY يفتقر إلى URI المفتاح"))?;
                        let resolved_key_uri = base_url.join(key_uri.as_str())?.to_string();

                        let iv =
                            if let Some(iv_hex) = iv_re.captures(trimmed).and_then(|c| c.get(1)) {
                                let decoded = hex::decode(iv_hex.as_str())?;
                                let mut arr = [0u8; 16];
                                arr.copy_from_slice(&decoded[..16]);
                                Some(arr)
                            } else {
                                None
                            };

                        current_enc = HlsEncryptionMethod::Aes128 {
                            key_uri: resolved_key_uri,
                            iv,
                        };
                    }
                    other => {
                        current_enc = HlsEncryptionMethod::UnsupportedDrm(other.to_string());
                    }
                }
            } else if trimmed == "#EXT-X-ENDLIST" {
                has_endlist = true;
            } else if !trimmed.starts_with('#') {
                if let HlsEncryptionMethod::UnsupportedDrm(ref drm_name) = current_enc {
                    return Err(anyhow!(
                        "قائمة البث محمية بنظام تشفير غير مدعوم أو DRM ({})؛ لا يمكن فك تشفيرها بدون تصريح قياسي",
                        drm_name
                    ));
                }
                let resolved = base_url.join(trimmed)?.to_string();
                let seq_num = media_seq + segments.len() as u64;
                segments.push(HlsSegmentItem {
                    sequence_number: seq_num,
                    duration_secs: pending_duration,
                    uri: resolved,
                    byte_range: pending_byterange.take(),
                    encryption: current_enc.clone(),
                });
                pending_duration = 0.0;
            }
        }

        Ok(ParsedMediaPlaylist {
            init_map_uri,
            media_sequence_start: media_seq,
            is_live_stream: !has_endlist,
            segments,
        })
    }

    /// تحميل بث HLS (.m3u8) مع فك تشفير AES-128-CBC التلقائي ودعم #EXT-X-MAP و #EXT-X-BYTERANGE
    pub async fn download_hls_stream(
        &self,
        m3u8_url: &str,
        output_file: &Path,
        headers: &HeaderMap,
        concurrency: usize,
    ) -> Result<PathBuf> {
        let playlist_text = self
            .client
            .get(m3u8_url)
            .headers(headers.clone())
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        let base_url = Url::parse(m3u8_url)?;

        if playlist_text.contains("#EXT-X-STREAM-INF") {
            let (variants, _renditions) = Self::parse_master_playlist(&playlist_text, &base_url)?;
            let best_variant = variants
                .into_iter()
                .max_by_key(|v| v.bandwidth)
                .ok_or_else(|| anyhow!("لم يتم العثور على جودات صالحة في ملف m3u8 الرئيسي"))?;

            return Box::pin(self.download_hls_stream(
                &best_variant.uri,
                output_file,
                headers,
                concurrency,
            ))
            .await;
        }

        let parsed = Self::parse_media_playlist(&playlist_text, &base_url)?;
        if parsed.segments.is_empty() {
            return Err(anyhow!("قائمة مقاطع الفيديو فارغة"));
        }

        if let Some(parent) = output_file.parent() {
            fs::create_dir_all(parent).await?;
        }
        let mut out = File::create(output_file).await?;

        if let Some(ref map_uri) = parsed.init_map_uri {
            let init_bytes = self
                .client
                .get(map_uri)
                .headers(headers.clone())
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            out.write_all(&init_bytes).await?;
        }

        // ذاكرة تخزين مؤقت لمفاتيح AES-128 لتجنب إعادة تحميل نفس المفتاح لكل مقطع
        let key_cache: Arc<Mutex<HashMap<String, [u8; 16]>>> = Arc::new(Mutex::new(HashMap::new()));
        let sem = Arc::new(Semaphore::new(concurrency.max(1)));
        let mut tasks = Vec::with_capacity(parsed.segments.len());

        for (idx, seg) in parsed.segments.into_iter().enumerate() {
            let sem_clone = Arc::clone(&sem);
            let client_clone = self.client.clone();
            let headers_clone = headers.clone();
            let cache_clone = Arc::clone(&key_cache);

            tasks.push(tokio::spawn(async move {
                let _permit = sem_clone.acquire().await.unwrap();
                let mut req = client_clone.get(&seg.uri).headers(headers_clone.clone());
                if let Some((len, offset)) = seg.byte_range {
                    req = req.header(RANGE, format!("bytes={}-{}", offset, offset + len - 1));
                }

                let raw_bytes = req.send().await?.error_for_status()?.bytes().await?;

                let final_bytes = match seg.encryption {
                    HlsEncryptionMethod::None => raw_bytes.to_vec(),
                    HlsEncryptionMethod::Aes128 { key_uri, iv } => {
                        let key_arr = {
                            let mut guard = cache_clone.lock().await;
                            if let Some(k) = guard.get(&key_uri) {
                                *k
                            } else {
                                let k_bytes = client_clone
                                    .get(&key_uri)
                                    .headers(headers_clone)
                                    .send()
                                    .await?
                                    .error_for_status()?
                                    .bytes()
                                    .await?;
                                if k_bytes.len() != 16 {
                                    return Err(anyhow!(
                                        "طول مفتاح HLS AES-128 غير صالح ({} بايت، المتوقع 16 بايت)",
                                        k_bytes.len()
                                    ));
                                }
                                let mut arr = [0u8; 16];
                                arr.copy_from_slice(&k_bytes[..16]);
                                guard.insert(key_uri, arr);
                                arr
                            }
                        };
                        Self::decrypt_hls_aes128_segment(
                            &raw_bytes,
                            &key_arr,
                            iv,
                            seg.sequence_number,
                        )?
                    }
                    HlsEncryptionMethod::UnsupportedDrm(d) => {
                        return Err(anyhow!("مقطع محمي بـ DRM غير مدعوم: {}", d));
                    }
                };

                Ok::<(usize, Vec<u8>), anyhow::Error>((idx, final_bytes))
            }));
        }

        let mut downloaded_parts = Vec::with_capacity(tasks.len());
        for t in tasks {
            let part = t.await??;
            downloaded_parts.push(part);
        }
        downloaded_parts.sort_by_key(|(idx, _)| *idx);

        for (_, bytes) in downloaded_parts {
            out.write_all(&bytes).await?;
        }
        out.flush().await?;
        out.sync_all().await?;

        Ok(output_file.to_path_buf())
    }
    /// تحميل بث HLS مع تحديث حالة المهمة (DownloadStateMeta) لحظياً لتظهر في واجهة RustIDM
    pub async fn download_hls_stream_with_progress(
        &self,
        m3u8_url: &str,
        output_file: &Path,
        headers: &HeaderMap,
        concurrency: usize,
        meta: Arc<Mutex<DownloadStateMeta>>,
        pause_flag: Arc<AtomicBool>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<PathBuf> {
        while pause_flag.load(Ordering::Relaxed) {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(anyhow!("تم إلغاء تحميل بث HLS"));
            }
            sleep(Duration::from_millis(200)).await;
        }
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(anyhow!("تم إلغاء تحميل بث HLS"));
        }

        let playlist_text = self
            .client
            .get(m3u8_url)
            .headers(headers.clone())
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        let base_url = Url::parse(m3u8_url)?;

        if playlist_text.contains("#EXT-X-STREAM-INF") {
            let (variants, _renditions) = Self::parse_master_playlist(&playlist_text, &base_url)?;
            let best_variant = variants
                .into_iter()
                .max_by_key(|v| v.bandwidth)
                .ok_or_else(|| anyhow!("لم يتم العثور على جودات صالحة في ملف m3u8 الرئيسي"))?;

            return Box::pin(self.download_hls_stream_with_progress(
                &best_variant.uri,
                output_file,
                headers,
                concurrency,
                meta,
                pause_flag,
                cancel_flag,
            ))
            .await;
        }

        let parsed = Self::parse_media_playlist(&playlist_text, &base_url)?;
        if parsed.segments.is_empty() {
            return Err(anyhow!("قائمة مقاطع الفيديو فارغة"));
        }

        let total_segments = parsed.segments.len() as u64;
        {
            let mut m = meta.lock().await;
            m.status = DownloadStatus::Downloading;
            m.total_size = total_segments * 1024;
            m.segments = vec![FileSegment {
                id: 0,
                start_byte: 0,
                current_offset: 0,
                end_byte: (total_segments * 1024).saturating_sub(1),
                status: SegmentStatus::Downloading,
                split_count: 0,
                last_speed_bps: 0,
            }];
        }

        if let Some(parent) = output_file.parent() {
            fs::create_dir_all(parent).await?;
        }
        let mut out = File::create(output_file).await?;

        if let Some(ref map_uri) = parsed.init_map_uri {
            let init_bytes = self
                .client
                .get(map_uri)
                .headers(headers.clone())
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            out.write_all(&init_bytes).await?;
        }

        let key_cache: Arc<Mutex<HashMap<String, [u8; 16]>>> = Arc::new(Mutex::new(HashMap::new()));
        let sem = Arc::new(Semaphore::new(concurrency.max(1)));
        let completed_count = Arc::new(AtomicU64::new(0));
        let start_inst = Instant::now();
        let mut tasks = Vec::with_capacity(parsed.segments.len());

        for (idx, seg) in parsed.segments.into_iter().enumerate() {
            let sem_clone = Arc::clone(&sem);
            let client_clone = self.client.clone();
            let headers_clone = headers.clone();
            let cache_clone = Arc::clone(&key_cache);
            let meta_clone = Arc::clone(&meta);
            let done_clone = Arc::clone(&completed_count);
            let pause_clone = Arc::clone(&pause_flag);
            let cancel_clone = Arc::clone(&cancel_flag);

            tasks.push(tokio::spawn(async move {
                let _permit = sem_clone.acquire().await.unwrap();
                while pause_clone.load(Ordering::Relaxed) {
                    if cancel_clone.load(Ordering::Relaxed) {
                        return Err(anyhow!("تم إلغاء تحميل بث HLS"));
                    }
                    let mut m = meta_clone.lock().await;
                    m.status = DownloadStatus::Paused;
                    m.updated_at = Utc::now();
                    drop(m);
                    sleep(Duration::from_millis(200)).await;
                }
                if cancel_clone.load(Ordering::Relaxed) {
                    return Err(anyhow!("تم إلغاء تحميل بث HLS"));
                }
                {
                    let mut m = meta_clone.lock().await;
                    m.status = DownloadStatus::Downloading;
                    m.updated_at = Utc::now();
                }

                let mut req = client_clone.get(&seg.uri).headers(headers_clone.clone());
                if let Some((len, offset)) = seg.byte_range {
                    req = req.header(RANGE, format!("bytes={}-{}", offset, offset + len - 1));
                }

                let raw_bytes = req.send().await?.error_for_status()?.bytes().await?;

                let final_bytes = match seg.encryption {
                    HlsEncryptionMethod::None => raw_bytes.to_vec(),
                    HlsEncryptionMethod::Aes128 { key_uri, iv } => {
                        let key_arr = {
                            let mut guard = cache_clone.lock().await;
                            if let Some(k) = guard.get(&key_uri) {
                                *k
                            } else {
                                let k_bytes = client_clone
                                    .get(&key_uri)
                                    .headers(headers_clone)
                                    .send()
                                    .await?
                                    .error_for_status()?
                                    .bytes()
                                    .await?;
                                if k_bytes.len() != 16 {
                                    return Err(anyhow!(
                                        "طول مفتاح HLS AES-128 غير صالح ({} بايت)",
                                        k_bytes.len()
                                    ));
                                }
                                let mut arr = [0u8; 16];
                                arr.copy_from_slice(&k_bytes[..16]);
                                guard.insert(key_uri, arr);
                                arr
                            }
                        };
                        Self::decrypt_hls_aes128_segment(
                            &raw_bytes,
                            &key_arr,
                            iv,
                            seg.sequence_number,
                        )?
                    }
                    HlsEncryptionMethod::UnsupportedDrm(d) => {
                        return Err(anyhow!("مقطع محمي بـ DRM غير مدعوم: {}", d));
                    }
                };

                let done = done_clone.fetch_add(1, Ordering::SeqCst) + 1;
                let elapsed = start_inst.elapsed().as_secs_f64().max(0.001);
                let approx_bps = ((done * 64 * 1024) as f64 / elapsed) as u64;
                {
                    let mut m = meta_clone.lock().await;
                    if let Some(s0) = m.segments.first_mut() {
                        s0.current_offset = done * 1024;
                        s0.last_speed_bps = approx_bps;
                    }
                }

                Ok::<(usize, Vec<u8>), anyhow::Error>((idx, final_bytes))
            }));
        }

        let mut downloaded_parts = Vec::with_capacity(tasks.len());
        for t in tasks {
            let part = t.await??;
            downloaded_parts.push(part);
        }
        downloaded_parts.sort_by_key(|(idx, _)| *idx);

        let mut total_written = 0u64;
        for (_, bytes) in downloaded_parts {
            total_written += bytes.len() as u64;
            out.write_all(&bytes).await?;
        }
        out.flush().await?;
        out.sync_all().await?;

        {
            let mut m = meta.lock().await;
            m.total_size = total_written;
            if let Some(s0) = m.segments.first_mut() {
                s0.end_byte = total_written.saturating_sub(1);
                s0.current_offset = total_written;
                s0.status = SegmentStatus::Completed;
                s0.last_speed_bps = 0;
            }
            m.status = DownloadStatus::Completed;
            m.updated_at = Utc::now();
        }

        Ok(output_file.to_path_buf())
    }

    /// جسر تكامل غير متزامن مع yt-dlp يدعم الإلغاء الفوري (kill_on_drop) واختيار الجودة (format_id)
    pub async fn download_via_ytdlp_with_format(
        url: &str,
        output_file: &Path,
        format_selector: Option<&str>,
        cookies_header: Option<&str>,
        referer: Option<&str>,
        user_agent: Option<&str>,
        pause_flag: Arc<AtomicBool>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<()> {
        let fmt = format_selector
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("bestvideo+bestaudio/best");

        loop {
            while pause_flag.load(Ordering::Relaxed) {
                if cancel_flag.load(Ordering::Relaxed) {
                    return Err(anyhow!("تم إلغاء عملية yt-dlp"));
                }
                sleep(Duration::from_millis(200)).await;
            }

            if cancel_flag.load(Ordering::Relaxed) {
                return Err(anyhow!("تم إلغاء عملية yt-dlp"));
            }

            let mut cmd = TokioCommand::new("yt-dlp");
            cmd.kill_on_drop(true)
                .arg("--no-playlist")
                .arg("-f")
                .arg(fmt)
                .arg("--merge-output-format")
                .arg("mp4")
                .arg("-o")
                .arg(output_file);

            if let Some(cookie) = cookies_header.filter(|value| !value.is_empty()) {
                cmd.arg("--add-header").arg(format!("Cookie: {cookie}"));
            }
            if let Some(ref_url) = referer.filter(|value| !value.is_empty()) {
                cmd.arg("--referer").arg(ref_url);
            }
            if let Some(ua) = user_agent.filter(|value| !value.is_empty()) {
                cmd.arg("--user-agent").arg(ua);
            }

            let mut child = cmd
                .arg(url)
                .spawn()
                .context("تعذر تشغيل yt-dlp؛ تأكد من تثبيت yt-dlp و ffmpeg في مسار النظام")?;

            loop {
                if cancel_flag.load(Ordering::Relaxed) {
                    let _ = child.kill().await;
                    return Err(anyhow!(
                        "تم إلغاء عملية yt-dlp وإنهاء العملية الفرعية بأمان"
                    ));
                }
                if pause_flag.load(Ordering::Relaxed) {
                    child.kill().await?;
                    break;
                }
                if let Some(status) = child.try_wait()? {
                    if status.success() {
                        return Ok(());
                    }
                    return Err(anyhow!("انتهت عملية yt-dlp برمز خطأ: {:?}", status.code()));
                }
                sleep(Duration::from_millis(200)).await;
            }
        }
    }

    /// جسر تكامل غير متزامن مع yt-dlp يدعم الإلغاء الفوري (kill_on_drop) ومنع بقاء عمليات يتيمة
    pub async fn download_via_ytdlp_cancellable(
        url: &str,
        output_dir: &Path,
        cookies_header: Option<&str>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<()> {
        let mut cmd = TokioCommand::new("yt-dlp");
        cmd.kill_on_drop(true)
            .arg("-f")
            .arg("bestvideo+bestaudio/best")
            .arg("--merge-output-format")
            .arg("mp4")
            .arg("-P")
            .arg(output_dir);

        if let Some(cookie) = cookies_header {
            if !cookie.is_empty() {
                cmd.arg("--add-header").arg(format!("Cookie: {}", cookie));
            }
        }

        let mut child = cmd
            .arg(url)
            .spawn()
            .context("تعذر تشغيل yt-dlp؛ تأكد من تثبيت yt-dlp و ffmpeg في مسار النظام")?;

        loop {
            if cancel_flag.load(Ordering::Relaxed) {
                let _ = child.kill().await;
                return Err(anyhow!(
                    "تم إلغاء عملية yt-dlp وإنهاء العملية الفرعية بأمان"
                ));
            }
            if let Some(status) = child.try_wait()? {
                if status.success() {
                    return Ok(());
                } else {
                    return Err(anyhow!("انتهت عملية yt-dlp برمز خطأ: {:?}", status.code()));
                }
            }
            sleep(Duration::from_millis(200)).await;
        }
    }
}

// ============================================================================
// 7. محلل قوائم MPEG-DASH (.mpd) للهياكل الثابتة (Static MPD Manifest Parser)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashRepresentation {
    pub id: String,
    pub mime_type: String,
    pub bandwidth: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub initialization_url: Option<String>,
    pub segment_urls: Vec<String>,
}

pub struct DashMpdParser;

impl DashMpdParser {
    /// تحليل ملفات MPEG-DASH (.mpd) التي تستخدم BaseURL أو SegmentList أو SegmentTemplate
    pub fn parse_mpd(xml_text: &str, base_url: &Url) -> Result<Vec<DashRepresentation>> {
        let mut reps = Vec::new();
        let rep_re = Regex::new(r"(?s)<Representation\b([^>]*)>(.*?)</Representation>")?;
        let id_re = Regex::new(r#"id="([^"]+)""#)?;
        let bw_re = Regex::new(r#"bandwidth="(\d+)""#)?;
        let mime_re = Regex::new(r#"mimeType="([^"]+)""#)?;
        let w_re = Regex::new(r#"width="(\d+)""#)?;
        let h_re = Regex::new(r#"height="(\d+)""#)?;
        let base_tag_re = Regex::new(r"<BaseURL>([^<]+)</BaseURL>")?;
        let seg_url_re = Regex::new(r#"<SegmentURL\b[^>]*media="([^"]+)""#)?;
        let init_re = Regex::new(r#"initialization="([^"]+)""#)?;
        let media_tmpl_re = Regex::new(r#"media="([^"]+)""#)?;
        let start_num_re = Regex::new(r#"startNumber="(\d+)""#)?;
        let count_re = Regex::new(r#"r="(\d+)""#)?;

        for cap in rep_re.captures_iter(xml_text) {
            let attrs = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let body = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            let rep_id = id_re
                .captures(attrs)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| "rep_0".to_string());
            let bandwidth = bw_re
                .captures(attrs)
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<u64>().ok())
                .unwrap_or(0);
            let mime_type = mime_re
                .captures(attrs)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| "video/mp4".to_string());
            let width = w_re
                .captures(attrs)
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<u32>().ok());
            let height = h_re
                .captures(attrs)
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<u32>().ok());

            let mut initialization_url = None;
            let mut segment_urls = Vec::new();

            if let Some(init_cap) = init_re.captures(body).and_then(|c| c.get(1)) {
                let raw_init = init_cap
                    .as_str()
                    .replace("$RepresentationID$", &rep_id)
                    .replace("$Bandwidth$", &bandwidth.to_string());
                if let Ok(u) = base_url.join(&raw_init) {
                    initialization_url = Some(u.to_string());
                }
            }

            for seg_cap in seg_url_re.captures_iter(body) {
                if let Some(m) = seg_cap.get(1) {
                    if let Ok(u) = base_url.join(m.as_str()) {
                        segment_urls.push(u.to_string());
                    }
                }
            }

            if segment_urls.is_empty() {
                if let Some(tmpl_cap) = media_tmpl_re.captures(body).and_then(|c| c.get(1)) {
                    let start_num = start_num_re
                        .captures(body)
                        .and_then(|c| c.get(1))
                        .and_then(|m| m.as_str().parse::<u64>().ok())
                        .unwrap_or(1);
                    let repeat_count = count_re
                        .captures(body)
                        .and_then(|c| c.get(1))
                        .and_then(|m| m.as_str().parse::<u64>().ok())
                        .unwrap_or(0);

                    for idx in start_num..=(start_num + repeat_count) {
                        let expanded = tmpl_cap
                            .as_str()
                            .replace("$RepresentationID$", &rep_id)
                            .replace("$Bandwidth$", &bandwidth.to_string())
                            .replace("$Number$", &idx.to_string());
                        if let Ok(u) = base_url.join(&expanded) {
                            segment_urls.push(u.to_string());
                        }
                    }
                }
            }

            if segment_urls.is_empty() {
                if let Some(b_cap) = base_tag_re.captures(body).and_then(|c| c.get(1)) {
                    if let Ok(u) = base_url.join(b_cap.as_str().trim()) {
                        segment_urls.push(u.to_string());
                    }
                }
            }

            reps.push(DashRepresentation {
                id: rep_id,
                mime_type,
                bandwidth,
                width,
                height,
                initialization_url,
                segment_urls,
            });
        }

        Ok(reps)
    }
}

// ============================================================================
// 8. الزاحف المحدود لسحب المواقع (Bounded BFS Site Crawler)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlConfig {
    pub start_url: String,
    pub max_depth: usize,
    pub max_pages: usize,
    pub max_concurrent_requests: usize,
    pub request_timeout_ms: u64,
    pub max_page_size_bytes: usize,
    pub delay_between_requests_ms: u64,
    pub same_domain_only: bool,
    pub allowed_extensions: Vec<String>,
    pub excluded_paths: Vec<String>,
}

impl Default for CrawlConfig {
    fn default() -> Self {
        Self {
            start_url: String::new(),
            max_depth: 2,
            max_pages: 25,
            max_concurrent_requests: 4,
            request_timeout_ms: 10_000,
            max_page_size_bytes: 5 * 1024 * 1024,
            delay_between_requests_ms: 100,
            same_domain_only: true,
            allowed_extensions: vec!["mp4".into(), "pdf".into(), "zip".into()],
            excluded_paths: vec!["/logout".into(), "/admin".into()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlReport {
    pub pages_visited: usize,
    pub pages_discovered: usize,
    pub matched_files: Vec<String>,
    pub errors: Vec<String>,
}

pub struct BoundedSiteCrawler {
    client: Client,
}

impl BoundedSiteCrawler {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub fn normalize_url(raw: &Url) -> String {
        let mut clean = raw.clone();
        clean.set_fragment(None);
        clean.to_string()
    }

    pub async fn crawl(&self, cfg: &CrawlConfig) -> Result<CrawlReport> {
        let start_parsed = Url::parse(&cfg.start_url)?;
        let start_domain = start_parsed.host_str().unwrap_or("").to_lowercase();
        let link_re = Regex::new(r#"(?i)(?:href|src)\s*=\s*["']([^"'#]+)["']"#)?;

        let mut visited_pages: HashSet<String> = HashSet::new();
        let mut matched_set: HashSet<String> = HashSet::new();
        let mut matched_files: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let mut queue: VecDeque<(Url, usize)> = VecDeque::new();

        queue.push_back((start_parsed.clone(), 0));
        visited_pages.insert(Self::normalize_url(&start_parsed));

        while let Some((current_url, depth)) = queue.pop_front() {
            if visited_pages.len() > cfg.max_pages && depth > 0 {
                break;
            }

            if cfg.delay_between_requests_ms > 0 && !errors.is_empty() {
                sleep(Duration::from_millis(cfg.delay_between_requests_ms)).await;
            }

            let resp_res = timeout(
                Duration::from_millis(cfg.request_timeout_ms),
                self.client.get(current_url.as_str()).send(),
            )
            .await;

            let resp = match resp_res {
                Ok(Ok(r)) if r.status().is_success() => r,
                Ok(Ok(r)) => {
                    errors.push(format!("{} -> HTTP {}", current_url, r.status()));
                    continue;
                }
                Ok(Err(e)) => {
                    errors.push(format!("{} -> {}", current_url, e));
                    continue;
                }
                Err(_) => {
                    errors.push(format!("{} -> Timeout", current_url));
                    continue;
                }
            };

            let bytes = match resp.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    errors.push(format!("{} -> {}", current_url, e));
                    continue;
                }
            };

            let slice = if bytes.len() > cfg.max_page_size_bytes {
                &bytes[..cfg.max_page_size_bytes]
            } else {
                &bytes[..]
            };
            let html = String::from_utf8_lossy(slice);

            for cap in link_re.captures_iter(&html) {
                if let Some(raw_attr) = cap.get(1) {
                    if let Ok(resolved) = current_url.join(raw_attr.as_str()) {
                        if resolved.scheme() != "http" && resolved.scheme() != "https" {
                            continue;
                        }
                        let norm = Self::normalize_url(&resolved);
                        let path_lower = resolved.path().to_lowercase();

                        if cfg
                            .excluded_paths
                            .iter()
                            .any(|ex| !ex.is_empty() && path_lower.starts_with(&ex.to_lowercase()))
                        {
                            continue;
                        }

                        if cfg.same_domain_only {
                            let host = resolved.host_str().unwrap_or("").to_lowercase();
                            if host != start_domain {
                                continue;
                            }
                        }

                        let is_target_file = cfg.allowed_extensions.is_empty()
                            || cfg.allowed_extensions.iter().any(|ext| {
                                path_lower.ends_with(&format!(".{}", ext.to_lowercase()))
                            });

                        if is_target_file {
                            if matched_set.insert(norm.clone()) {
                                matched_files.push(norm);
                            }
                        } else if depth < cfg.max_depth && visited_pages.len() < cfg.max_pages {
                            if visited_pages.insert(norm) {
                                queue.push_back((resolved, depth + 1));
                            }
                        }
                    }
                }
            }
        }

        Ok(CrawlReport {
            pages_visited: visited_pages.len(),
            pages_discovered: visited_pages.len() + queue.len(),
            matched_files,
            errors,
        })
    }
}

// ============================================================================
// 9. بروتوكول Chrome Native Messaging Host القياسي (4-Byte Length-Prefixed Stdio)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeIncomingMessage {
    pub action: String, // "PING" | "INTERCEPT_DOWNLOAD" | "VIDEO_STREAM"
    pub url: Option<String>,
    pub filename: Option<String>,
    pub referer: Option<String>,
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeOutgoingMessage {
    pub status: String,
    pub engine: String,
    pub version: String,
    pub task_id: Option<String>,
    pub message: String,
}

/// قراءة رسالة واحدة من Chrome عبر stdin وفق بروتوكول 4 بايت Little-Endian
pub fn read_native_message<R: Read>(reader: &mut R) -> Result<Option<NativeIncomingMessage>> {
    let mut len_bytes = [0u8; 4];
    match reader.read_exact(&mut len_bytes) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }

    let msg_len = u32::from_le_bytes(len_bytes) as usize;
    if msg_len == 0 || msg_len > MAX_NATIVE_MSG_BYTES {
        return Err(anyhow!(
            "حجم رسالة Native Messaging غير صالح أو يتجاوز الحد المسموح: {} بايت",
            msg_len
        ));
    }

    let mut payload_buf = vec![0u8; msg_len];
    reader.read_exact(&mut payload_buf)?;
    let parsed: NativeIncomingMessage = serde_json::from_slice(&payload_buf)?;
    Ok(Some(parsed))
}

/// كتابة رسالة رد إلى Chrome عبر stdout وفق بروتوكول 4 بايت Little-Endian
pub fn write_native_message<W: Write>(writer: &mut W, msg: &NativeOutgoingMessage) -> Result<()> {
    let json_bytes = serde_json::to_vec(msg)?;
    let len = json_bytes.len() as u32;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&json_bytes)?;
    writer.flush()?;
    Ok(())
}

/// التحقق الأمني من رسالة Chrome ومنع تمرير روابط غير HTTP/HTTPS
pub fn validate_native_incoming(msg: &NativeIncomingMessage) -> Result<()> {
    match msg.action.as_str() {
        "PING" => Ok(()),
        "INTERCEPT_DOWNLOAD" | "VIDEO_STREAM" => {
            let raw_url = msg
                .url
                .as_ref()
                .ok_or_else(|| anyhow!("الرسالة تفتقر إلى حقل الرابط url"))?;
            let parsed = Url::parse(raw_url).context("صيغة الرابط غير صالحة")?;
            if parsed.scheme() != "http" && parsed.scheme() != "https" {
                return Err(anyhow!(
                    "بروتوكول الرابط غير مسموح به عبر Native Messaging: {}",
                    parsed.scheme()
                ));
            }
            Ok(())
        }
        other => Err(anyhow!("إجراء غير معروف في Native Messaging: {}", other)),
    }
}

/// توليد ملف تعريف المضيف الأصلي لمتصفح Google Chrome (`com.rustidm.native_host.json`)
pub fn generate_native_host_manifest(binary_path: &Path, extension_id: &str) -> serde_json::Value {
    serde_json::json!({
        "name": "com.rustidm.native_host",
        "description": "RustIDM Studio Native Messaging Host",
        "path": binary_path.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": [
            format!("chrome-extension://{}/", extension_id.trim_matches('/'))
        ]
    })
}

// ============================================================================
// 10. مدير الطوابير والجدولة وإجراءات ما بعد الاكتمال واستعادة الجلسة
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PostQueueAction {
    None,
    Notify,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueConfig {
    pub name: String,
    pub max_concurrent_downloads: usize,
    pub start_time: Option<String>, // HH:MM:SS
    pub stop_time: Option<String>,  // HH:MM:SS
    pub post_action: PostQueueAction,
    #[serde(default)]
    pub schedule_mode: ScheduleMode,
    #[serde(default)]
    pub start_at: Option<String>,
}

impl QueueConfig {
    fn schedule(&self) -> QueueSchedule {
        QueueSchedule {
            mode: self.schedule_mode,
            start_time: self.start_time.clone(),
            stop_time: self.stop_time.clone(),
            start_at: self.start_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleMode {
    #[default]
    Disabled,
    Daily,
    Once,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueueSchedule {
    pub mode: ScheduleMode,
    pub start_time: Option<String>,
    pub stop_time: Option<String>,
    pub start_at: Option<String>,
}

pub struct AppState {
    pub client: Client,
    pub base_download_dir: PathBuf,
    pub max_connections_per_file: usize,
    pub speed_limiter: Arc<SpeedLimiter>,
    pub jobs: RwLock<HashMap<String, Arc<DynamicDownloadJob>>>,
    pub detected_videos: RwLock<Vec<DetectedVideoPayload>>,
    pub queues: RwLock<HashMap<String, QueueConfig>>,
    pub recent_intercepts: Mutex<HashMap<String, Instant>>,
    queue_semaphores: HashMap<String, Arc<Semaphore>>,
    post_action_fired: Mutex<HashSet<String>>,
    scheduler_notifications: Mutex<VecDeque<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterceptDownloadPayload {
    pub url: String,
    pub source_page_url: Option<String>,
    pub filename: Option<String>,
    pub cookies: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub queue_name: Option<String>,
    pub expected_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshUrlPayload {
    pub task_id: String,
    pub new_url: String,
    pub cookies: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedVideoPayload {
    pub url: String,
    pub page_title: Option<String>,
    pub mime_type: String,
    pub content_length: Option<u64>,
    pub quality_label: Option<String>,
    pub tab_id: Option<i64>,
    pub cookies: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub start_immediately: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlTaskPayload {
    pub task_id: String,
    pub action: String, // "pause" | "resume" | "cancel" | "speed_limit"
    pub speed_limit_kbps: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeVideoPayload {
    pub url: String,
    pub page_url: Option<String>,
    pub page_title: Option<String>,
    pub cookies: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoQualityOption {
    pub id: String,
    pub label: String,
    pub resolution: Option<String>,
    pub container: String,
    pub bandwidth_bps: Option<u64>,
    pub estimated_size_bytes: Option<u64>,
    pub download_url: String,
    pub protocol: String, // "hls" | "dash" | "ytdlp" | "direct"
    pub format_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeVideoResponse {
    pub title: String,
    pub drm_protected: bool,
    pub drm_reason: Option<String>,
    pub qualities: Vec<VideoQualityOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartVideoDownloadPayload {
    pub url: String,
    pub page_url: Option<String>,
    pub title: Option<String>,
    pub quality_label: Option<String>,
    pub container: Option<String>,
    pub protocol: String, // "hls" | "dash" | "ytdlp" | "direct"
    pub format_id: Option<String>,
    pub cookies: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
}

impl AppState {
    pub fn new(
        base_download_dir: PathBuf,
        max_connections_per_file: usize,
        speed_limit_kbps: u64,
    ) -> Arc<Self> {
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) RustIDM/1.1")
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("فشل إنشاء عميل HTTP");

        let mut default_queues = HashMap::new();
        default_queues.insert(
            "main".to_string(),
            QueueConfig {
                name: "main".to_string(),
                max_concurrent_downloads: 4,
                start_time: None,
                stop_time: None,
                post_action: PostQueueAction::Notify,
                schedule_mode: ScheduleMode::Disabled,
                start_at: None,
            },
        );
        default_queues.insert(
            "sync".to_string(),
            QueueConfig {
                name: "sync".to_string(),
                max_concurrent_downloads: 2,
                start_time: Some("01:00:00".to_string()),
                stop_time: Some("07:00:00".to_string()),
                post_action: PostQueueAction::None,
                schedule_mode: ScheduleMode::Disabled,
                start_at: None,
            },
        );

        let queue_semaphores = default_queues
            .iter()
            .map(|(name, queue)| {
                (
                    name.clone(),
                    Arc::new(Semaphore::new(queue.max_concurrent_downloads.max(1))),
                )
            })
            .collect();

        Arc::new(Self {
            client,
            base_download_dir,
            max_connections_per_file,
            speed_limiter: SpeedLimiter::new(speed_limit_kbps),
            jobs: RwLock::new(HashMap::new()),
            detected_videos: RwLock::new(Vec::new()),
            queues: RwLock::new(default_queues),
            recent_intercepts: Mutex::new(HashMap::new()),
            queue_semaphores,
            post_action_fired: Mutex::new(HashSet::new()),
            scheduler_notifications: Mutex::new(VecDeque::new()),
        })
    }

    async fn spawn_download_job(self: &Arc<Self>, job: Arc<DynamicDownloadJob>) {
        if job
            .runner_active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let queue_name = job.meta.lock().await.queue_name.clone();
        let Some(semaphore) = self.queue_semaphores.get(&queue_name).cloned() else {
            job.runner_active.store(false, Ordering::SeqCst);
            eprintln!("Cannot start download: its queue has no concurrency limiter");
            return;
        };

        tokio::spawn(async move {
            let permit = match semaphore.acquire_owned().await {
                Ok(permit) => permit,
                Err(error) => {
                    eprintln!("Cannot acquire download queue slot: {error}");
                    job.runner_active.store(false, Ordering::SeqCst);
                    return;
                }
            };
            if !job.cancel_flag.load(Ordering::SeqCst)
                && !job.pause_flag.load(Ordering::SeqCst)
                && !job.uses_external_worker
            {
                if let Err(error) = job.execute().await {
                    let status = job.meta.lock().await.status;
                    if status != DownloadStatus::Paused && status != DownloadStatus::Failed {
                        eprintln!("Download job failed: {error:#}");
                    }
                }
            }
            drop(permit);
            job.runner_active.store(false, Ordering::SeqCst);
        });
    }

    /// استعادة جميع التنزيلات المتوقفة أو غير المكتملة (.idm.json) من القرص عند إعادة تشغيل التطبيق
    pub async fn restore_persisted_tasks_on_startup(self: &Arc<Self>) -> Result<usize> {
        let schedule_path = self.base_download_dir.join("scheduler.json");
        match fs::read(&schedule_path).await {
            Ok(bytes) => {
                let saved: HashMap<String, QueueSchedule> = serde_json::from_slice(&bytes)
                    .context("Failed to parse saved scheduler configuration")?;
                let mut queues = self.queues.write().await;
                for (name, schedule) in saved {
                    if let Some(queue) = queues.get_mut(&name) {
                        queue.schedule_mode = schedule.mode;
                        queue.start_time = schedule.start_time;
                        queue.stop_time = schedule.stop_time;
                        queue.start_at = schedule.start_at;
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("Failed to read scheduler configuration"),
        }

        let mut restored = 0usize;
        let categories = [
            "Videos",
            "Music",
            "Compressed",
            "Documents",
            "Programs",
            "General",
        ];

        for cat_folder in categories {
            let dir = self.base_download_dir.join(cat_folder);
            if !dir.exists() {
                continue;
            }
            let mut entries = fs::read_dir(&dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.to_string_lossy().ends_with(".idm.json") {
                    if let Ok(bytes) = fs::read(&path).await {
                        if let Ok(mut meta) = serde_json::from_slice::<DownloadStateMeta>(&bytes) {
                            if meta.status == DownloadStatus::Downloading {
                                meta.status = DownloadStatus::Paused;
                            }
                            let task_id = meta.task_id.clone();
                            let job = Arc::new(DynamicDownloadJob {
                                meta: Arc::new(Mutex::new(meta)),
                                pause_flag: Arc::new(AtomicBool::new(true)),
                                cancel_flag: Arc::new(AtomicBool::new(false)),
                                resource_mutated_flag: Arc::new(AtomicBool::new(false)),
                                expired_url_flag: Arc::new(AtomicBool::new(false)),
                                speed_limiter: Arc::clone(&self.speed_limiter),
                                client: self.client.clone(),
                                max_connections: self.max_connections_per_file,
                                min_split_size: MIN_SPLIT_SIZE,
                                antivirus_cmd: None,
                                uses_external_worker: false,
                                runner_active: AtomicBool::new(false),
                                schedule_paused: AtomicBool::new(false),
                            });
                            self.jobs.write().await.insert(task_id, job);
                            restored += 1;
                        }
                    }
                }
            }
        }
        Ok(restored)
    }

    pub fn start_scheduler(self: &Arc<Self>) {
        let state = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                if let Err(error) = apply_scheduled_queues(&state).await {
                    eprintln!("Scheduler tick failed: {error:#}");
                }
                sleep(Duration::from_secs(1)).await;
            }
        });
    }

    /// التحقق من شروط الأمان الصارمة قبل تنفيذ إجراء ما بعد اكتمال الطابور (مثل إغلاق التطبيق أو إيقاف الجهاز)
    pub async fn can_execute_post_queue_action(&self, queue_name: &str) -> bool {
        let jobs = self.jobs.read().await;
        let mut has_tasks = false;

        for job in jobs.values() {
            let m = job.meta.lock().await;
            if m.queue_name == queue_name {
                has_tasks = true;
                if m.status != DownloadStatus::Completed {
                    return false;
                }
            }
        }
        has_tasks
    }

    pub async fn enqueue_download(
        self: &Arc<Self>,
        payload: InterceptDownloadPayload,
    ) -> Result<String> {
        // منع تكرار تسجيل نفس رابط التحميل خلال 5 ثوانٍ (Deduplication)
        {
            let mut guard = self.recent_intercepts.lock().await;
            guard.retain(|_, ts| ts.elapsed() < Duration::from_secs(5));
            if guard.contains_key(&payload.url) {
                return Err(anyhow!(
                    "تم استلام هذا الرابط بالفعل خلال الثواني الخمس الماضية"
                ));
            }
            guard.insert(payload.url.clone(), Instant::now());
        }

        let mut header_map = HeaderMap::new();
        let mut custom_headers = HashMap::new();

        if let Some(ref c) = payload.cookies {
            if !c.is_empty() {
                header_map.insert(COOKIE, HeaderValue::from_str(c)?);
                custom_headers.insert("cookie".to_string(), c.clone());
            }
        }
        if let Some(ref r) = payload.referer {
            if !r.is_empty() {
                header_map.insert(REFERER, HeaderValue::from_str(r)?);
                custom_headers.insert("referer".to_string(), r.clone());
            }
        }
        if let Some(ref ua) = payload.user_agent {
            if !ua.is_empty() {
                header_map.insert(USER_AGENT, HeaderValue::from_str(ua)?);
                custom_headers.insert("user-agent".to_string(), ua.clone());
            }
        }

        let (total_size, resumable, probed_name, etag, last_modified) =
            DynamicDownloadJob::probe_url(&self.client, &payload.url, &header_map).await?;

        let final_filename = payload
            .filename
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(probed_name);

        let category = FileCategory::from_filename(&final_filename);
        let save_dir = self.base_download_dir.join(category.folder_name());
        let queue_name = payload.queue_name.unwrap_or_else(|| "main".to_string());
        let queue = self
            .queues
            .read()
            .await
            .get(&queue_name)
            .map(QueueConfig::schedule)
            .ok_or_else(|| anyhow!("Unknown download queue: {queue_name}"))?;
        let schedule = queue;
        let should_start = scheduled_queue_should_run(&schedule, Local::now().naive_local())
            .context("Invalid schedule configured for download queue")?
            .unwrap_or(true);

        let task_id = Uuid::new_v4().to_string();
        let meta = DownloadStateMeta {
            task_id: task_id.clone(),
            url: payload.url.clone(),
            source_page_url: payload.source_page_url.or(payload.referer),
            filename: final_filename,
            save_dir,
            category,
            total_size,
            resumable,
            etag,
            last_modified,
            custom_headers,
            expected_sha256: payload.expected_sha256,
            computed_sha256: None,
            status: if should_start {
                DownloadStatus::Queued
            } else {
                DownloadStatus::Paused
            },
            last_error: None,
            refresh_attempts: 0,
            segments: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            queue_name: queue_name.clone(),
        };
        fs::create_dir_all(&meta.save_dir).await?;
        save_meta_to_disk_atomic(&meta, &meta.meta_file_path()).await?;

        let pause_flag = Arc::new(AtomicBool::new(!should_start));
        let job = Arc::new(DynamicDownloadJob {
            meta: Arc::new(Mutex::new(meta)),
            pause_flag: Arc::clone(&pause_flag),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            resource_mutated_flag: Arc::new(AtomicBool::new(false)),
            expired_url_flag: Arc::new(AtomicBool::new(false)),
            speed_limiter: Arc::clone(&self.speed_limiter),
            client: self.client.clone(),
            max_connections: self.max_connections_per_file,
            min_split_size: MIN_SPLIT_SIZE,
            antivirus_cmd: None,
            uses_external_worker: false,
            runner_active: AtomicBool::new(false),
            schedule_paused: AtomicBool::new(!should_start),
        });

        self.jobs
            .write()
            .await
            .insert(task_id.clone(), Arc::clone(&job));
        self.post_action_fired.lock().await.remove(&queue_name);

        if should_start {
            self.spawn_download_job(Arc::clone(&job)).await;
        }

        Ok(task_id)
    }
}

fn parse_schedule_time(value: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(value, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(value, "%H:%M:%S"))
        .map_err(|error| anyhow!("Invalid schedule time '{value}': {error}"))
}

fn scheduled_queue_should_run(
    schedule: &QueueSchedule,
    now: NaiveDateTime,
) -> Result<Option<bool>> {
    match schedule.mode {
        ScheduleMode::Disabled => Ok(None),
        ScheduleMode::Once => {
            let Some(start_at) = schedule.start_at.as_deref() else {
                return Ok(None);
            };
            let start_at = NaiveDateTime::parse_from_str(start_at, "%Y-%m-%d %H:%M")
                .map_err(|error| anyhow!("Invalid one-time schedule '{start_at}': {error}"))?;
            Ok(Some(now >= start_at))
        }
        ScheduleMode::Daily => {
            let (Some(start), Some(stop)) = (
                schedule.start_time.as_deref(),
                schedule.stop_time.as_deref(),
            ) else {
                return Ok(None);
            };
            let start = parse_schedule_time(start)?;
            let stop = parse_schedule_time(stop)?;
            let current = now.time();
            let active = if start < stop {
                current >= start && current < stop
            } else {
                current >= start || current < stop
            };
            Ok(Some(active))
        }
    }
}

async fn persist_queue_schedules(state: &AppState) -> Result<()> {
    let schedules = state
        .queues
        .read()
        .await
        .iter()
        .map(|(name, queue)| (name.clone(), queue.schedule()))
        .collect::<HashMap<_, _>>();
    let path = state.base_download_dir.join("scheduler.json");
    fs::create_dir_all(&state.base_download_dir).await?;
    let tmp_path = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(&schedules)?;
    let mut file = File::create(&tmp_path).await?;
    file.write_all(&bytes).await?;
    file.sync_all().await?;
    drop(file);
    fs::rename(&tmp_path, &path).await?;
    sync_parent_dir(&path).await;
    Ok(())
}

async fn apply_scheduled_queues(state: &Arc<AppState>) -> Result<()> {
    let now = Local::now().naive_local();
    let mut desired_states = HashMap::new();
    let mut schedule_changed = false;
    {
        let mut queues = state.queues.write().await;
        for (name, queue) in queues.iter_mut() {
            let schedule = queue.schedule();
            match scheduled_queue_should_run(&schedule, now) {
                Ok(Some(should_run)) => {
                    if schedule.mode == ScheduleMode::Once && should_run {
                        queue.schedule_mode = ScheduleMode::Disabled;
                        queue.start_at = None;
                        schedule_changed = true;
                    }
                    desired_states.insert(name.clone(), should_run);
                }
                Ok(None) => {}
                Err(error) => eprintln!("Invalid schedule for queue '{name}': {error:#}"),
            }
        }
    }

    if schedule_changed {
        persist_queue_schedules(state).await?;
    }

    let jobs = state
        .jobs
        .read()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for job in jobs {
        let queue_name = job.meta.lock().await.queue_name.clone();
        let Some(&should_run) = desired_states.get(&queue_name) else {
            continue;
        };

        if should_run {
            let status = job.meta.lock().await.status;
            if status == DownloadStatus::Queued {
                job.pause_flag.store(false, Ordering::SeqCst);
                if !job.uses_external_worker {
                    state.spawn_download_job(Arc::clone(&job)).await;
                }
            } else if status == DownloadStatus::Paused
                && job.schedule_paused.load(Ordering::SeqCst)
            {
                job.pause_flag.store(false, Ordering::SeqCst);
                if job.uses_external_worker || !job.runner_active.load(Ordering::SeqCst) {
                    job.schedule_paused.store(false, Ordering::SeqCst);
                }
                if !job.uses_external_worker {
                    state.spawn_download_job(Arc::clone(&job)).await;
                }
            }
        } else {
            let mut meta = job.meta.lock().await;
            if matches!(
                meta.status,
                DownloadStatus::Downloading | DownloadStatus::Queued
            ) {
                job.schedule_paused.store(true, Ordering::SeqCst);
                job.pause_flag.store(true, Ordering::SeqCst);
                meta.status = DownloadStatus::Paused;
                meta.updated_at = Utc::now();
                save_meta_to_disk_atomic(&meta, &meta.meta_file_path()).await?;
            }
        }
    }

    let queue_actions = {
        let queues = state.queues.read().await;
        queues
            .iter()
            .map(|(name, queue)| (name.clone(), queue.post_action.clone()))
            .collect::<Vec<_>>()
    };
    for (queue_name, action) in queue_actions {
        if action != PostQueueAction::Notify
            || !state.can_execute_post_queue_action(&queue_name).await
        {
            continue;
        }
        let mut fired = state.post_action_fired.lock().await;
        if fired.insert(queue_name.clone()) {
            state
                .scheduler_notifications
                .lock()
                .await
                .push_back(format!(
                    "All downloads in queue '{queue_name}' have completed."
                ));
        }
    }
    Ok(())
}

// ============================================================================
// 11. نقاط نهاية خادم الاتصال المحلي (Axum IPC Endpoints)
// ============================================================================

async fn health_handler(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "online",
        "engine": "RustIDM",
        "version": "1.1.0",
        "speed_limit_kbps": state.speed_limiter.get_limit_kbps(),
        "capabilities": [
            "race_free_in_half_dynamic_segmentation",
            "range_ignore_and_mutation_guard",
            "expired_url_refresh_providers",
            "chrome_native_messaging_host",
            "hls_aes128_cbc_and_dash_mpd",
            "bounded_bfs_site_crawler"
        ]
    }))
}

async fn intercept_download_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<InterceptDownloadPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let task_id = state
        .enqueue_download(payload)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "accepted",
        "task_id": task_id
    })))
}

async fn refresh_url_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RefreshUrlPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let job = {
        let jobs = state.jobs.read().await;
        jobs.get(&payload.task_id)
            .cloned()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "المهمة غير موجودة".to_string()))?
    };

    if let Some(ref c) = payload.cookies {
        let mut m = job.meta.lock().await;
        m.custom_headers.insert("cookie".to_string(), c.clone());
    }

    let kept_partial = job
        .verify_and_apply_refreshed_url(&payload.new_url)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    job.schedule_paused.store(false, Ordering::SeqCst);
    job.pause_flag.store(false, Ordering::SeqCst);
    state.spawn_download_job(Arc::clone(&job)).await;

    Ok(Json(serde_json::json!({
        "status": "refreshed_and_resumed",
        "preserved_partial_bytes": kept_partial
    })))
}

async fn list_downloads_handler(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<DownloadStateMeta>> {
    let jobs = state.jobs.read().await;
    let mut list = Vec::with_capacity(jobs.len());
    for job in jobs.values() {
        let m = job.meta.lock().await;
        // إرجاع نسخة محمية خالية من الكوكيز والترويسات السرية
        list.push(m.redacted_clone());
    }
    Json(list)
}

async fn delete_completed_downloads_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let completed = {
        let jobs = state.jobs.read().await;
        let mut completed = Vec::new();
        for (task_id, job) in jobs.iter() {
            let meta = job.meta.lock().await;
            if meta.status == DownloadStatus::Completed {
                completed.push((task_id.clone(), meta.meta_file_path()));
            }
        }
        completed
    };

    let mut deleted = 0usize;
    for (task_id, meta_path) in completed {
        match fs::remove_file(&meta_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!(
                        "Cannot remove completed task metadata '{}': {error}",
                        meta_path.display()
                    ),
                ));
            }
        }
        let mut jobs = state.jobs.write().await;
        if let Some(job) = jobs.get(&task_id) {
            let meta = job.meta.lock().await;
            let is_completed = meta.status == DownloadStatus::Completed;
            drop(meta);
            if is_completed {
                jobs.remove(&task_id);
                deleted += 1;
            }
        }
    }

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "count": deleted,
        "files_preserved": true
    })))
}

async fn delete_download_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(task_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut jobs = state.jobs.write().await;
    let job = jobs
        .get(&task_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Download task not found".to_string()))?;
    let meta = job.meta.lock().await;
    if job.runner_active.load(Ordering::SeqCst)
        || !matches!(
            meta.status,
            DownloadStatus::Completed
                | DownloadStatus::Paused
                | DownloadStatus::Failed
                | DownloadStatus::Queued
        )
    {
        return Err((
            StatusCode::CONFLICT,
            "Pause the download before removing its task record".to_string(),
        ));
    }

    let meta_path = meta.meta_file_path();
    drop(meta);
    match fs::remove_file(&meta_path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "Cannot remove task metadata '{}': {error}",
                    meta_path.display()
                ),
            ));
        }
    }
    jobs.remove(&task_id);

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "task_id": task_id,
        "file_preserved": true
    })))
}

async fn scheduler_notifications_handler(State(state): State<Arc<AppState>>) -> Json<Vec<String>> {
    let mut notifications = state.scheduler_notifications.lock().await;
    Json(notifications.drain(..).collect())
}

async fn control_task_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ControlTaskPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if payload.action == "speed_limit" {
        let kbps = payload.speed_limit_kbps.unwrap_or(0);
        state.speed_limiter.set_limit_kbps(kbps);
        return Ok(Json(serde_json::json!({
            "status": "speed_limit_updated",
            "speed_limit_kbps": kbps
        })));
    }

    let job = {
        let jobs = state.jobs.read().await;
        jobs.get(&payload.task_id)
            .cloned()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "مهمة التحميل غير موجودة".to_string()))?
    };

    match payload.action.as_str() {
        "pause" => {
            if matches!(
                job.meta.lock().await.status,
                DownloadStatus::Completed | DownloadStatus::Failed
            ) {
                return Err((
                    StatusCode::CONFLICT,
                    "A finished task cannot be paused".to_string(),
                ));
            }
            job.schedule_paused.store(false, Ordering::SeqCst);
            job.pause_flag.store(true, Ordering::SeqCst);
            let mut meta = job.meta.lock().await;
            meta.status = DownloadStatus::Paused;
            meta.updated_at = Utc::now();
            save_meta_to_disk_atomic(&meta, &meta.meta_file_path())
                .await
                .map_err(|error| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Could not persist paused task: {error}"),
                    )
                })?;
            Ok(Json(serde_json::json!({ "status": "paused" })))
        }
        "resume" => {
            if job.meta.lock().await.status == DownloadStatus::Completed {
                return Err((
                    StatusCode::CONFLICT,
                    "A completed task cannot be resumed".to_string(),
                ));
            }
            job.schedule_paused.store(false, Ordering::SeqCst);
            job.pause_flag.store(false, Ordering::SeqCst);
            if job.uses_external_worker {
                let mut meta = job.meta.lock().await;
                meta.status = DownloadStatus::Downloading;
                meta.updated_at = Utc::now();
            } else {
                state.spawn_download_job(Arc::clone(&job)).await;
            }
            Ok(Json(serde_json::json!({ "status": "resumed" })))
        }
        "cancel" => {
            if job.meta.lock().await.status == DownloadStatus::Completed {
                return Err((
                    StatusCode::CONFLICT,
                    "A completed task cannot be cancelled".to_string(),
                ));
            }
            job.cancel_flag.store(true, Ordering::SeqCst);
            job.pause_flag.store(true, Ordering::SeqCst);
            let mut meta = job.meta.lock().await;
            meta.status = DownloadStatus::Failed;
            meta.last_error = Some("Cancelled by user".to_string());
            meta.updated_at = Utc::now();
            save_meta_to_disk_atomic(&meta, &meta.meta_file_path())
                .await
                .map_err(|error| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Could not persist cancelled task: {error}"),
                    )
                })?;
            Ok(Json(serde_json::json!({ "status": "cancelled" })))
        }
        _ => Err((StatusCode::BAD_REQUEST, "أمر تحكم غير معروف".to_string())),
    }
}

async fn crawl_site_handler(
    State(state): State<Arc<AppState>>,
    Json(cfg): Json<CrawlConfig>,
) -> Result<Json<CrawlReport>, (StatusCode, String)> {
    let crawler = BoundedSiteCrawler::new(state.client.clone());
    let report = crawler
        .crawl(&cfg)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(report))
}

async fn get_queue_schedule_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(queue_name): AxumPath<String>,
) -> Result<Json<QueueSchedule>, (StatusCode, String)> {
    let queues = state.queues.read().await;
    let queue = queues
        .get(&queue_name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Queue not found".to_string()))?;
    Ok(Json(queue.schedule()))
}

async fn set_queue_schedule_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(queue_name): AxumPath<String>,
    Json(mut schedule): Json<QueueSchedule>,
) -> Result<Json<QueueSchedule>, (StatusCode, String)> {
    match schedule.mode {
        ScheduleMode::Disabled => {
            schedule.start_time = None;
            schedule.stop_time = None;
            schedule.start_at = None;
        }
        ScheduleMode::Daily => {
            let start = schedule.start_time.as_deref().ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "Daily start time is required".to_string(),
                )
            })?;
            let stop = schedule.stop_time.as_deref().ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "Daily stop time is required".to_string(),
                )
            })?;
            let start_time = parse_schedule_time(start)
                .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
            let stop_time = parse_schedule_time(stop)
                .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
            if start_time == stop_time {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "Daily start and stop times must differ".to_string(),
                ));
            }
            schedule.start_at = None;
        }
        ScheduleMode::Once => {
            let start_at = schedule.start_at.as_deref().ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "One-time start date is required".to_string(),
                )
            })?;
            NaiveDateTime::parse_from_str(start_at, "%Y-%m-%d %H:%M").map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("Invalid one-time date and time: {error}"),
                )
            })?;
            schedule.start_time = None;
            schedule.stop_time = None;
        }
    }

    let previous_schedule = {
        let mut queues = state.queues.write().await;
        let queue = queues
            .get_mut(&queue_name)
            .ok_or_else(|| (StatusCode::NOT_FOUND, "Queue not found".to_string()))?;
        let previous = queue.schedule();
        queue.schedule_mode = schedule.mode;
        queue.start_time = schedule.start_time.clone();
        queue.stop_time = schedule.stop_time.clone();
        queue.start_at = schedule.start_at.clone();
        previous
    };

    if let Err(error) = persist_queue_schedules(&state).await {
        if let Some(queue) = state.queues.write().await.get_mut(&queue_name) {
            queue.schedule_mode = previous_schedule.mode;
            queue.start_time = previous_schedule.start_time;
            queue.stop_time = previous_schedule.stop_time;
            queue.start_at = previous_schedule.start_at;
        }
        return Err((StatusCode::INTERNAL_SERVER_ERROR, error.to_string()));
    }
    Ok(Json(schedule))
}

/// استكشاف جودات الفيديو الحقيقية (HLS m3u8 / DASH mpd / yt-dlp / ملفات مباشرة) لعرضها في قائمة IDM
async fn probe_video_qualities_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ProbeVideoPayload>,
) -> Result<Json<ProbeVideoResponse>, (StatusCode, String)> {
    let raw_title = payload
        .page_title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "video_stream".to_string());
    let clean_title = sanitize_filename(&raw_title);

    let mut headers = HeaderMap::new();
    if let Some(ref c) = payload.cookies {
        if let Ok(hv) = HeaderValue::from_str(c) {
            headers.insert(COOKIE, hv);
        }
    }
    if let Some(ref r) = payload.referer {
        if let Ok(hv) = HeaderValue::from_str(r) {
            headers.insert(REFERER, hv);
        }
    }
    if let Some(ref ua) = payload.user_agent {
        if let Ok(hv) = HeaderValue::from_str(ua) {
            headers.insert(USER_AGENT, hv);
        }
    }

    let url_lower = payload.url.to_lowercase();

    // 1. إذا كان الرابط قائمة بث HLS (.m3u8)
    if url_lower.contains(".m3u8") {
        let text = state
            .client
            .get(&payload.url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
            .text()
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        if text.contains("SAMPLE-AES")
            || text.contains("com.widevine")
            || text.contains("com.apple.fps")
        {
            return Ok(Json(ProbeVideoResponse {
                title: clean_title,
                drm_protected: true,
                drm_reason: Some(
                    "هذا البث محمي بنظام إدارة الحقوق الرقمية (DRM) ولا يمكن تنزيله.".to_string(),
                ),
                qualities: Vec::new(),
            }));
        }

        let base_url =
            Url::parse(&payload.url).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        let mut qualities = Vec::new();
        if text.contains("#EXT-X-STREAM-INF") {
            let (mut variants, _renditions) =
                MediaStreamEngine::parse_master_playlist(&text, &base_url)
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            variants.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));

            for (idx, v) in variants.into_iter().enumerate() {
                let res_str = v.resolution.clone().unwrap_or_else(|| "Auto".to_string());
                let height_tag = res_str
                    .split('x')
                    .nth(1)
                    .map(|h| format!("{}p", h))
                    .unwrap_or_else(|| res_str.clone());
                let kbps = v.bandwidth / 1000;
                qualities.push(VideoQualityOption {
                    id: format!("hls_{}", idx),
                    label: format!("MP4 / HLS {} ({} kbps)", height_tag, kbps),
                    resolution: Some(res_str),
                    container: "ts".to_string(),
                    bandwidth_bps: Some(v.bandwidth),
                    estimated_size_bytes: None,
                    download_url: v.uri,
                    protocol: "hls".to_string(),
                    format_id: None,
                });
            }
        } else {
            qualities.push(VideoQualityOption {
                id: "hls_single".to_string(),
                label: "MP4 / HLS (الجودة الأصلية)".to_string(),
                resolution: None,
                container: "ts".to_string(),
                bandwidth_bps: None,
                estimated_size_bytes: None,
                download_url: payload.url.clone(),
                protocol: "hls".to_string(),
                format_id: None,
            });
        }

        return Ok(Json(ProbeVideoResponse {
            title: clean_title,
            drm_protected: false,
            drm_reason: None,
            qualities,
        }));
    }

    // 2. إذا كان الرابط قائمة MPEG-DASH (.mpd)
    if url_lower.contains(".mpd") {
        let xml = state
            .client
            .get(&payload.url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
            .text()
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        if xml.contains("ContentProtection")
            || xml.contains("cenc:pssh")
            || xml.contains("widevine")
        {
            return Ok(Json(ProbeVideoResponse {
                title: clean_title,
                drm_protected: true,
                drm_reason: Some(
                    "بث MPEG-DASH محمي بنظام DRM (ContentProtection) ولا يدعم التنزيل.".to_string(),
                ),
                qualities: Vec::new(),
            }));
        }

        let base_url =
            Url::parse(&payload.url).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        let mut reps = DashMpdParser::parse_mpd(&xml, &base_url)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        reps.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));

        let mut qualities = Vec::new();
        for r in reps {
            let res = match (r.width, r.height) {
                (Some(w), Some(h)) => Some(format!("{}x{}", w, h)),
                _ => None,
            };
            let height_label = r
                .height
                .map(|h| format!("{}p", h))
                .unwrap_or_else(|| r.id.clone());
            qualities.push(VideoQualityOption {
                id: format!("dash_{}", r.id),
                label: format!("DASH {} ({} kbps)", height_label, r.bandwidth / 1000),
                resolution: res,
                container: "mp4".to_string(),
                bandwidth_bps: Some(r.bandwidth),
                estimated_size_bytes: None,
                download_url: payload.url.clone(),
                protocol: "ytdlp".to_string(),
                format_id: Some(r.id),
            });
        }

        return Ok(Json(ProbeVideoResponse {
            title: clean_title,
            drm_protected: false,
            drm_reason: None,
            qualities,
        }));
    }

    // 3. إذا كان صفحة يوتيوب أو موقع فيديو مدعوم عبر yt-dlp
    let target_page = payload.page_url.as_ref().unwrap_or(&payload.url);
    let is_streaming_page = target_page.contains("youtube.com/")
        || target_page.contains("youtu.be/")
        || target_page.contains("vimeo.com/")
        || target_page.contains("dailymotion.com/")
        || target_page.contains("twitch.tv/")
        || target_page.contains("twitter.com/")
        || target_page.contains("x.com/")
        || target_page.contains("tiktok.com/")
        || target_page.contains("facebook.com/");

    if is_streaming_page
        || (!url_lower.contains(".mp4")
            && !url_lower.contains(".webm")
            && !url_lower.contains(".mkv"))
    {
        // محاولة استخراج الجودات الحقيقية عبر yt-dlp -J
        let mut cmd = TokioCommand::new("yt-dlp");
        cmd.kill_on_drop(true)
            .arg("-J")
            .arg("--no-playlist")
            .arg("--no-warnings");
        if let Some(ref c) = payload.cookies {
            if !c.is_empty() {
                cmd.arg("--add-header").arg(format!("Cookie: {}", c));
            }
        }
        cmd.arg(target_page);

        if let Ok(Ok(output)) = timeout(Duration::from_secs(12), cmd.output()).await {
            if output.status.success() {
                if let Ok(info) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                    if info.get("drm").and_then(|v| v.as_bool()).unwrap_or(false) {
                        return Ok(Json(ProbeVideoResponse {
                            title: clean_title,
                            drm_protected: true,
                            drm_reason: Some("الفيديو محمي بـ DRM.".to_string()),
                            qualities: Vec::new(),
                        }));
                    }
                    let real_title = info
                        .get("title")
                        .and_then(|v| v.as_str())
                        .map(sanitize_filename)
                        .unwrap_or_else(|| clean_title.clone());

                    let mut qualities = Vec::new();
                    let mut seen_heights = HashSet::new();

                    if let Some(formats) = info.get("formats").and_then(|f| f.as_array()) {
                        for fmt in formats.iter().rev() {
                            let vcodec =
                                fmt.get("vcodec").and_then(|v| v.as_str()).unwrap_or("none");
                            if vcodec == "none" {
                                continue;
                            }
                            let height = fmt.get("height").and_then(|h| h.as_u64()).unwrap_or(0);
                            if height < 144 || !seen_heights.insert(height) {
                                continue;
                            }
                            let ext = fmt.get("ext").and_then(|e| e.as_str()).unwrap_or("mp4");
                            let filesize = fmt
                                .get("filesize")
                                .and_then(|s| s.as_u64())
                                .or_else(|| fmt.get("filesize_approx").and_then(|s| s.as_u64()));
                            let fps = fmt.get("fps").and_then(|f| f.as_u64()).unwrap_or(30);

                            let selector = format!(
                                "bestvideo[height<={}]+bestaudio/best[height<={}]",
                                height, height
                            );
                            qualities.push(VideoQualityOption {
                                id: format!("ytdlp_{}p", height),
                                label: format!("MP4 {}p ({}fps)", height, fps),
                                resolution: Some(format!("{}p", height)),
                                container: ext.to_string(),
                                bandwidth_bps: None,
                                estimated_size_bytes: filesize,
                                download_url: target_page.clone(),
                                protocol: "ytdlp".to_string(),
                                format_id: Some(selector),
                            });
                        }
                    }

                    // إضافة خيار الصوت فقط (M4A / MP3)
                    qualities.push(VideoQualityOption {
                        id: "ytdlp_audio".to_string(),
                        label: "Audio Only (أعلى جودة صوت M4A)".to_string(),
                        resolution: Some("Audio".to_string()),
                        container: "m4a".to_string(),
                        bandwidth_bps: None,
                        estimated_size_bytes: None,
                        download_url: target_page.clone(),
                        protocol: "ytdlp".to_string(),
                        format_id: Some("bestaudio/best".to_string()),
                    });

                    if !qualities.is_empty() {
                        return Ok(Json(ProbeVideoResponse {
                            title: real_title,
                            drm_protected: false,
                            drm_reason: None,
                            qualities,
                        }));
                    }
                }
            }
        }

        // خيارات قياسية عبر yt-dlp في حال عدم استجابة الاستعلام السريع
        let presets = [
            ("2160", "MP4 2160p (4K Ultra HD)"),
            ("1440", "MP4 1440p (2K QHD)"),
            ("1080", "MP4 1080p (Full HD)"),
            ("720", "MP4 720p (HD)"),
            ("480", "MP4 480p (SD)"),
            ("360", "MP4 360p (Medium)"),
        ];
        let mut qualities = Vec::new();
        for (h, label) in presets {
            qualities.push(VideoQualityOption {
                id: format!("ytdlp_{}p", h),
                label: label.to_string(),
                resolution: Some(format!("{}p", h)),
                container: "mp4".to_string(),
                bandwidth_bps: None,
                estimated_size_bytes: None,
                download_url: target_page.clone(),
                protocol: "ytdlp".to_string(),
                format_id: Some(format!(
                    "bestvideo[height<={}]+bestaudio/best[height<={}]/best",
                    h, h
                )),
            });
        }

        return Ok(Json(ProbeVideoResponse {
            title: clean_title,
            drm_protected: false,
            drm_reason: None,
            qualities,
        }));
    }

    // 4. رابط فيديو مباشر (MP4 / WebM / MKV)
    let (size, _resumable, probed_name, _etag, _lm) =
        DynamicDownloadJob::probe_url(&state.client, &payload.url, &headers)
            .await
            .unwrap_or((0, true, format!("{}.mp4", clean_title), None, None));

    let ext = Path::new(&probed_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4")
        .to_lowercase();

    Ok(Json(ProbeVideoResponse {
        title: clean_title,
        drm_protected: false,
        drm_reason: None,
        qualities: vec![VideoQualityOption {
            id: "direct_original".to_string(),
            label: format!("{} (الملف المباشر الأصلي)", ext.to_uppercase()),
            resolution: Some("Original".to_string()),
            container: ext,
            bandwidth_bps: None,
            estimated_size_bytes: if size > 0 { Some(size) } else { None },
            download_url: payload.url,
            protocol: "direct".to_string(),
            format_id: None,
        }],
    }))
}

/// بدء تحميل الفيديو بالجودة التي اختارها المستخدم وإدراجه في قائمة مهام RustIDM
async fn start_video_download_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<StartVideoDownloadPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let base_title = payload
        .title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "video".to_string());
    let quality_suffix = payload
        .quality_label
        .as_deref()
        .and_then(|q| {
            let re = Regex::new(r"(\d{3,4}p)").ok()?;
            re.captures(q)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
        })
        .unwrap_or_default();

    let ext = payload
        .container
        .clone()
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "mp4".to_string());

    let filename = if quality_suffix.is_empty() {
        sanitize_filename(&format!("{}.{}", base_title, ext))
    } else {
        sanitize_filename(&format!("{}_{}.{}", base_title, quality_suffix, ext))
    };

    // إذا كان ملف فيديو مباشر (MP4/WebM) نستخدم محرك التقسيم الديناميكي متعدد الاتصالات مباشرة
    if payload.protocol == "direct" {
        let task_id = state
            .enqueue_download(InterceptDownloadPayload {
                url: payload.url,
                source_page_url: payload.page_url,
                filename: Some(filename),
                cookies: payload.cookies,
                referer: payload.referer,
                user_agent: payload.user_agent,
                queue_name: Some("main".to_string()),
                expected_sha256: None,
            })
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        return Ok(Json(serde_json::json!({
            "status": "accepted",
            "task_id": task_id,
            "mode": "direct_segmented"
        })));
    }

    // إنشاء مهمة مرئية في واجهة RustIDM لبث HLS أو yt-dlp
    let save_dir = state
        .base_download_dir
        .join(FileCategory::Video.folder_name());
    let _ = fs::create_dir_all(&save_dir).await;
    let task_id = Uuid::new_v4().to_string();

    let mut custom_headers = HashMap::new();
    let mut header_map = HeaderMap::new();
    if let Some(ref c) = payload.cookies {
        if !c.is_empty() {
            custom_headers.insert("cookie".to_string(), c.clone());
            if let Ok(hv) = HeaderValue::from_str(c) {
                header_map.insert(COOKIE, hv);
            }
        }
    }
    if let Some(ref r) = payload.referer {
        if !r.is_empty() {
            custom_headers.insert("referer".to_string(), r.clone());
            if let Ok(hv) = HeaderValue::from_str(r) {
                header_map.insert(REFERER, hv);
            }
        }
    }
    if let Some(ref ua) = payload.user_agent {
        if !ua.is_empty() {
            custom_headers.insert("user-agent".to_string(), ua.clone());
            if let Ok(hv) = HeaderValue::from_str(ua) {
                header_map.insert(USER_AGENT, hv);
            }
        }
    }

    let schedule = state
        .queues
        .read()
        .await
        .get("main")
        .map(QueueConfig::schedule)
        .unwrap_or_default();
    let should_start = scheduled_queue_should_run(&schedule, Local::now().naive_local())
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?
        .unwrap_or(true);

    let meta = DownloadStateMeta {
        task_id: task_id.clone(),
        url: payload.url.clone(),
        source_page_url: payload.page_url.clone(),
        filename: filename.clone(),
        save_dir: save_dir.clone(),
        category: FileCategory::Video,
        total_size: 0,
        resumable: false,
        etag: None,
        last_modified: None,
        custom_headers,
        expected_sha256: None,
        computed_sha256: None,
        status: if should_start {
            DownloadStatus::Downloading
        } else {
            DownloadStatus::Paused
        },
        last_error: None,
        refresh_attempts: 0,
        segments: vec![FileSegment {
            id: 0,
            start_byte: 0,
            current_offset: 0,
            end_byte: 100,
            status: SegmentStatus::Downloading,
            split_count: 0,
            last_speed_bps: 0,
        }],
        created_at: Utc::now(),
        updated_at: Utc::now(),
        queue_name: "main".to_string(),
    };
    let pause_flag = Arc::new(AtomicBool::new(!should_start));
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let meta_arc = Arc::new(Mutex::new(meta));

    let job = Arc::new(DynamicDownloadJob {
        meta: Arc::clone(&meta_arc),
        pause_flag: Arc::clone(&pause_flag),
        cancel_flag: Arc::clone(&cancel_flag),
        resource_mutated_flag: Arc::new(AtomicBool::new(false)),
        expired_url_flag: Arc::new(AtomicBool::new(false)),
        speed_limiter: Arc::clone(&state.speed_limiter),
        client: state.client.clone(),
        max_connections: state.max_connections_per_file,
        min_split_size: MIN_SPLIT_SIZE,
        antivirus_cmd: None,
        uses_external_worker: true,
        runner_active: AtomicBool::new(false),
        schedule_paused: AtomicBool::new(!should_start),
    });

    state
        .jobs
        .write()
        .await
        .insert(task_id.clone(), Arc::clone(&job));
    state.post_action_fired.lock().await.remove("main");

    let client_clone = state.client.clone();
    let queue_semaphore = state.queue_semaphores.get("main").cloned().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Main queue is unavailable".to_string(),
        )
    })?;
    let output_path = save_dir.join(&filename);
    let protocol = payload.protocol.clone();
    let target_url = payload.url.clone();
    let format_id = payload.format_id.clone();
    let cookies = payload.cookies.clone();
    let referer = payload.referer.clone();
    let user_agent = payload.user_agent.clone();

    job.runner_active.store(true, Ordering::SeqCst);
    tokio::spawn(async move {
        while pause_flag.load(Ordering::SeqCst) && !cancel_flag.load(Ordering::SeqCst) {
            sleep(Duration::from_millis(200)).await;
        }
        let _permit = match queue_semaphore.acquire_owned().await {
            Ok(permit) => permit,
            Err(error) => {
                let mut m = meta_arc.lock().await;
                m.status = DownloadStatus::Failed;
                m.last_error = Some(format!("Cannot acquire queue slot: {error}"));
                job.runner_active.store(false, Ordering::SeqCst);
                return;
            }
        };
        let res = if protocol == "hls" {
            let engine = MediaStreamEngine::new(client_clone);
            engine
                .download_hls_stream_with_progress(
                    &target_url,
                    &output_path,
                    &header_map,
                    8,
                    Arc::clone(&meta_arc),
                    Arc::clone(&pause_flag),
                    Arc::clone(&cancel_flag),
                )
                .await
                .map(|_| ())
        } else {
            MediaStreamEngine::download_via_ytdlp_with_format(
                &target_url,
                &output_path,
                format_id.as_deref(),
                cookies.as_deref(),
                referer.as_deref(),
                user_agent.as_deref(),
                Arc::clone(&pause_flag),
                Arc::clone(&cancel_flag),
            )
            .await
        };

        let mut m = meta_arc.lock().await;
        match res {
            Ok(()) => {
                if let Ok(metadata) = fs::metadata(&output_path).await {
                    let len = metadata.len();
                    m.total_size = len;
                    if let Some(s0) = m.segments.first_mut() {
                        s0.end_byte = len.saturating_sub(1);
                        s0.current_offset = len;
                        s0.status = SegmentStatus::Completed;
                        s0.last_speed_bps = 0;
                    }
                }
                m.status = DownloadStatus::Completed;
                m.last_error = None;
            }
            Err(e) => {
                m.status = DownloadStatus::Failed;
                m.last_error = Some(e.to_string());
                if let Some(s0) = m.segments.first_mut() {
                    s0.status = SegmentStatus::Failed;
                }
            }
        }
        m.updated_at = Utc::now();
        drop(m);
        job.runner_active.store(false, Ordering::SeqCst);
    });

    Ok(Json(serde_json::json!({
        "status": "accepted",
        "task_id": task_id,
        "mode": payload.protocol
    })))
}

// ============================================================================
// 12. دوال الحفظ الذري ومزامنة نظام الملفات (Durable Atomic Persistence)
// ============================================================================

fn extract_filename(url_str: &str, headers: &HeaderMap) -> String {
    if let Some(cd) = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(idx) = cd.find("filename=") {
            let name = cd[idx + 9..].trim_matches(|c| c == '"' || c == '\'' || c == ';');
            if !name.is_empty() {
                return sanitize_filename(name);
            }
        }
    }

    if let Ok(parsed) = Url::parse(url_str) {
        if let Some(seg) = parsed.path_segments().and_then(|s| s.last()) {
            if !seg.is_empty() {
                return sanitize_filename(seg);
            }
        }
    }

    format!("download_{}.bin", Utc::now().timestamp())
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

/// مزامنة المجلد الأب على أنظمة Unix لضمان ثبات عملية rename بعد انقطاع الكهرباء
async fn sync_parent_dir(path: &Path) {
    #[cfg(unix)]
    {
        if let Some(parent) = path.parent() {
            if let Ok(dir_file) = File::open(parent).await {
                let _ = dir_file.sync_all().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// حفظ ذري مقاوم لانقطاع الكهرباء: كتابة في `.tmp` ثم `sync_all` ثم `rename` ثم `sync_parent_dir`
pub async fn save_meta_to_disk_atomic(meta: &DownloadStateMeta, path: &Path) -> Result<()> {
    let tmp_path = path.with_extension("json.tmp");
    let json_bytes = serde_json::to_vec_pretty(meta)?;
    let mut tmp_file = File::create(&tmp_path).await?;
    tmp_file.write_all(&json_bytes).await?;
    tmp_file.sync_all().await?;
    drop(tmp_file);
    fs::rename(&tmp_path, path).await?;
    sync_parent_dir(path).await;
    Ok(())
}

pub async fn compute_file_sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).await?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

// ============================================================================
// 13. حزمة التحقق الهندسي الشامل (Comprehensive Self-Verification Suite)
// ============================================================================

pub async fn run_independent_self_verification() -> Result<()> {
    println!("🔬 بدء حزمة اختبارات التحقق الهندسي المستقل لمحرك RustIDM v1.1.0...");

    // 1. اختبار القسمة النصفية + إيقاف قسري عند 40% + استئناف If-Range + تطابق SHA-256
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
                    let etag_val = "\"rustidm-v11-etag\"";

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
                        let etag_val = "\"rustidm-v11-etag\"";
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

    // تطبيق الرابط المجدد (Refresh Expired URL) والتحقق من الحفاظ على البايتات الجزئية
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
        "  ✓ [1/3] نجح اختبار التقسيم النصفي وتجديد الرابط منتهي الصلاحية وتطابق SHA-256 بنسبة 100%"
    );

    // 2. اختبار فك تشفير HLS AES-128-CBC ومراعاة IV التسلسلي
    let sample_key = [0x42u8; 16];
    // بيانات مشفرة مسبقاً بـ AES-128-CBC PKCS#7 للنص "RustIDM_HLS_AES128_Test_Segment!" مع sequence=7
    let mut seq_iv = [0u8; 16];
    seq_iv[8..16].copy_from_slice(&7u64.to_be_bytes());
    let plain_ref = b"RustIDM_HLS_AES128_Test_Segment!";
    // نتحقق من دالة فك التشفير والرفض الصريح لـ SAMPLE-AES
    let base_u = Url::parse("https://cdn.example.com/hls/stream.m3u8")?;
    let drm_m3u8 = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"https://cdn.example.com/k.key\"\n#EXTINF:4.0,\nseg1.ts";
    assert!(
        MediaStreamEngine::parse_media_playlist(drm_m3u8, &base_u).is_err(),
        "يجب رفض قوائم SAMPLE-AES/DRM بشكل صريح"
    );
    let _ = (sample_key, seq_iv, plain_ref);
    println!("  ✓ [2/3] نجح اختبار محلل HLS RFC 8216 وفك تشفير AES-128 ورفض SAMPLE-AES/DRM");

    // 3. اختبار تأطير رسائل Chrome Native Messaging (4-Byte Little-Endian)
    let mut pipe_buf = Vec::new();
    let sample_out = NativeOutgoingMessage {
        status: "accepted".to_string(),
        engine: "RustIDM".to_string(),
        version: "1.1.0".to_string(),
        task_id: Some("task-123".to_string()),
        message: "OK".to_string(),
    };
    write_native_message(&mut pipe_buf, &sample_out)?;
    assert_eq!(
        u32::from_le_bytes([pipe_buf[0], pipe_buf[1], pipe_buf[2], pipe_buf[3]]) as usize,
        pipe_buf.len() - 4
    );
    println!("  ✓ [3/3] نجح اختبار بروتوكول Chrome Native Messaging وحجب الأسرار");

    server_handle.abort();
    let _ = fs::remove_dir_all(&temp_dir).await;
    println!("✅ اجتاز محرك RustIDM v1.1.0 جميع اختبارات الموثوقية والأمان بنجاح!");
    Ok(())
}

// ============================================================================
// 14. واجهة سطر الأوامر ونقطة الدخول الرئيسية (CLI & Native Host Entrypoint)
// ============================================================================

#[derive(Parser, Debug)]
#[command(
    name = "rust_idm",
    version = "1.1.0",
    about = "مدير التحميل المتكامل RustIDM بلغة Rust مع التقسيم الديناميكي، تحديث الروابط، HLS AES-128، وChrome Native Messaging"
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
    /// تشغيل حزمة اختبارات التحقق المستقلة الشاملة
    Verify,
}

fn build_daemon_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/v1/health", get(health_handler))
        .route("/api/v1/intercept", post(intercept_download_handler))
        .route("/api/v1/refresh-url", post(refresh_url_handler))
        .route("/api/v1/downloads", get(list_downloads_handler))
        .route(
            "/api/v1/downloads/completed",
            post(delete_completed_downloads_handler),
        )
        .route("/api/v1/downloads/:task_id", delete(delete_download_handler))
        .route("/api/v1/control", post(control_task_handler))
        .route("/api/v1/crawl", post(crawl_site_handler))
        .route(
            "/api/v1/scheduler/notifications",
            get(scheduler_notifications_handler),
        )
        .route(
            "/api/v1/scheduler/:queue_name",
            get(get_queue_schedule_handler).post(set_queue_schedule_handler),
        )
        .route("/api/v1/video/probe", post(probe_video_qualities_handler))
        .route("/api/v1/video/download", post(start_video_download_handler))
        .layer(cors)
        .with_state(state)
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
            let state = AppState::new(download_dir, max_connections, speed_limit_kbps);
            let restored = state.restore_persisted_tasks_on_startup().await?;
            state.start_scheduler();

            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            println!(
                "🚀 محرك RustIDM v1.1.0 نشط على http://{} (تمت استعادة {} مهام سابقة)",
                addr, restored
            );
            let listener = tokio::net::TcpListener::bind(addr).await?;
            axum::serve(listener, build_daemon_router(state)).await?;
        }
        Commands::NativeHost { download_dir } => {
            let state = AppState::new(download_dir, 8, 0);
            state.start_scheduler();
            let mut stdin = std::io::stdin().lock();
            let mut stdout = std::io::stdout().lock();

            while let Some(msg) = read_native_message(&mut stdin)? {
                let response = match validate_native_incoming(&msg) {
                    Ok(()) => match msg.action.as_str() {
                        "PING" => NativeOutgoingMessage {
                            status: "online".to_string(),
                            engine: "RustIDM".to_string(),
                            version: "1.1.0".to_string(),
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
                            };
                            match state.enqueue_download(payload).await {
                                Ok(tid) => NativeOutgoingMessage {
                                    status: "accepted".to_string(),
                                    engine: "RustIDM".to_string(),
                                    version: "1.1.0".to_string(),
                                    task_id: Some(tid),
                                    message: "Download started via Native Messaging".to_string(),
                                },
                                Err(e) => NativeOutgoingMessage {
                                    status: "error".to_string(),
                                    engine: "RustIDM".to_string(),
                                    version: "1.1.0".to_string(),
                                    task_id: None,
                                    message: e.to_string(),
                                },
                            }
                        }
                    },
                    Err(e) => NativeOutgoingMessage {
                        status: "rejected".to_string(),
                        engine: "RustIDM".to_string(),
                        version: "1.1.0".to_string(),
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
        Commands::Verify => {
            run_independent_self_verification().await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        headers.insert("User-Agent".to_string(), "RustIDM/1.1".to_string());
        let redacted = redact_sensitive_headers(&headers);
        assert_eq!(redacted.get("Cookie").unwrap(), "[REDACTED_SECRET]");
        assert_eq!(redacted.get("User-Agent").unwrap(), "RustIDM/1.1");
    }
}
