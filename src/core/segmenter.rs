//! محرك التقسيم الديناميكي النصفي مع التحديث الذري لتقدم المقاطع في حلقة التدفق (`SegmentAtomicTelemetry`)
//! يطبق:
//! 1. دعم الملفات المتناثرة على ويندوز (`FSCTL_SET_SPARSE` عبر `make_sparse`) والكتابة الموضعية المتزامنة (`write_at` / `seek_write`) على مقبض ملف مشترك (`Arc<std::fs::File>`).
//! 2. تجزئة طلبات خوادم يوتيوب (`googlevideo.com`) إلى نوافذ `10 MB` (`max_request_for`) لكسر خنق السرعة.
//! 3. التحقق الصارم من ترويسة `Content-Range` في استجابات `206 Partial Content` (`validate_partial_content_range`) ورفض الاستجابات الفارغة أو المنقطعة مبكراً (`EMPTY_RESPONSE_STREAM` / `STREAM_TRUNCATED`).
//! 4. السياسة المحافظة لتجديد الروابط المنتهية (`is_refreshed_resource_compatible`) ومطابقة حجم ملف `.part` الفعلي على القرص مع `.idm.json` (`validate_part_file_against_segments`).

use crate::core::{
    adaptive::{AdaptiveConfig, AdaptiveController, AdaptiveSample},
    lanes::global_network_lane_pool,
    limiter::SpeedLimiter,
    path_safety::open_part_file_no_follow,
    publisher::{discover_publisher_checksum, verify_file_against_publisher_checksum},
    resume_verify::{
        compute_part_file_stamp, evaluate_resume_validators, hash_completed_segments_in_manifest,
        reverify_completed_segments_with_manifest, verify_saved_samples_against_server,
        PartVerifyManifest, ResumeProof,
    },
    storage::{
        compute_file_all_digests, extract_filename, save_meta_to_disk_atomic, sync_parent_dir,
        DownloadStateMeta, DownloadStatus, FileSegment, SegmentStatus,
    },
    tools::{
        apply_server_last_modified_time, check_disk_room_and_filesystem, mark_file_from_internet,
        run_virus_scanner_on_file, set_system_awake_for_downloads, validate_safe_executable_path,
    },
};
use anyhow::{anyhow, bail, Context, Result};
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
    fs::File as StdFile,
    io,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
        Arc, RwLock as StdRwLock,
    },
    time::{Duration, Instant},
};
use tokio::{
    fs,
    sync::Mutex,
    time::{sleep, timeout},
};
use url::Url;

// ============================================================================
// 0. الكتابة الموضعية المتزامنة والملفات المتناثرة (NTFS Sparse & Positional File I/O)
// ============================================================================

/// الحد الأقصى لحجم نافذة طلب `Range` الواحد لخوادم `googlevideo.com` (10 ميجابايت) لمنع خنق السرعة
pub const YOUTUBE_RANGE_WINDOW_BYTES: u64 = 10 * 1024 * 1024;

/// بناء عميل HTTP محسن للتحميل متعدد الاتصالات (مستلهم من `ezoo-download/crates/engine/src/http.rs:162-171`):
/// - يفرض `.http1_only()` لمنع دمج الاتصالات المتوازية في اتصال TCP واحد عبر HTTP/2 Multiplexing.
/// - يفعّل `.tcp_nodelay(true)` لتقليل زمن استجابة إرسال طلبات `Range`.
pub fn build_optimized_http_client(user_agent: &str, timeout_secs: u64) -> Client {
    Client::builder()
        .user_agent(user_agent)
        .http1_only()
        .tcp_nodelay(true)
        .connect_timeout(Duration::from_secs(timeout_secs.max(5)))
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// فحص ما إذا كان الخطأ ناتجاً عن انقطاع الشبكة المؤقت (انقطاع الواي فاي، DNS، المهلة، إعادة ضبط الاتصال)
/// وليس رفضاً صريحاً من خادم HTTP (401/403/404/410) — مستلهم من `ezoo-download/crates/engine/src/http.rs:61-81`.
pub fn is_transient_network_error(err_msg: &str) -> bool {
    let lower = err_msg.to_ascii_lowercase();
    if lower.contains("url_expired")
        || lower.contains("resource_mutated")
        || lower.contains("range_ignored")
        || lower.contains("http_expired_or_forbidden")
    {
        return false;
    }
    lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("انتهت مهلة")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("broken pipe")
        || lower.contains("dns error")
        || lower.contains("failed to lookup")
        || lower.contains("network is unreachable")
        || lower.contains("software caused connection abort")
        || lower.contains("empty_response_stream")
        || lower.contains("stream_truncated")
}

/// مصفوفة التراجع الذكي للترويسات عند فحص الرابط (`Probe Header Fallbacks`)
/// مستلهمة من `ezoo-download/crates/engine/src/http.rs:232-284`:
/// عندما يرفض الخادم الفحص الأولي بحالة `403 / 406 / 416 / 429`، يجرّب المحرك التباديل التالية بالترتيب:
/// 1. `(with_range = true, الترويسات الكاملة بما فيها الكوكيز والـ Referer)`
/// 2. `(with_range = false, الترويسات الكاملة بدون ترويسة Range)`
/// 3. `(with_range = true, الترويسات بدون Cookie — لأن بعض الخوادم ترفض كوكيز الجلسة الطويلة)`
/// 4. `(with_range = false, بدون Cookie وبدون Referer)`
pub fn build_probe_fallback_matrix(custom_headers: &HeaderMap) -> Vec<(bool, HeaderMap)> {
    let has_cookie = custom_headers.contains_key(reqwest::header::COOKIE);
    let has_referer = custom_headers.contains_key(reqwest::header::REFERER);

    let mut without_cookies = custom_headers.clone();
    without_cookies.remove(reqwest::header::COOKIE);

    let mut minimal_headers = without_cookies.clone();
    minimal_headers.remove(reqwest::header::REFERER);

    let mut matrix = vec![
        (true, custom_headers.clone()),
        (false, custom_headers.clone()),
    ];
    if has_cookie {
        matrix.push((true, without_cookies));
    }
    if has_cookie || has_referer {
        matrix.push((false, minimal_headers));
    }
    matrix
}

/// إشارة إيقاظ فورية مشتركة لجميع القطاعات النشطة عند عودة الاتصال بالشبكة (`back_online.notify_waiters()`)
static GLOBAL_BACK_ONLINE_NOTIFY: std::sync::OnceLock<tokio::sync::Notify> =
    std::sync::OnceLock::new();

pub fn global_back_online_notify() -> &'static tokio::sync::Notify {
    GLOBAL_BACK_ONLINE_NOTIFY.get_or_init(tokio::sync::Notify::new)
}


