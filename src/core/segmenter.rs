//! محرك التقسيم الديناميكي النصفي الخالي من التسابق والتنافس على الأقفال (Lock-Free Atomic Segment Telemetry & In-Half Dynamic Segmentation)
//! مع نظام تجديد الروابط منتهية الصلاحية (`SourcePageRefreshAdapter`) وفاحص سلامة المقاطع (`SegmentIntegrityAuditor`).

use crate::core::{
    limiter::SpeedLimiter,
    storage::{
        compute_file_all_digests, extract_filename, save_meta_to_disk_atomic, sync_parent_dir,
        DownloadStateMeta, DownloadStatus, FileSegment, SegmentStatus,
    },
};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use futures_util::StreamExt;
use regex::Regex;
use reqwest::{
    header::{
        HeaderMap, HeaderName, HeaderValue, ACCEPT_RANGES, CONTENT_LENGTH, ETAG, IF_RANGE,
        LAST_MODIFIED, RANGE,
    },
    Client,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::SeekFrom,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        Arc, RwLock as StdRwLock,
    },
    time::{Duration, Instant},
};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncSeekExt, AsyncWriteExt},
    sync::Mutex,
    time::{sleep, timeout},
};
use url::Url;

// ============================================================================
// 1. الإحصائيات الذرية عديمة الأقفال للمقاطع النشطة (Lock-Free Atomic Segment Telemetry)
// ============================================================================

/// هيكل ذري خالٍ من الأقفال (Lock-Free) لتتبع تقدم وسرعة ونهاية كل مقطع أثناء التحميل الفائق السرعة
/// يمنع التنافس على القفل (`Mutex Lock Contention`) عند تشغيل 16 أو 32 اتصالاً موازياً بسرعات Gigabit.
#[derive(Debug)]
pub struct SegmentAtomicTelemetry {
    pub id: usize,
    pub start_byte: u64,
    pub current_offset: AtomicU64,
    pub end_byte: AtomicU64,
    pub last_speed_bps: AtomicU64,
    pub split_count: AtomicU64,
    pub status_code: AtomicU8,
}

impl SegmentAtomicTelemetry {
    pub fn from_segment(seg: &FileSegment) -> Arc<Self> {
        Arc::new(Self {
            id: seg.id,
            start_byte: seg.start_byte,
            current_offset: AtomicU64::new(seg.current_offset),
            end_byte: AtomicU64::new(seg.end_byte),
            last_speed_bps: AtomicU64::new(seg.last_speed_bps),
            split_count: AtomicU64::new(seg.split_count as u64),
            status_code: AtomicU8::new(seg.status.to_u8()),
        })
    }

    pub fn remaining_bytes(&self) -> u64 {
        let cur = self.current_offset.load(Ordering::Acquire);
        let end = self.end_byte.load(Ordering::Acquire);
        if cur > end {
            0
        } else {
            (end - cur) + 1
        }
    }

    pub fn to_file_segment(&self) -> FileSegment {
        FileSegment {
            id: self.id,
            start_byte: self.start_byte,
            current_offset: self.current_offset.load(Ordering::Acquire),
            end_byte: self.end_byte.load(Ordering::Acquire),
            status: SegmentStatus::from_u8(self.status_code.load(Ordering::Acquire)),
            split_count: self.split_count.load(Ordering::Relaxed) as usize,
            last_speed_bps: self.last_speed_bps.load(Ordering::Relaxed),
        }
    }
}

/// سجل مركزي للعدادات الذرية للمقاطع النشطة يتيح لعمال الشبكة التحديث بدون قفل `DownloadStateMeta`
#[derive(Default)]
pub struct AtomicSegmentRegistry {
    segments: StdRwLock<HashMap<usize, Arc<SegmentAtomicTelemetry>>>,
}