/// الحد الأدنى من التقدم الفعلي (64 كيلوبايت) لتصفير عداد محاولات الاتصال عند حدوث انقطاع شبكي
pub const MIN_PROGRESS_TO_RESET_RETRIES: u64 = 64 * 1024;

/// فحص ما إذا كان الخادم يبطئ طلبات النطاق الكبيرة (مثل `*.googlevideo.com`) وتحديد سقف الطلب الواحد بـ 10 ميجابايت
pub fn max_request_for(url: &str) -> Option<u64> {
    let host = Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    if host == "googlevideo.com" || host.ends_with(".googlevideo.com") {
        Some(YOUTUBE_RANGE_WINDOW_BYTES)
    } else {
        None
    }
}

/// كتابة موضعية متزامنة (`Positional Write`) تتيح لـ 32 اتصالاً الكتابة في نفس الملف (`Arc<StdFile>`)
/// في آنٍ واحد دون الحاجة لقفل الملف أو تحريك مؤشر القراءة والكتابة المشترك (`Seek`).
#[allow(unused_mut)]
pub fn write_at(file: &StdFile, buf: &[u8], mut offset: u64) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.write_all_at(buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut rest = buf;
        while !rest.is_empty() {
            let n = file.seek_write(rest, offset)?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "تعذر كتابة البيانات في الإزاحة المحددة على القرص",
                ));
            }
            rest = &rest[n..];
            offset += n as u64;
        }
        Ok(())
    }
}

/// تفعيل خاصية الملف المتناثر (`FSCTL_SET_SPARSE`) على أنظمة ملفات NTFS في ويندوز
/// لمنع نظام التشغيل من ملء الملفات الضخمة (10GB+) بالأصفار وتجميد البرنامج عند بدء التحميل.

/// سعة مخزن التجميع الدفعي للكتابة الموضعية لكل قطاع نشط (256 كيلوبايت)
/// يقلل عدد استدعاءات النظام للقرص (`write_at` IOPS) بمقدار 16x إلى 32x عند التحميل بـ 64 اتصالاً متوازياً!
pub const SEGMENT_WRITE_COALESCE_BYTES: usize = 256 * 1024;

/// الحد الأقصى لبقاء البيانات في مخزن التجميع قبل تفريغها للقرص (250 مللي ثانية) لضمان حداثة البيانات على الاتصالات البطيئة
pub const SEGMENT_WRITE_FLUSH_INTERVAL: Duration = Duration::from_millis(250);

/// مجمّع كتابة موضعي ذكي (`SegmentWriteCoalescer`) يجمع شرائح الشبكة الصغيرة (`8KB–32KB`)
/// في كتل متصلة بحجم `256 KB` قبل استدعاء `write_at` على القرص، مع تفريغ تلقائي عند الإغلاق (`Drop`).
pub struct SegmentWriteCoalescer<'a> {
    file: &'a StdFile,
    buf: Vec<u8>,
    buf_start_offset: u64,
    last_flush: Instant,
}

impl<'a> SegmentWriteCoalescer<'a> {
    pub fn new(file: &'a StdFile, initial_offset: u64) -> Self {
        Self {
            file,
            buf: Vec::with_capacity(SEGMENT_WRITE_COALESCE_BYTES),
            buf_start_offset: initial_offset,
            last_flush: Instant::now(),
        }
    }

    pub fn push(&mut self, data: &[u8], offset: u64) -> io::Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        if self.buf.is_empty() {
            self.buf_start_offset = offset;
        } else if self.buf_start_offset + (self.buf.len() as u64) != offset
            || self.buf.len() + data.len() > SEGMENT_WRITE_COALESCE_BYTES
        {
            self.flush()?;
            self.buf_start_offset = offset;
        }

        if data.len() >= SEGMENT_WRITE_COALESCE_BYTES {
            write_at(self.file, data, offset)?;
            self.buf_start_offset = offset + data.len() as u64;
            self.last_flush = Instant::now();
        } else {
            self.buf.extend_from_slice(data);
            if self.buf.len() >= SEGMENT_WRITE_COALESCE_BYTES
                || self.last_flush.elapsed() >= SEGMENT_WRITE_FLUSH_INTERVAL
            {
                self.flush()?;
            }
        }
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        if !self.buf.is_empty() {
            write_at(self.file, &self.buf, self.buf_start_offset)?;
            self.buf_start_offset += self.buf.len() as u64;
            self.buf.clear();
            self.last_flush = Instant::now();
        }
        Ok(())
    }
}

impl Drop for SegmentWriteCoalescer<'_> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

pub fn make_sparse(file: &StdFile) {
    #[cfg(windows)]
    unsafe {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Ioctl::FSCTL_SET_SPARSE;
        use windows_sys::Win32::System::IO::DeviceIoControl;
        let mut returned: u32 = 0;
        DeviceIoControl(
            file.as_raw_handle() as _,
            FSCTL_SET_SPARSE,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        );
    }
    #[cfg(not(windows))]
    let _ = file;
}

// ============================================================================
// 1. الإحصائيات الذرية لتقدم المقاطع النشطة (Atomic Segment Progress Telemetry)
// ============================================================================

/// هيكل ذري لتتبع تقدم وسرعة ونهاية كل مقطع أثناء تدفق البيانات دون قفل بنية المهمة في كل جزء (`chunk`)
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