impl AtomicSegmentRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            segments: StdRwLock::new(HashMap::new()),
        })
    }

    pub fn initialize_from_meta(&self, meta: &DownloadStateMeta) {
        let mut guard = self.segments.write().unwrap();
        guard.clear();
        for seg in &meta.segments {
            guard.insert(seg.id, SegmentAtomicTelemetry::from_segment(seg));
        }
    }

    pub fn get(&self, seg_id: usize) -> Option<Arc<SegmentAtomicTelemetry>> {
        self.segments.read().unwrap().get(&seg_id).cloned()
    }

    pub fn insert(&self, seg: &FileSegment) -> Arc<SegmentAtomicTelemetry> {
        let arc = SegmentAtomicTelemetry::from_segment(seg);
        self.segments
            .write()
            .unwrap()
            .insert(seg.id, Arc::clone(&arc));
        arc
    }

    /// مزامنة القراءات الذرية اللحظية إلى هيكل `DownloadStateMeta` دون إبطاء حلقات الشبكة
    pub fn sync_to_meta(&self, meta: &mut DownloadStateMeta) {
        let guard = self.segments.read().unwrap();
        for seg in meta.segments.iter_mut() {
            if let Some(atom) = guard.get(&seg.id) {
                seg.current_offset = atom.current_offset.load(Ordering::Acquire);
                seg.end_byte = atom.end_byte.load(Ordering::Acquire);
                seg.last_speed_bps = atom.last_speed_bps.load(Ordering::Relaxed);
                seg.split_count = atom.split_count.load(Ordering::Relaxed) as usize;
                let atomic_status = SegmentStatus::from_u8(atom.status_code.load(Ordering::Acquire));
                if seg.status == SegmentStatus::Downloading
                    && atomic_status == SegmentStatus::Completed
                {
                    seg.status = SegmentStatus::Completed;
                    seg.last_speed_bps = 0;
                }
            }
        }
    }
}

// ============================================================================
// 2. نظام تحديث الروابط منتهية الصلاحية (Extensible Expired URL Refresh System)
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
// 3. محرك التقسيم الديناميكي المحصّن (Hardened DynamicDownloadJob)
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
            for seg in m.segments.iter_mut() {
                if seg.status != SegmentStatus::Completed {
                    seg.status = SegmentStatus::Pending;
                }
            }
            let meta_path = m.meta_file_path();
            save_meta_to_disk_atomic(&m, &meta_path).await?;
            Ok(true)
        } else {
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

    /// تنفيذ التحميل مع عدادات ذرية عديمة الأقفال (Lock-Free Atomic Segment Telemetry) ومزامنة خلفية كل 500ms
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

            let atomic_registry = AtomicSegmentRegistry::new();
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
                atomic_registry.initialize_from_meta(&m);
                save_meta_to_disk_atomic(&m, &meta_path).await?;
            }

            // مهمة خلفية غير متزامنة تنسخ القراءات الذرية (AtomicU64) إلى DownloadStateMeta كل 50ms
            // وتحفظ ملف الحالة الذري (.idm.json) على القرص كل 500ms فقط لمنع أي Lock Contention!
            let meta_saver_clone = Arc::clone(&self.meta);
            let pause_saver_clone = Arc::clone(&self.pause_flag);
            let registry_saver_clone = Arc::clone(&atomic_registry);
            let meta_path_clone = meta_path.clone();
            let saver_handle = tokio::spawn(async move {
                let mut tick_count = 0u64;
                loop {
                    sleep(Duration::from_millis(50)).await;
                    tick_count += 1;
                    let mut m = meta_saver_clone.lock().await;
                    registry_saver_clone.sync_to_meta(&mut m);

                    if tick_count % 10 == 0 {
                        let current_speed: u64 = m
                            .segments
                            .iter()
                            .filter(|s| s.status == SegmentStatus::Downloading)
                            .map(|s| s.last_speed_bps)
                            .sum();
                        let active_conns = m
                            .segments
                            .iter()
                            .filter(|s| s.status == SegmentStatus::Downloading)
                            .count();
                        m.push_speed_sample(current_speed, active_conns);
                        let _ = save_meta_to_disk_atomic(&m, &meta_path_clone).await;
                    }

                    if pause_saver_clone.load(Ordering::Relaxed)
                        || m.status == DownloadStatus::Completed
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
                let reg_arc = Arc::clone(&atomic_registry);
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
                        reg_arc,
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

            // مزامنة ختامية للعدادات الذرية مع DownloadStateMeta بعد انتهاء جميع العمال
            {
                let mut m = self.meta.lock().await;
                atomic_registry.sync_to_meta(&mut m);
            }

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
                            continue;
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

            m.status = DownloadStatus::Assembling;
            {
                let pf = OpenOptions::new().write(true).open(&part_path).await?;
                pf.sync_all().await?;
            }
            fs::rename(&part_path, &final_path).await?;
            sync_parent_dir(&final_path).await;

            m.status = DownloadStatus::VerifyingChecksum;
            m.push_log(
                None,
                "INFO",
                "جاري فحص التكامل والبصمات الرقمية (SHA-256 / MD5 / CRC32)...",
            );
            let (actual_sha256, actual_md5, actual_crc32) =
                compute_file_all_digests(&final_path).await?;
            m.computed_sha256 = Some(actual_sha256.clone());
            m.computed_md5 = Some(actual_md5.clone());
            m.computed_crc32 = Some(actual_crc32.clone());

            if let Some(expected_hash) = m.expected_sha256.clone().filter(|s| !s.trim().is_empty()) {
                if !actual_sha256.eq_ignore_ascii_case(expected_hash.trim()) {
                    m.status = DownloadStatus::Failed;
                    let err_msg = format!(
                        "عدم تطابق بصمة SHA-256: المتوقع {} والفعلي {}",
                        expected_hash, actual_sha256
                    );
                    m.last_error = Some(err_msg.clone());
                    m.push_log(None, "ERROR", err_msg);
                    save_meta_to_disk_atomic(&m, &meta_path).await?;
                    return Err(anyhow!("فشل التحقق من بصمة SHA-256"));
                }
            }

            if let Some(expected_md5) = m.expected_md5.clone().filter(|s| !s.trim().is_empty()) {
                if !actual_md5.eq_ignore_ascii_case(expected_md5.trim()) {
                    m.status = DownloadStatus::Failed;
                    let err_msg = format!(
                        "عدم تطابق بصمة MD5: المتوقع {} والفعلي {}",
                        expected_md5, actual_md5
                    );
                    m.last_error = Some(err_msg.clone());
                    m.push_log(None, "ERROR", err_msg);
                    save_meta_to_disk_atomic(&m, &meta_path).await?;
                    return Err(anyhow!("فشل التحقق من بصمة MD5"));
                }
            }

            if let Some(expected_crc) = m.expected_crc32.clone().filter(|s| !s.trim().is_empty()) {
                if !actual_crc32.eq_ignore_ascii_case(expected_crc.trim()) {
                    m.status = DownloadStatus::Failed;
                    let err_msg = format!(
                        "عدم تطابق بصمة CRC32: المتوقع {} والفعلي {}",
                        expected_crc, actual_crc32
                    );
                    m.last_error = Some(err_msg.clone());
                    m.push_log(None, "ERROR", err_msg);
                    save_meta_to_disk_atomic(&m, &meta_path).await?;
                    return Err(anyhow!("فشل التحقق من بصمة CRC32"));
                }
            }

            if let Some(ref av_cmd) = self.antivirus_cmd {
                let _ = Command::new(av_cmd).arg(&final_path).status();
            }

            for seg in m.segments.iter_mut() {
                seg.last_speed_bps = 0;
            }
            m.status = DownloadStatus::Completed;
            m.last_error = None;
            m.updated_at = Utc::now();
            m.push_log(
                None,
                "INFO",
                format!(
                    "اكتمل التحميل بنجاح (SHA-256: {}.. | CRC32: {})",
                    &actual_sha256[..12.min(actual_sha256.len())],
                    actual_crc32
                ),
            );
            let _ = save_meta_to_disk_atomic(&m, &meta_path).await;

            return Ok(final_path);
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn connection_worker_loop(
        _worker_idx: usize,
        meta: Arc<Mutex<DownloadStateMeta>>,
        atomic_registry: Arc<AtomicSegmentRegistry>,
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
                atomic_registry.sync_to_meta(&mut m);
                let active_count = m
                    .segments
                    .iter()
                    .filter(|s| s.status == SegmentStatus::Downloading)
                    .count();
                if active_count >= max_connections {
                    None
                } else {
                    Self::acquire_or_split_segment_with_atomics(
                        &mut m,
                        min_split_size,
                        Some(&atomic_registry),
                    )
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
                    &atomic_registry,
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
                        {
                            let mut m = meta.lock().await;
                            atomic_registry.sync_to_meta(&mut m);
                            m.push_log(
                                Some(seg_id),
                                "WARN",
                                format!(
                                    "محاولة إعادة اتصال #{}/4 للمقطع #{}: {}",
                                    retries, seg_id, err_msg
                                ),
                            );
                            if retries == 2 {
                                let _ = m.rotate_mirror();
                            }
                        }
                        if retries > 4 {
                            let mut m = meta.lock().await;
                            atomic_registry.sync_to_meta(&mut m);
                            m.last_error = Some(err_msg.clone());
                            m.push_log(
                                Some(seg_id),
                                "ERROR",
                                format!(
                                    "فشل المقطع #{} بعد استنفاد المحاولات: {}",
                                    seg_id, err_msg
                                ),
                            );
                            if let Some(atom) = atomic_registry.get(seg_id) {
                                atom.status_code
                                    .store(SegmentStatus::Failed.to_u8(), Ordering::Release);
                                atom.last_speed_bps.store(0, Ordering::Relaxed);
                            }
                            if let Some(s) = m.segments.iter_mut().find(|s| s.id == seg_id) {
                                s.status = SegmentStatus::Failed;
                                s.last_speed_bps = 0;
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

    /// قاعدة القسمة النصفية الذرية (In-Half Division Rule) المتوافقة مع الاختبارات المباشرة
    pub fn acquire_or_split_segment(
        m: &mut DownloadStateMeta,
        min_split_size: u64,
    ) -> Option<usize> {
        Self::acquire_or_split_segment_with_atomics(m, min_split_size, None)
    }

    /// قاعدة القسمة النصفية مع تحديث العدادات الذرية (`AtomicU64`) فورياً دون تعطيل العامل الجاري
    pub fn acquire_or_split_segment_with_atomics(
        m: &mut DownloadStateMeta,
        min_split_size: u64,
        registry: Option<&AtomicSegmentRegistry>,
    ) -> Option<usize> {
        if let Some(reg) = registry {
            reg.sync_to_meta(m);
        }

        if let Some(seg) = m
            .segments
            .iter_mut()
            .find(|s| s.status == SegmentStatus::Pending)
        {
            seg.status = SegmentStatus::Downloading;
            if let Some(reg) = registry {
                if let Some(atom) = reg.get(seg.id) {
                    atom.status_code
                        .store(SegmentStatus::Downloading.to_u8(), Ordering::Release);
                } else {
                    reg.insert(seg);
                }
            }
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
                let parent_id = m.segments[idx].id;
                let old_end = m.segments[idx].end_byte;
                let cur_off = m.segments[idx].current_offset;
                let midpoint = cur_off + (max_remaining / 2) - 1;

                if midpoint >= cur_off && midpoint < old_end {
                    m.segments[idx].end_byte = midpoint;
                    m.segments[idx].split_count += 1;

                    // تحديث الحد النهائي الذري (end_byte) للمقطع الأصلي فوراً ليقرأه العامل النشط بدون قفل!
                    if let Some(reg) = registry {
                        if let Some(parent_atom) = reg.get(parent_id) {
                            parent_atom.end_byte.store(midpoint, Ordering::SeqCst);
                            parent_atom.split_count.fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    let new_segment = FileSegment {
                        id: new_id,
                        start_byte: midpoint + 1,
                        current_offset: midpoint + 1,
                        end_byte: old_end,
                        status: SegmentStatus::Downloading,
                        split_count: 0,
                        last_speed_bps: 0,
                    };
                    if let Some(reg) = registry {
                        reg.insert(&new_segment);
                    }
                    m.segments.push(new_segment);
                    m.push_log(
                        Some(new_id),
                        "INFO",
                        format!(
                            "قسمة ديناميكية نصفية للمقطع #{}: إنشاء المقطع #{} للنطاق {}..{}",
                            idx,
                            new_id,
                            midpoint + 1,
                            old_end
                        ),
                    );
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
        atomic_registry: &Arc<AtomicSegmentRegistry>,
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
            seg_atomic,
        ) = {
            let mut m = meta.lock().await;
            atomic_registry.sync_to_meta(&mut m);
            let seg = m
                .segments
                .iter()
                .find(|s| s.id == seg_id)
                .ok_or_else(|| anyhow!("المقطع غير موجود"))?;
            if seg.current_offset > seg.end_byte {
                return Ok(());
            }
            let atom = atomic_registry
                .get(seg_id)
                .unwrap_or_else(|| atomic_registry.insert(seg));
            atom.status_code
                .store(SegmentStatus::Downloading.to_u8(), Ordering::Release);
            let if_range = m.etag.clone().or_else(|| m.last_modified.clone());
            let resumed = seg.current_offset > seg.start_byte;
            (
                m.current_effective_url(),
                seg.current_offset,
                seg.end_byte,
                m.total_size,
                m.resumable,
                m.custom_headers.clone(),
                if_range,
                resumed,
                atom,
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

        if status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::GONE
        {
            expired_url_flag.store(true, Ordering::SeqCst);
            return Err(anyhow!("URL_EXPIRED: HTTP {}", status));
        }

        resp.error_for_status_ref()?;

        if resumable && (start_offset > 0 || is_resumed) && status == reqwest::StatusCode::OK {
            if is_resumed {
                resource_mutated_flag.store(true, Ordering::SeqCst);
                return Err(anyhow!(
                    "RESOURCE_MUTATED: أعاد الخادم 200 OK عند الاستئناف بـ If-Range مما يدل على تغير الملف"
                ));
            } else {
                seg_atomic
                    .status_code
                    .store(SegmentStatus::Completed.to_u8(), Ordering::Release);
                let mut m = meta.lock().await;
                m.resumable = false;
                if let Some(seg) = m.segments.iter_mut().find(|s| s.id == seg_id) {
                    seg.status = SegmentStatus::Completed;
                }
                return Err(anyhow!("RANGE_IGNORED: الخادم تجاهل ترويسة Range"));
            }
        }

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

        // حلقة القراءة السريعة (Hot Streaming Loop) — خالية تماماً من أقفال Mutex!
        // تعتمد 100% على تعليمات المعالج الذرية (AtomicU64 Load/Store) لمنع أي اختناق.
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
                let mut m = meta.lock().await;
                atomic_registry.sync_to_meta(&mut m);
                return Ok(());
            }

            let chunk = chunk_res?;
            speed_limiter.acquire(chunk.len()).await;

            // قراءة ذرية فورية لنهاية المقطع (تتحدث تلقائياً إذا تعرض المقطع للقسمة النصفية أثناء التحميل)
            let dynamic_end = seg_atomic.end_byte.load(Ordering::Acquire);

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

            // تحديث ذري خالٍ من الأقفال لمؤشر البايت الحالي وسرعة التدفق اللحظية
            seg_atomic
                .current_offset
                .store(current_pos, Ordering::Release);
            seg_atomic
                .last_speed_bps
                .store(current_bps, Ordering::Relaxed);

            if current_pos > seg_atomic.end_byte.load(Ordering::Acquire) {
                seg_atomic
                    .status_code
                    .store(SegmentStatus::Completed.to_u8(), Ordering::Release);
                seg_atomic.last_speed_bps.store(0, Ordering::Relaxed);
                file.flush().await?;
                let mut m = meta.lock().await;
                atomic_registry.sync_to_meta(&mut m);
                return Ok(());
            }

            if write_len < chunk.len() {
                break;
            }
        }

        file.flush().await?;
        {
            let mut m = meta.lock().await;
            atomic_registry.sync_to_meta(&mut m);
            let total_size = m.total_size;
            if let Some(seg) = m.segments.iter_mut().find(|s| s.id == seg_id) {
                if seg.current_offset > seg.end_byte || total_size == 0 {
                    seg.status = SegmentStatus::Completed;
                    seg.last_speed_bps = 0;
                    seg_atomic
                        .status_code
                        .store(SegmentStatus::Completed.to_u8(), Ordering::Release);
                    seg_atomic.last_speed_bps.store(0, Ordering::Relaxed);
                }
            }
        }

        Ok(())
    }
}

// ============================================================================
// 4. فاحص ومصلح الفجوات للمقاطع المتعددة (Segment Map Integrity Auditor & Gap Healer)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentAuditReport {
    pub is_contiguous: bool,
    pub total_covered_bytes: u64,
    pub expected_total_bytes: u64,
    pub detected_gaps: Vec<(u64, u64)>,
    pub overlapping_ranges: Vec<(usize, usize)>,
}

pub struct SegmentIntegrityAuditor;

impl SegmentIntegrityAuditor {
    /// فحص رياضي صارم لحدود جميع المقاطع للتأكد من عدم وجود أي فجوة بايتية أو تداخل بين العمال المتوازيين
    pub fn audit_segments(segments: &[FileSegment], total_size: u64) -> SegmentAuditReport {
        if segments.is_empty() || total_size == 0 {
            return SegmentAuditReport {
                is_contiguous: total_size == 0,
                total_covered_bytes: 0,
                expected_total_bytes: total_size,
                detected_gaps: Vec::new(),
                overlapping_ranges: Vec::new(),
            };
        }

        let mut sorted: Vec<FileSegment> = segments.to_vec();
        sorted.sort_by_key(|s| s.start_byte);

        let mut gaps = Vec::new();
        let mut overlaps = Vec::new();
        let mut covered = 0u64;

        if sorted[0].start_byte > 0 {
            gaps.push((0, sorted[0].start_byte - 1));
        }

        for i in 0..sorted.len() {
            let seg = &sorted[i];
            if seg.end_byte >= seg.start_byte && seg.end_byte < u64::MAX {
                covered = covered.saturating_add(seg.end_byte - seg.start_byte + 1);
            }

            if i + 1 < sorted.len() {
                let next = &sorted[i + 1];
                if seg.end_byte + 1 < next.start_byte {
                    gaps.push((seg.end_byte + 1, next.start_byte - 1));
                } else if seg.end_byte >= next.start_byte {
                    overlaps.push((seg.id, next.id));
                }
            }
        }

        if let Some(last) = sorted.last() {
            if last.end_byte < total_size.saturating_sub(1) {
                gaps.push((last.end_byte + 1, total_size - 1));
            }
        }

        let is_contiguous = gaps.is_empty() && overlaps.is_empty() && covered == total_size;

        SegmentAuditReport {
            is_contiguous,
            total_covered_bytes: covered,
            expected_total_bytes: total_size,
            detected_gaps: gaps,
            overlapping_ranges: overlaps,
        }
    }

    /// إصلاح تلقائي لأي فجوات مكتشفة في خريطة المقاطع عن طريق إنشاء مقاطع تعويضية (Gap Healing)
    pub fn heal_gaps(meta: &mut DownloadStateMeta) -> usize {
        if meta.total_size == 0 || !meta.resumable {
            return 0;
        }
        let report = Self::audit_segments(&meta.segments, meta.total_size);
        if report.detected_gaps.is_empty() {
            return 0;
        }

        let mut added = 0usize;
        for (gap_start, gap_end) in report.detected_gaps {
            let next_id = meta.segments.iter().map(|s| s.id).max().unwrap_or(0) + 1;
            meta.segments.push(FileSegment {
                id: next_id,
                start_byte: gap_start,
                current_offset: gap_start,
                end_byte: gap_end,
                status: SegmentStatus::Pending,
                split_count: 0,
                last_speed_bps: 0,
            });
            added += 1;
        }
        meta.segments.sort_by_key(|s| s.start_byte);
        meta.push_log(
            None,
            "WARN",
            format!("تم إصلاح {} فجوات بايتية تلقائياً في خريطة المقاطع", added),
        );
        added
    }
}