/// سجل مركزي للعدادات الذرية للمقاطع النشطة يتيح لعمال الشبكة تحديث الإزاحة والسرعة بدون قفل `DownloadStateMeta`
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
        let mut guard = self.segments.write().unwrap_or_else(|p| p.into_inner());
        guard.clear();
        for seg in &meta.segments {
            guard.insert(seg.id, SegmentAtomicTelemetry::from_segment(seg));
        }
    }

    pub fn get(&self, seg_id: usize) -> Option<Arc<SegmentAtomicTelemetry>> {
        self.segments.read().unwrap_or_else(|p| p.into_inner()).get(&seg_id).cloned()
    }

    pub fn insert(&self, seg: &FileSegment) -> Arc<SegmentAtomicTelemetry> {
        let arc = SegmentAtomicTelemetry::from_segment(seg);
        self.segments
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(seg.id, Arc::clone(&arc));
        arc
    }

    /// مزامنة القراءات الذرية اللحظية إلى هيكل `DownloadStateMeta` دون إبطاء حلقات الشبكة
    pub fn sync_to_meta(&self, meta: &mut DownloadStateMeta) {
        let guard = self.segments.read().unwrap_or_else(|p| p.into_inner());
        for seg in meta.segments.iter_mut() {
            if let Some(atom) = guard.get(&seg.id) {
                let snap = atom.to_file_segment();
                seg.current_offset = snap.current_offset;
                seg.end_byte = snap.end_byte;
                seg.last_speed_bps = snap.last_speed_bps;
                seg.split_count = snap.split_count;
                let atomic_status = snap.status;
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
// 2. التحقق الصارم من HTTP Range وتوافق تجديد الروابط وسلامة ملف .part
// ============================================================================

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

/// التحقق الصارم من استجابة `206 Partial Content` وفق معيار RFC 9110 §15.3.7:
/// 1. يرفض غياب ترويسة `Content-Range` أو تلف صيغتها في استجابات `206`.
/// 2. يتحقق من تطابق بداية النطاق `cr_start` مع المؤشر المطلوب `expected_start`.
/// 3. يتحقق من عدم تغير الحجم الإجمالي للملف `cr_total` مقارنة بـ `expected_total`.
/// 4. يرفض الاستجابة إذا كانت ترويسة `Content-Length` تساوي `0`.
pub fn validate_partial_content_range(
    status: reqwest::StatusCode,
    headers: &HeaderMap,
    expected_start: u64,
    expected_total: u64,
) -> Result<(u64, u64, Option<u64>)> {
    if status != reqwest::StatusCode::PARTIAL_CONTENT {
        bail!(
            "UNEXPECTED_RANGE_STATUS: الحالة المتوقعة 206 Partial Content ولكن الخادم أعاد {}",
            status
        );
    }

    if let Some(cl_val) = headers.get(CONTENT_LENGTH).and_then(|v| v.to_str().ok()) {
        if let Ok(0) = cl_val.trim().parse::<u64>() {
            bail!("EMPTY_RESPONSE_STREAM: استجاب الخادم بـ Content-Length: 0 للنطاق المطلوب");
        }
    }

    let cr_raw = headers
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            anyhow!(
                "MISSING_CONTENT_RANGE: استجاب الخادم بـ 206 Partial Content بدون ترويسة Content-Range إلزامية"
            )
        })?;

    let (cr_start, cr_end, cr_total) = parse_content_range_header(cr_raw).ok_or_else(|| {
        anyhow!(
            "INVALID_CONTENT_RANGE: صيغة ترويسة Content-Range غير صالحة ({})",
            cr_raw
        )
    })?;

    if cr_start != expected_start {
        bail!(
            "CONTENT_RANGE_MISMATCH: بداية Content-Range ({}) لا تطابق المؤشر المطلوب ({})",
            cr_start,
            expected_start
        );
    }

    if let Some(tot) = cr_total {
        if expected_total > 0 && tot != expected_total {
            bail!(
                "RESOURCE_MUTATED: تغير حجم الملف على الخادم من {} إلى {}",
                expected_total,
                tot
            );
        }
    }

    Ok((cr_start, cr_end, cr_total))
}

/// فحص صارم ومحافظ لتوافق الرابط المجدد مع البيانات الجزئية المحملة سابقاً:
/// - يشترط أن يدعم الرابط الجديد الاستئناف (`new_resumable == true`) وأن يكون الحجم معلوماً (`> 0`) ومتطابقاً تماماً.
/// - إذا كان للملف الأصلي `ETag` قوي (لا يبدأ بـ `W/`)، يجب أن يرسل الرابط الجديد نفس الـ `ETag` القوي.
/// - عند غياب `ETag` القوي، يشترط وجود وتطابق ترويسة `Last-Modified` في كلا الرابطين.
/// - إذا غاب كلٌ من `ETag` القوي و`Last-Modified`، يرفض افتراض التوافق ويعيد التحميل من الصفر بأمان لمنع دمج ملفين مختلفين.
pub fn is_refreshed_resource_compatible(
    old_size: u64,
    new_size: u64,
    new_resumable: bool,
    old_etag: Option<&str>,
    new_etag: Option<&str>,
    old_last_modified: Option<&str>,
    new_last_modified: Option<&str>,
) -> bool {
    if !new_resumable || old_size == 0 || new_size != old_size {
        return false;
    }

    fn strong_etag(e: Option<&str>) -> Option<&str> {
        let trimmed = e?.trim();
        if trimmed.is_empty() || trimmed.starts_with("W/") || trimmed.starts_with("w/") {
            None
        } else {
            Some(trimmed)
        }
    }

    match (strong_etag(old_etag), strong_etag(new_etag)) {
        (Some(old_s), Some(new_s)) => {
            if old_s != new_s {
                return false;
            }
            // إذا توفرت أيضاً Last-Modified في كليهما، يجب ألا تتعارض
            if let (Some(old_lm), Some(new_lm)) = (old_last_modified, new_last_modified) {
                if !old_lm.trim().is_empty()
                    && !new_lm.trim().is_empty()
                    && old_lm.trim() != new_lm.trim()
                {
                    return false;
                }
            }
            true
        }
        (Some(_), None) => {
            // كان للمورد ETag قوي واختفى في الرابط الجديد -> لا يمكن إثبات تطابق المورد
            false
        }
        (None, _) => {
            // لا يوجد ETag قوي للمقارنة؛ نشترط وجود وتطابق Last-Modified غير الفارغة
            match (old_last_modified, new_last_modified) {
                (Some(old_lm), Some(new_lm)) => {
                    let o = old_lm.trim();
                    let n = new_lm.trim();
                    !o.is_empty() && o == n
                }
                _ => false,
            }
        }
    }
}

/// التحقق من تطابق إزاحات المقاطع المحفوظة في `.idm.json` مع الحجم الفعلي لملف `.part` على القرص:
/// إذا كانت المقاطع تدعي تحميل بايتات تتجاوز حجم ملف `.part` الموجود فعلياً على القرص (أو كان ملف `.part` مفقوداً أو بحجم 0)،
/// يتم تصفير المقاطع فوراً وإعادة التحميل من البداية لمنع الفجوات الصامتة وتلف الملف النهائي.
pub fn validate_part_file_against_segments(
    meta: &mut DownloadStateMeta,
    actual_part_file_len: u64,
) -> bool {
    let max_claimed_offset = meta
        .segments
        .iter()
        .filter(|s| s.current_offset > s.start_byte || s.status == SegmentStatus::Completed)
        .map(|s| s.current_offset)
        .max()
        .unwrap_or(0);

    if max_claimed_offset > 0 && (actual_part_file_len == 0 || actual_part_file_len < max_claimed_offset) {
        meta.segments.clear();
        meta.push_log(
            None,
            "WARN",
            format!(
                "تعارض في الاستعادة: سجل المقاطع يشير إلى الإزاحة {} بينما حجم ملف .part الفعلي على القرص {} بايت؛ تمت إعادة تهيئة المقاطع لضمان سلامة الملف",
                max_claimed_offset, actual_part_file_len
            ),
        );
        return false;
    }

    if !meta.segments.is_empty() && meta.total_size > 0 && meta.resumable {
        SegmentIntegrityAuditor::heal_gaps(meta);
        let _ = crate::core::resume_verify::verify_exact_segment_coverage(
            &meta.segments,
            meta.total_size,
        );
    }

    true
}

pub trait UrlRefreshProvider: Send + Sync {
    fn provider_name(&self) -> &'static str;
    fn can_handle(&self, meta: &DownloadStateMeta) -> bool;
}

/// مزود قياسي يعيد فحص صفحة المصدر (source_page_url) لاستخراج رابط موقع جديد لنفس اسم الملف
pub struct SourcePageRefreshAdapter;

impl UrlRefreshProvider for SourcePageRefreshAdapter {
    fn provider_name(&self) -> &'static str {
        "source-page-scraper"
    }

    fn can_handle(&self, meta: &DownloadStateMeta) -> bool {
        meta.source_page_url
            .as_ref()
            .is_some_and(|u| !u.trim().is_empty())
    }
}

impl SourcePageRefreshAdapter {
    pub async fn try_refresh_from_source_page(
        client: &Client,
        meta: &DownloadStateMeta,
    ) -> Result<String> {
        let adapter = SourcePageRefreshAdapter;
        if !adapter.can_handle(meta) {
            bail!(
                "مزود {} لا يملك رابط صفحة مصدر لتجديد الرابط",
                adapter.provider_name()
            );
        }
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
    /// استكشاف الرابط المبدئي مع مصفوفة التراجع الذكي (`build_probe_fallback_matrix`) والتحقق من دعم Range و Content-Range
    pub async fn probe_url(
        client: &Client,
        url: &str,
        custom_headers: &HeaderMap,
    ) -> Result<(u64, bool, String, Option<String>, Option<String>)> {
        let candidates = build_probe_fallback_matrix(custom_headers);
        let mut chosen_resp: Option<reqwest::Response> = None;

        for (idx, (use_range, hdrs)) in candidates.iter().enumerate() {
            let mut req = client.get(url).headers(hdrs.clone());
            if *use_range {
                req = req.header(RANGE, "bytes=0-0");
            }
            let send_res = timeout(Duration::from_secs(15), req.send()).await;
            match send_res {
                Ok(Ok(r)) => {
                    let last_status = r.status();
                    if r.status().is_success() {
                        chosen_resp = Some(r);
                        break;
                    }
                    // إذا كان الخطأ قابلاً للتعافي بتغيير الترويسات (400/403/406/416/429/501) نجرّب البديل التالي في المصفوفة
                    let retryable_probe_status = matches!(
                        last_status,
                        reqwest::StatusCode::FORBIDDEN
                            | reqwest::StatusCode::NOT_ACCEPTABLE
                            | reqwest::StatusCode::RANGE_NOT_SATISFIABLE
                            | reqwest::StatusCode::TOO_MANY_REQUESTS
                            | reqwest::StatusCode::BAD_REQUEST
                            | reqwest::StatusCode::METHOD_NOT_ALLOWED
                            | reqwest::StatusCode::NOT_IMPLEMENTED
                    );
                    if !retryable_probe_status || idx + 1 == candidates.len() {
                        chosen_resp = Some(r);
                        break;
                    }
                }
                Ok(Err(e)) => {
                    if idx == 0 {
                        return Err(e).context("فشل الاتصال المبدئي بالخادم");
                    }
                }
                Err(_) => {
                    if idx == 0 {
                        bail!("انتهت مهلة الاتصال المبدئي بالخادم");
                    }
                }
            }
        }

        let mut resp = chosen_resp.ok_or_else(|| anyhow!("فشل الاتصال المبدئي بالخادم"))?;
        let mut status = resp.status();

        // محاولة أخيرة بطلب HEAD إذا رفض الخادم طلبات GET الاستكشافية
        if matches!(
            status,
            reqwest::StatusCode::RANGE_NOT_SATISFIABLE
                | reqwest::StatusCode::BAD_REQUEST
                | reqwest::StatusCode::METHOD_NOT_ALLOWED
                | reqwest::StatusCode::NOT_IMPLEMENTED
        ) {
            if let Ok(Ok(head_resp)) = timeout(
                Duration::from_secs(10),
                client.head(url).headers(custom_headers.clone()).send(),
            )
            .await
            {
                if head_resp.status().is_success() {
                    resp = head_resp;
                    status = resp.status();
                }
            }
        }

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
            if let Some((cr_s, _e, Some(tot))) = parse_content_range_header(cr) {
                if cr_s == 0 {
                    total_size = tot;
                } else {
                    resumable = false;
                }
            } else if status == reqwest::StatusCode::PARTIAL_CONTENT {
                resumable = false;
            }
        } else if status == reqwest::StatusCode::PARTIAL_CONTENT {
            // إذا أعاد الخادم 206 بدون Content-Range صالحة، لا نثق بدعمه للتقسيم الجزئي
            resumable = false;
        }

        if total_size == 0 {
            total_size = headers
                .get(CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
        }

        let final_redirect_url = resp.url().as_str();
        let mut filename = extract_filename(final_redirect_url, headers);
        if filename.starts_with("download_") && final_redirect_url != url {
            filename = extract_filename(url, headers);
        }
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

    /// التحقق الصارم من رابط جديد وتحديث المهمة بأمان دون المخاطرة بدمج ملفين مختلفين
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

        let (old_size, old_etag, old_last_mod, part_path_snap, segs_snap) = {
            let m = self.meta.lock().await;
            (
                m.total_size,
                m.etag.clone(),
                m.last_modified.clone(),
                m.part_file_path(),
                m.segments.clone(),
            )
        };

        let mut resource_compatible = is_refreshed_resource_compatible(
            old_size,
            new_size,
            new_resumable,
            old_etag.as_deref(),
            new_etag.as_deref(),
            old_last_mod.as_deref(),
            new_last_mod.as_deref(),
        );

        // ميزة VeloxFetch #1: إذا تطابق الحجم ودعم الخادم Range ولكن غاب ETag القوي (أو كان ضعيفاً)،
        // نتحقق من تطابق نوافذ العينات الفعلية من ملف .part مع الخادم (Sample-Based Resume Verification)
        if !resource_compatible && old_size > 0 && old_size == new_size && new_resumable {
            if let Ok(proof) = evaluate_resume_validators(
                old_etag.as_deref(),
                old_last_mod.as_deref(),
                new_etag.as_deref(),
                new_last_mod.as_deref(),
            ) {
                if proof == ResumeProof::Proven
                    || verify_saved_samples_against_server(
                        &self.client,
                        new_url,
                        &custom_headers_map,
                        &part_path_snap,
                        &segs_snap,
                    )
                    .await
                    .unwrap_or(false)
                {
                    resource_compatible = true;
                }
            }
        }

        let mut m = self.meta.lock().await;

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
            m.push_log(
                None,
                "INFO",
                "تم التحقق من تطابق هوية المورد (ETag / Last-Modified + الحجم) واستئناف المقاطع الجزئية بأمان",
            );
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
            m.push_log(
                None,
                "WARN",
                "تعذر إثبات تطابق هوية الملف الجديد مع المقاطع السابقة؛ تمت إعادة التحميل من البداية لمنع تلف البيانات",
            );
            let meta_path = m.meta_file_path();
            save_meta_to_disk_atomic(&m, &meta_path).await?;
            Ok(false)
        }
    }

    /// تنفيذ التحميل مع التحديث الذري لتقدم المقاطع، والملفات المتناثرة (`FSCTL_SET_SPARSE`)،
    /// والتحقق من سلامة ملف `.part` على القرص قبل الاستئناف.
    pub async fn execute(&self) -> Result<PathBuf> {
        set_system_awake_for_downloads(true);
        loop {
            self.resource_mutated_flag.store(false, Ordering::SeqCst);
            self.expired_url_flag.store(false, Ordering::SeqCst);

            let (part_path, meta_path, final_path, total_size, resumable, save_dir, already_done) = {
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
                    m.save_dir.clone(),
                    m.total_downloaded(),
                )
            };

            if total_size > 0 {
                let needed = total_size.saturating_sub(already_done);
                if let Err(disk_err) =
                    check_disk_room_and_filesystem(&save_dir, needed, Some(total_size))
                {
                    let mut m = self.meta.lock().await;
                    m.status = DownloadStatus::Failed;
                    m.last_error = Some(disk_err.to_string());
                    let _ = save_meta_to_disk_atomic(&m, &meta_path).await;
                    return Err(disk_err);
                }
            }

            // فحص الحجم الفعلي لملف .part على القرص قبل تعديله للتأكد من توافقه مع سجل المقاطع المحفوظ
            let existing_part_len = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);

            let shared_part_file = {
                let mut m = self.meta.lock().await;
                validate_part_file_against_segments(&mut m, existing_part_len);

                // ميزة VeloxFetch #2: التحقق من ختم ملف .part وإعادة فحص بصمات SHA-256 للقطاعات المكتملة إذا تغير الختم بعد إغلاق غير نظيف
                if existing_part_len > 0 && !m.segments.is_empty() {
                    let mut v_manifest = PartVerifyManifest::load(&part_path);
                    let current_stamp = compute_part_file_stamp(&part_path);
                    if v_manifest.stamp.is_some()
                        && v_manifest.stamp != current_stamp
                        && !v_manifest.segment_hashes.is_empty()
                    {
                        if let Ok(rep) = reverify_completed_segments_with_manifest(
                            &part_path,
                            &mut m.segments,
                            &mut v_manifest,
                        ) {
                            v_manifest.save(&part_path);
                            if rep.invalid > 0 {
                                m.push_log(
                                    None,
                                    "WARN",
                                    format!(
                                        "فحص سلامة القطاعات (Per-Segment SHA-256): تم اكتشاف تلف في {} قطاع ({} بايت) وإعادة جدولته وحده للتنزيل",
                                        rep.invalid, rep.invalid_bytes
                                    ),
                                );
                            }
                        }
                    }
                }

                // ميزة VeloxFetch #8: فتح ملف .part مع رفض الروابط الرمزية المزروعة (Symlink Guard)
                let f = open_part_file_no_follow(&part_path)
                    .context("فشل إنشاء أو فتح ملف .part على القرص (أو تم رفض رابط رمزي مشبوه)")?;

                if total_size > 0 && resumable {
                    let cur_len = f.metadata().map(|md| md.len()).unwrap_or(0);
                    if cur_len != total_size {
                        make_sparse(&f);
                        f.set_len(total_size).context(
                            "فشل حجز المساحة المطلوبة على القرص (تحقق من المساحة المتوفرة)",
                        )?;
                    }
                } else {
                    // غير قابل للاستئناف: نبدأ دائماً من الصفر
                    m.segments.clear();
                    let _ = f.set_len(0);
                }
                Arc::new(f)
            };

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

            let initial_target_conns = self.max_connections.max(1);
            let adaptive_ceiling = if resumable && total_size >= self.min_split_size * 4 {
                self.max_connections.max(16).min(32)
            } else {
                initial_target_conns
            };
            let active_worker_limit = Arc::new(AtomicUsize::new(initial_target_conns));
            let throttled_signal = Arc::new(AtomicBool::new(false));

            let meta_saver_clone = Arc::clone(&self.meta);
            let pause_saver_clone = Arc::clone(&self.pause_flag);
            let registry_saver_clone = Arc::clone(&atomic_registry);
            let meta_path_clone = meta_path.clone();
            let part_file_saver_clone = Arc::clone(&shared_part_file);
            let worker_limit_saver = Arc::clone(&active_worker_limit);
            let throttled_saver = Arc::clone(&throttled_signal);
            let saver_handle = tokio::spawn(async move {
                let mut tick_count = 0u64;
                let mut adaptive_ctrl = AdaptiveController::new(
                    AdaptiveConfig::default(),
                    initial_target_conns,
                    adaptive_ceiling,
                )
                .with_floor(initial_target_conns.min(4).max(1));
                let _ = adaptive_ctrl.set_ceiling(adaptive_ceiling);

                loop {
                    sleep(Duration::from_millis(50)).await;
                    tick_count += 1;

                    // 1. التقاط لقطة من الحالة تحت القفل ثم تحريره فوراً: العمال يحتاجون هذا القفل
                    //    لاستلام المقاطع، ولا يجوز إبقاؤه أثناء عمليات القرص البطيئة (fsync).
                    let (snapshot, should_stop) = {
                        let mut m = meta_saver_clone.lock().await;
                        registry_saver_clone.sync_to_meta(&mut m);

                        if throttled_saver.swap(false, Ordering::Relaxed) {
                            if let Some(new_lim) = adaptive_ctrl.on_throttled(Instant::now()) {
                                worker_limit_saver.store(new_lim, Ordering::Relaxed);
                            }
                        }

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

                            if tick_count % 20 == 0 {
                                if let Some(new_lim) = adaptive_ctrl.on_sample(AdaptiveSample {
                                    throughput_bps: current_speed as f64,
                                    errors: 0,
                                    latency: None,
                                    speed_limited: m.task_speed_limit_kbps > 0,
                                }) {
                                    worker_limit_saver.store(new_lim, Ordering::Relaxed);
                                }
                            }
                        }

                        let snapshot = if tick_count % 40 == 0 {
                            Some(m.clone())
                        } else {
                            None
                        };
                        let should_stop = pause_saver_clone.load(Ordering::Relaxed)
                            || m.status == DownloadStatus::Completed
                            || m.status == DownloadStatus::Failed
                            || m.status == DownloadStatus::Paused
                            || m.status == DownloadStatus::NeedsUrlRefresh;
                        (snapshot, should_stop)
                    };

                    // 2. الحفظ الدوري كل ~2 ثانية خارج القفل. الترتيب مهم لسلامة الاستئناف بعد
                    //    انقطاع الكهرباء: نزامن بيانات ملف `.part` على القرص أولاً، ثم نحفظ اللقطة
                    //    (المأخوذة قبل المزامنة)، فلا يدّعي سجل التقدم بايتات غير مثبّتة فعلياً.
                    if let Some(snapshot) = snapshot {
                        let part_file = Arc::clone(&part_file_saver_clone);
                        let synced = tokio::task::spawn_blocking(move || part_file.sync_data())
                            .await
                            .map(|r| r.is_ok())
                            .unwrap_or(false);
                        if synced {
                            let _ = save_meta_to_disk_atomic(&snapshot, &meta_path_clone).await;
                        }
                    }

                    if should_stop {
                        break;
                    }
                }
            });

            let worker_count = if resumable && total_size >= self.min_split_size * 2 {
                adaptive_ceiling.max(1)
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
                let shared_file_clone = Arc::clone(&shared_part_file);
                let min_split = self.min_split_size;
                let worker_limit_arc = Arc::clone(&active_worker_limit);
                let throttled_arc = Arc::clone(&throttled_signal);

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
                        shared_file_clone,
                        min_split,
                        worker_limit_arc,
                        throttled_arc,
                    )
                    .await
                });
                handles.push(handle);
            }

            for h in handles {
                let _ = h.await;
            }
            saver_handle.abort();

            let _ = shared_part_file.sync_all();
            drop(shared_part_file);

            {
                let mut m = self.meta.lock().await;
                atomic_registry.sync_to_meta(&mut m);
            }

            // 1. فحص ما إذا تغير المورد على الخادم أثناء الاستئناف (If-Range أعاد 200 OK أو تغير الحجم)
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
                if m.last_error.is_none() {
                    m.last_error = Some("تعذر إكمال بعض المقاطع بعد استنفاد المحاولات".to_string());
                }
                save_meta_to_disk_atomic(&m, &meta_path).await?;
                return Err(anyhow!("تعذر إكمال بعض المقاطع، الحالة محفوظة للاستئناف"));
            }

            if m.total_size > 0 {
                let audit = SegmentIntegrityAuditor::audit_segments(&m.segments, m.total_size);
                if !audit.is_contiguous || m.total_downloaded() != m.total_size {
                    m.status = DownloadStatus::Failed;
                    m.last_error = Some(format!(
                        "فشل فحص تكامل المقاطع النهائية: المحمل ({}) مقابل المتوقع ({})",
                        m.total_downloaded(),
                        m.total_size
                    ));
                    save_meta_to_disk_atomic(&m, &meta_path).await?;
                    return Err(anyhow!("فشل فحص تكامل حدود المقاطع"));
                }
            }

            // حفظ بصمات القطاعات المكتملة قبل الدمج النهائي ثم تنظيف ملف البصمات المؤقت للقطاعات
            {
                let mut v_manifest = PartVerifyManifest::load(&part_path);
                let _ = hash_completed_segments_in_manifest(&part_path, &m.segments, &mut v_manifest);
                PartVerifyManifest::remove(&part_path);
            }

            m.status = DownloadStatus::Assembling;
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
                    return Err(anyhow!(
                        "فشل التحقق من بصمة SHA-256: المتوقع {} والفعلي {}",
                        expected_hash,
                        actual_sha256
                    ));
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

            // ميزة VeloxFetch #3: الاكتشاف التلقائي لبصمة الناشر (SHA256SUMS / SHA512SUMS / <file>.sha256)
            // إذا لم يدخل المستخدم بصمة يدوية وكان الرابط عبر HTTPS
            if m.expected_sha256.as_deref().map(str::trim).unwrap_or("").is_empty()
                && m.url.starts_with("https://")
            {
                if let Some(pub_chk) = discover_publisher_checksum(&self.client, &m.url, false).await
                {
                    if let Err(pub_err) =
                        verify_file_against_publisher_checksum(&final_path, &pub_chk).await
                    {
                        m.status = DownloadStatus::Failed;
                        let err_msg = pub_err.to_string();
                        m.last_error = Some(err_msg.clone());
                        m.push_log(None, "ERROR", err_msg);
                        save_meta_to_disk_atomic(&m, &meta_path).await?;
                        return Err(pub_err);
                    }
                    m.push_log(
                        None,
                        "INFO",
                        format!(
                            "تم التحقق التلقائي من بصمة الناشر بنجاح عبر قائمة '{}'",
                            pub_chk.source_list
                        ),
                    );
                }
            }

            // وسم الملف بأنه قادم من الإنترنت (:Zone.Identifier) وتطبيق تاريخ التعديل الأصلي من الخادم (Last-Modified)
            mark_file_from_internet(&final_path, &m.url, m.source_page_url.as_deref());
            apply_server_last_modified_time(&final_path, m.last_modified.as_deref());

            if let Some(ref av_cmd) = self.antivirus_cmd {
                if validate_safe_executable_path(av_cmd).is_ok() {
                    run_virus_scanner_on_file(av_cmd, "[File]", &final_path);
                }
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
        worker_idx: usize,
        meta: Arc<Mutex<DownloadStateMeta>>,
        atomic_registry: Arc<AtomicSegmentRegistry>,
        pause_flag: Arc<AtomicBool>,
        cancel_flag: Arc<AtomicBool>,
        resource_mutated_flag: Arc<AtomicBool>,
        expired_url_flag: Arc<AtomicBool>,
        speed_limiter: Arc<SpeedLimiter>,
        client: Client,
        shared_file: Arc<StdFile>,
        min_split_size: u64,
        active_worker_limit: Arc<AtomicUsize>,
        throttled_signal: Arc<AtomicBool>,
    ) -> Result<()> {
        loop {
            if pause_flag.load(Ordering::Relaxed)
                || cancel_flag.load(Ordering::Relaxed)
                || resource_mutated_flag.load(Ordering::Relaxed)
                || expired_url_flag.load(Ordering::Relaxed)
            {
                break;
            }

            let dynamic_max_conns = active_worker_limit.load(Ordering::Relaxed).max(1);
            if worker_idx >= dynamic_max_conns {
                let still_has_splittable_work = {
                    let mut m = meta.lock().await;
                    atomic_registry.sync_to_meta(&mut m);
                    m.segments.iter().any(|s| {
                        s.status == SegmentStatus::Pending
                            || (s.status == SegmentStatus::Downloading
                                && s.remaining_bytes() >= min_split_size * 2)
                    })
                };
                if !still_has_splittable_work {
                    break;
                }
                sleep(Duration::from_millis(150)).await;
                continue;
            }

            let assigned_seg_id = {
                let mut m = meta.lock().await;
                atomic_registry.sync_to_meta(&mut m);
                let active_count = m
                    .segments
                    .iter()
                    .filter(|s| s.status == SegmentStatus::Downloading)
                    .count();
                if active_count >= dynamic_max_conns {
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

            let mut retries = 0u32;
            loop {
                if pause_flag.load(Ordering::Relaxed)
                    || cancel_flag.load(Ordering::Relaxed)
                    || resource_mutated_flag.load(Ordering::Relaxed)
                    || expired_url_flag.load(Ordering::Relaxed)
                {
                    break;
                }

                let offset_before = atomic_registry
                    .get(seg_id)
                    .map(|a| a.current_offset.load(Ordering::Acquire))
                    .unwrap_or(0);

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
                    &shared_file,
                    &throttled_signal,
                )
                .await;

                let offset_after = atomic_registry
                    .get(seg_id)
                    .map(|a| a.current_offset.load(Ordering::Acquire))
                    .unwrap_or(offset_before);
                let bytes_advanced = offset_after.saturating_sub(offset_before);

                match res {
                    Ok(()) => {
                        let is_seg_done = atomic_registry
                            .get(seg_id)
                            .map(|a| {
                                a.status_code.load(Ordering::Acquire)
                                    == SegmentStatus::Completed.to_u8()
                                    || a.current_offset.load(Ordering::Acquire)
                                        > a.end_byte.load(Ordering::Acquire)
                            })
                            .unwrap_or(true);
                        if is_seg_done {
                            break;
                        }

                        // حماية ضد الحلقات اللانهائية: إذا لم يكتمل المقطع ولم يتقدم المؤشر، يعامل كفشل شبكي مع تأخير أسي
                        if bytes_advanced > 0 {
                            retries = 0;
                        } else {
                            retries += 1;
                            if retries > 4 {
                                let mut m = meta.lock().await;
                                atomic_registry.sync_to_meta(&mut m);
                                m.last_error = Some(
                                    "توقف الخادم عن إرسال بيانات للمقطع (Zero-progress stream)"
                                        .to_string(),
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
                            let backoff_ms = 300 * (1u64 << retries.min(5));
                            sleep(Duration::from_millis(backoff_ms)).await;
                        }
                    }
                    Err(e) => {
                        let err_msg = e.to_string();
                        if err_msg.contains("RESOURCE_MUTATED")
                            || err_msg.contains("URL_EXPIRED")
                            || err_msg.contains("RANGE_IGNORED")
                        {
                            break;
                        }

                        // لا نصفّر عداد المحاولات إلا إذا أحرز المقطع تقدماً فعلياً معتبراً (>= 64KB)
                        if bytes_advanced >= MIN_PROGRESS_TO_RESET_RETRIES {
                            retries = 0;
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
                        // إذا كان الخطأ انقطاعاً مؤقتاً في الشبكة، ننتظر إما انتهاء مهلة التراجع الأسي أو إشارة عودة الشبكة الفورية من أي مقطع زميل!
                        let backoff_ms = 300 * (1u64 << retries.min(5));
                        tokio::select! {
                            _ = sleep(Duration::from_millis(backoff_ms)) => {},
                            _ = global_back_online_notify().notified() => {},
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// قاعدة القسمة النصفية الذرية (In-Half Division Rule) المتوافقة مع الاختبارات المباشرة
    #[allow(dead_code)]
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
        shared_file: &Arc<StdFile>,
        throttled_signal: &Arc<AtomicBool>,
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
            task_speed_limit_kbps,
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
                m.task_speed_limit_kbps,
            )
        };
        let task_limiter = (task_speed_limit_kbps > 0).then(|| SpeedLimiter::new(task_speed_limit_kbps));

        let req_window_end = match max_request_for(&url) {
            Some(window_limit) if resumable && initial_end < u64::MAX => {
                initial_end.min(start_offset.saturating_add(window_limit.max(1) - 1))
            }
            _ => initial_end,
        };

        // ميزة VeloxFetch #7: اختيار مسار الشبكة الأقل عبئاً إذا كان دمج الشبكات المتعددة (Multi-Network Lanes) مفعلاً
        let selected_lane = global_network_lane_pool().pick_lane_for_url(&url);
        if let Some(ref lane) = selected_lane {
            lane.begin_connection();
        }
        struct LaneConnectionGuard(Option<Arc<crate::core::lanes::NetworkLane>>);
        impl Drop for LaneConnectionGuard {
            fn drop(&mut self) {
                if let Some(ref l) = self.0 {
                    l.end_connection();
                }
            }
        }
        let _lane_guard = LaneConnectionGuard(selected_lane.clone());
        let effective_client = selected_lane.as_ref().map(|l| &l.client).unwrap_or(client);
        let mut req = effective_client.get(&url);
        for (k, v) in &custom_headers {
            if let (Ok(hn), Ok(hv)) = (HeaderName::from_str(k), HeaderValue::from_str(v)) {
                req = req.header(hn, hv);
            }
        }

        if resumable && req_window_end < u64::MAX {
            req = req.header(RANGE, format!("bytes={}-{}", start_offset, req_window_end));
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

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
        {
            throttled_signal.store(true, Ordering::Relaxed);
            if let Some(ref lane) = selected_lane {
                if let Ok(parsed) = url::Url::parse(&url) {
                    if let Some(h) = parsed.host_str() {
                        lane.record_failure();
                        lane.record_host_refusal(h);
                    }
                }
            }
        }

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

        // التحقق الصارم من ترويسة Content-Range في جميع استجابات 206 Partial Content
        if resumable && req_window_end < u64::MAX {
            if let Err(cr_err) = validate_partial_content_range(
                status,
                resp.headers(),
                start_offset,
                expected_total,
            ) {
                if cr_err.to_string().contains("RESOURCE_MUTATED") {
                    resource_mutated_flag.store(true, Ordering::SeqCst);
                }
                return Err(cr_err);
            }
        }

        let mut stream = resp.bytes_stream();
        let mut current_pos = start_offset;
        let seg_start_instant = Instant::now();
        let mut seg_session_bytes = 0u64;
        // إصلاح الاختناق #5: تجميع شرائح الشبكة الصغيرة في كتل متصلة بحجم 256KB لتقليل استدعاءات الكتابة العشوائية على القرص
        let mut write_coalescer = SegmentWriteCoalescer::new(shared_file, start_offset);

        while let Some(chunk_res) = timeout(Duration::from_secs(20), stream.next())
            .await
            .context("انتهت مهلة قراءة البيانات من الشبكة")?
        {
            if pause_flag.load(Ordering::Relaxed)
                || cancel_flag.load(Ordering::Relaxed)
                || resource_mutated_flag.load(Ordering::Relaxed)
                || expired_url_flag.load(Ordering::Relaxed)
            {
                write_coalescer.flush()?;
                let mut m = meta.lock().await;
                atomic_registry.sync_to_meta(&mut m);
                return Ok(());
            }

            let chunk = chunk_res?;
            speed_limiter.acquire(chunk.len()).await;
            if let Some(ref tl) = task_limiter {
                tl.acquire(chunk.len()).await;
            }

            let seg_end = seg_atomic.end_byte.load(Ordering::Acquire);
            let effective_stop = seg_end.min(req_window_end);

            if current_pos > effective_stop {
                break;
            }

            let max_writable = (effective_stop - current_pos + 1) as usize;
            let write_len = chunk.len().min(max_writable);

            if write_len > 0 {
                if seg_session_bytes == 0 {
                    // بمجرد استلام أول دفعة بايتات بنجاح، نوقظ أي مقاطع زميلة تنتظر عودة الشبكة فوراً!
                    global_back_online_notify().notify_waiters();
                }
                write_coalescer.push(&chunk[..write_len], current_pos)?;
                if let Some(ref lane) = selected_lane {
                    lane.record_bytes(write_len);
                }
                current_pos += write_len as u64;
                seg_session_bytes += write_len as u64;
            }

            let elapsed_secs = seg_start_instant.elapsed().as_secs_f64().max(0.001);
            let current_bps = (seg_session_bytes as f64 / elapsed_secs) as u64;

            seg_atomic
                .current_offset
                .store(current_pos, Ordering::Release);
            seg_atomic
                .last_speed_bps
                .store(current_bps, Ordering::Relaxed);

            if current_pos > seg_atomic.end_byte.load(Ordering::Acquire) {
                write_coalescer.flush()?;
                seg_atomic
                    .status_code
                    .store(SegmentStatus::Completed.to_u8(), Ordering::Release);
                seg_atomic.last_speed_bps.store(0, Ordering::Relaxed);
                let mut m = meta.lock().await;
                atomic_registry.sync_to_meta(&mut m);
                return Ok(());
            }

            if current_pos > req_window_end || write_len < chunk.len() {
                break;
            }
        }
        write_coalescer.flush()?;

        // فحص صارم عند انتهاء التدفق (Stream EOF):
        // إذا كان التحميل مقسماً وبحجم معلوم، يجب أن يكون current_pos قد تجاوز نهاية المقطع أو نهاية نافذة الطلب (req_window_end).
        // وإلا فإن الخادم أغلق الاتصال مبكراً أو أرسل استجابة فارغة، ويجب إرجاع خطأ صريح لتفعيل سياسة Backoff.
        let final_seg_end = seg_atomic.end_byte.load(Ordering::Acquire);
        let required_stop = final_seg_end.min(req_window_end);

        if resumable && expected_total > 0 && current_pos <= required_stop {
            let mut m = meta.lock().await;
            atomic_registry.sync_to_meta(&mut m);
            if current_pos == start_offset {
                bail!(
                    "EMPTY_RESPONSE_STREAM: أغلق الخادم الاتصال دون إرسال أي بايتات للنطاق {}..={}",
                    start_offset,
                    required_stop
                );
            } else {
                bail!(
                    "STREAM_TRUNCATED: انقطع تدفق البيانات مبكراً عند البايت {} قبل إتمام النطاق المطلوب {}",
                    current_pos,
                    required_stop
                );
            }
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::storage::FileCategory;

    #[test]
    fn rejects_206_without_content_range_or_wrong_start() {
        let empty_headers = HeaderMap::new();
        assert!(
            validate_partial_content_range(
                reqwest::StatusCode::PARTIAL_CONTENT,
                &empty_headers,
                0,
                1000
            )
            .is_err(),
            "يجب رفض استجابة 206 الخالية من Content-Range"
        );

        let mut wrong_start = HeaderMap::new();
        wrong_start.insert("content-range", HeaderValue::from_static("bytes 50-999/1000"));
        assert!(
            validate_partial_content_range(
                reqwest::StatusCode::PARTIAL_CONTENT,
                &wrong_start,
                100,
                1000
            )
            .is_err(),
            "يجب رفض استجابة 206 عندما لا تطابق بداية النطاق الإزاحة المطلوبة"
        );

        let mut mutated_size = HeaderMap::new();
        mutated_size.insert("content-range", HeaderValue::from_static("bytes 100-999/2000"));
        assert!(
            validate_partial_content_range(
                reqwest::StatusCode::PARTIAL_CONTENT,
                &mutated_size,
                100,
                1000
            )
            .unwrap_err()
            .to_string()
            .contains("RESOURCE_MUTATED")
        );
    }

    #[test]
    fn refreshed_url_compatibility_requires_strong_etag_or_last_modified() {
        // غياب كل من ETag و Last-Modified رغم تساوي الحجم -> يجب رفضه لمنع دمج ملفين مختلفين
        assert!(!is_refreshed_resource_compatible(
            10_000, 10_000, true, None, None, None, None
        ));

        // وجود ETag قوي متطابق -> مقبول
        assert!(is_refreshed_resource_compatible(
            10_000,
            10_000,
            true,
            Some("\"abc-123\""),
            Some("\"abc-123\""),
            None,
            None
        ));

        // ETag ضعيف فقط بدون Last-Modified -> مرفوض
        assert!(!is_refreshed_resource_compatible(
            10_000,
            10_000,
            true,
            Some("W/\"weak-1\""),
            Some("W/\"weak-1\""),
            None,
            None
        ));

        // غياب ETag مع تطابق Last-Modified -> مقبول
        assert!(is_refreshed_resource_compatible(
            10_000,
            10_000,
            true,
            None,
            None,
            Some("Sat, 24 Oct 2026 08:00:00 GMT"),
            Some("Sat, 24 Oct 2026 08:00:00 GMT")
        ));
    }

    #[test]
    fn part_file_mismatch_resets_corrupt_resume_state() {
        let mut meta = DownloadStateMeta {
            task_id: "t_corrupt_part".to_string(),
            url: "https://example.com/file.bin".to_string(),
            source_page_url: None,
            filename: "file.bin".to_string(),
            save_dir: PathBuf::from("."),
            category: FileCategory::Programs,
            total_size: 10_000,
            resumable: true,
            etag: None,
            last_modified: None,
            custom_headers: HashMap::new(),
            expected_sha256: None,
            computed_sha256: None,
            status: DownloadStatus::Paused,
            last_error: None,
            refresh_attempts: 0,
            segments: vec![FileSegment {
                id: 0,
                start_byte: 0,
                current_offset: 5_000,
                end_byte: 9_999,
                status: SegmentStatus::Paused,
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

        // إذا كان حجم ملف .part الفعلي 0 بايت بينما السجل يدعي الوصول إلى 5000، يتم تصفير المقاطع فوراً
        assert!(!validate_part_file_against_segments(&mut meta, 0));
        assert!(meta.segments.is_empty());
    }
}
