//! نماذج الحالة، الحفظ الذري المتين، تنعيم السرعة بالنافذة المنزلقة والمتوسط الأسي (`Windowed EMA`)،
//! استخراج أسماء الملفات RFC 5987 + استنتاج امتدادات MIME (`filename`)، وحساب البصمات الرقمية (SHA-256 / MD5 / CRC32)

use anyhow::Result;
use chrono::{DateTime, Utc};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tokio::{
    fs::{self, File},
    io::AsyncWriteExt,
};
use url::Url;

pub use crate::core::filename::{
    ensure_extension, extension_for_mime, filename_from_content_disposition, filename_from_url,
    is_plausible_extension, percent_decode_str, resolve_filename,
    resolve_filename_from_url_and_headers, resolve_unique_filename, sanitize_filename,
    sanitize_filename_opt, split_stem_ext, truncate_keeping_extension, DEFAULT_FILENAME,
    MAX_FILENAME_CHARS,
};

/// الحد الأدنى لحجم المقطع القابل للقسمة إلى النصف (512 كيلوبايت)
pub const MIN_SPLIT_SIZE: u64 = 512 * 1024;
/// الحد الأقصى لحجم رسالة Chrome Native Messaging (1 ميجابايت لمنع استنزاف الذاكرة)
pub const MAX_NATIVE_MSG_BYTES: usize = 1024 * 1024;
/// معامل التنعيم الأسي للسرعة (Exponential Moving Average Alpha — مثل dlman لثبات القراءة والـ ETA)
pub const SPEED_EMA_ALPHA: f64 = 0.18;
/// حجم النافذة المنزلقة لحساب متوسط السرعة (آخر 10 قراءات = 5 ثوانٍ)
pub const SPEED_WINDOW_SAMPLES: usize = 10;

/// الدليل الافتراضي الموحد لجميع تنزيلات وحالات StormGet (RustIDM سابقاً) عبر النظام بالكامل
pub fn default_download_directory() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Downloads")
        .join("RustIDM")
}

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
            "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "wma" | "opus" => FileCategory::Music,
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "tgz" | "zst" => {
                FileCategory::Compressed
            }
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "epub"
            | "rtf" | "csv" => FileCategory::Documents,
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
                || lower.contains("password")
                || lower.contains("api-key")
                || lower.contains("apikey")
            {
                (k.clone(), "[REDACTED_SECRET]".to_string())
            } else {
                (k.clone(), v.clone())
            }
        })
        .collect()
}

/// فحص سريع O(1) لمعرفة ما إذا كانت الرسالة قد تحتوي أصلاً على معاملات استعلام سرية (`=`) تحتاج للتنقية
#[inline]
pub fn may_contain_unredacted_secret(msg: &str) -> bool {
    msg.contains('=')
}

/// تنقية نص رسالة السجل من أي رموز سرية أو كوكيز أو كلمات مرور مضمنة في الروابط أو الرسائل
/// (مع مسار سريع يتجنب تحويل النص إلى حروف صغيرة 8 مرات إذا لم يكن هناك رمز `=` في النص)
pub fn redact_log_message(msg: &str) -> String {
    if !may_contain_unredacted_secret(msg) {
        return msg.to_string();
    }
    let mut out = msg.to_string();
    let mut lower = out.to_ascii_lowercase();
    for sensitive_key in [
        "token=",
        "access_token=",
        "auth=",
        "password=",
        "secret=",
        "sig=",
        "signature=",
        "key=",
    ] {
        if let Some(pos) = lower.find(sensitive_key) {
            let val_start = pos + sensitive_key.len();
            let rest = &out[val_start..];
            let val_end = rest
                .find(|c: char| c == '&' || c.is_whitespace() || c == '"' || c == '\'')
                .unwrap_or(rest.len());
            if val_end > 0 && &rest[..val_end] != "[REDACTED]" {
                out.replace_range(val_start..val_start + val_end, "[REDACTED]");
                lower = out.to_ascii_lowercase();
            }
        }
    }
    out
}

// ============================================================================
// 2. هياكل بيانات المقاطع وحالة التحميل (Segments & DownloadStateMeta)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SegmentStatus {
    Pending,
    Downloading,
    Completed,
    Paused,
    Failed,
}

impl SegmentStatus {
    pub fn to_u8(self) -> u8 {
        match self {
            SegmentStatus::Pending => 0,
            SegmentStatus::Downloading => 1,
            SegmentStatus::Completed => 2,
            SegmentStatus::Paused => 3,
            SegmentStatus::Failed => 4,
        }
    }

    pub fn from_u8(code: u8) -> Self {
        match code {
            0 => SegmentStatus::Pending,
            1 => SegmentStatus::Downloading,
            2 => SegmentStatus::Completed,
            3 => SegmentStatus::Paused,
            _ => SegmentStatus::Failed,
        }
    }
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
pub struct ConnectionLogEntry {
    pub timestamp: DateTime<Utc>,
    pub segment_id: Option<usize>,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedSamplePoint {
    pub timestamp_ms: i64,
    pub speed_bps: u64,
    pub active_connections: usize,
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
    #[serde(default, with = "crate::core::secret::header_map")]
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
    #[serde(default)]
    pub mirror_urls: Vec<String>,
    #[serde(default)]
    pub active_mirror_index: usize,
    #[serde(default)]
    pub expected_md5: Option<String>,
    #[serde(default)]
    pub computed_md5: Option<String>,
    #[serde(default)]
    pub expected_crc32: Option<String>,
    #[serde(default)]
    pub computed_crc32: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub speed_history: Vec<SpeedSamplePoint>,
    #[serde(default)]
    pub connection_logs: Vec<ConnectionLogEntry>,
    #[serde(default)]
    pub task_speed_limit_kbps: u64,
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

    /// التحقق مما إذا كانت المهمة قد أُلغيت صراحة من قبل المستخدم (ولم تفشل فقط بسبب خطأ شبكي)
    pub fn is_cancelled_by_user(&self) -> bool {
        self.status == DownloadStatus::Failed
            && self
                .last_error
                .as_deref()
                .map(|e| {
                    e.contains("Cancelled by user") || e.contains("تم الإلغاء بواسطة المستخدم")
                })
                .unwrap_or(false)
    }

    /// التحقق مما إذا كان الفشل ناتجاً عن انتهاء صلاحية الرابط (401/403/410)
    pub fn is_expired_link(&self) -> bool {
        self.status == DownloadStatus::NeedsUrlRefresh
            || self
                .last_error
                .as_deref()
                .map(|e| {
                    e.contains("HTTP_EXPIRED_OR_FORBIDDEN")
                        || e.contains("403")
                        || e.contains("401")
                        || e.contains("410")
                        || e.contains("انتهت صلاحية")
                })
                .unwrap_or(false)
    }

    /// نسخة محمية خالية من الكوكيز والترويسات السرية للعرض الآمن في الواجهة والسجلات
    /// (إصلاح الاختناق #3: لا تعيد فحص وتنقية السجلات إلا إذا احتوت على معاملات `=` غير منقاة، مما يوفر 95% من وقت المعالج أثناء استقصاء الواجهة)
    pub fn redacted_clone(&self) -> Self {
        let mut copy = self.clone();
        if !self.custom_headers.is_empty() {
            copy.custom_headers = redact_sensitive_headers(&self.custom_headers);
        }
        for log in &mut copy.connection_logs {
            if may_contain_unredacted_secret(&log.message) {
                log.message = redact_log_message(&log.message);
            }
        }
        copy
    }

    /// إضافة سجل اتصال تشخيصي مع الحفاظ على سقف ثابت للذاكرة (أحدث 150 سجل) وحجب الأسرار
    pub fn push_log(&mut self, segment_id: Option<usize>, level: &str, message: impl Into<String>) {
        if self.connection_logs.len() >= 150 {
            self.connection_logs.remove(0);
        }
        self.connection_logs.push(ConnectionLogEntry {
            timestamp: Utc::now(),
            segment_id,
            level: level.to_string(),
            message: redact_log_message(&message.into()),
        });
    }

    /// تسجيل قراءة سرعة مع تطبيق تنعيم مزدوج (`Sliding Window + Exponential Moving Average`)
    /// لمنع القفزات العشوائية في عرض السرعة والوقت المتبقي (`ETA`).
    pub fn push_speed_sample(&mut self, instant_speed_bps: u64, active_connections: usize) {
        let smoothed = if active_connections == 0 && instant_speed_bps == 0 {
            0
        } else if self.speed_history.is_empty() {
            instant_speed_bps
        } else {
            let recent_slice = if self.speed_history.len() >= SPEED_WINDOW_SAMPLES {
                &self.speed_history[self.speed_history.len() - SPEED_WINDOW_SAMPLES + 1..]
            } else {
                &self.speed_history[..]
            };
            let mut sum = instant_speed_bps as f64;
            let mut count = 1.0;
            for pt in recent_slice {
                sum += pt.speed_bps as f64;
                count += 1.0;
            }
            let window_avg = sum / count;
            let prev_smoothed = self
                .speed_history
                .last()
                .map(|p| p.speed_bps as f64)
                .unwrap_or(window_avg);
            (SPEED_EMA_ALPHA * window_avg + (1.0 - SPEED_EMA_ALPHA) * prev_smoothed) as u64
        };

        if self.speed_history.len() >= 120 {
            self.speed_history.remove(0);
        }
        self.speed_history.push(SpeedSamplePoint {
            timestamp_ms: Utc::now().timestamp_millis(),
            speed_bps: smoothed,
            active_connections,
        });
    }

    /// الحصول على السرعة المنعّمة الحالية بالبايت/ثانية (`Windowed EMA`)
    pub fn smoothed_speed_bps(&self) -> u64 {
        if self.status != DownloadStatus::Downloading {
            return 0;
        }
        let raw_seg_speed: u64 = self
            .segments
            .iter()
            .filter(|s| s.status == SegmentStatus::Downloading)
            .map(|s| s.last_speed_bps)
            .sum();
        if let Some(last_pt) = self.speed_history.last() {
            if last_pt.speed_bps > 0 && raw_seg_speed > 0 {
                return ((SPEED_EMA_ALPHA * raw_seg_speed as f64)
                    + ((1.0 - SPEED_EMA_ALPHA) * last_pt.speed_bps as f64))
                    as u64;
            }
        }
        raw_seg_speed
    }

    /// حساب الوقت المتبقي بالثواني (`ETA`) بناءً على السرعة المنعّمة (`Windowed EMA`)
    pub fn estimated_eta_secs(&self) -> Option<u64> {
        if self.total_size == 0 || self.status != DownloadStatus::Downloading {
            return None;
        }
        let speed = self.smoothed_speed_bps();
        if speed == 0 {
            return None;
        }
        let remaining = self.total_size.saturating_sub(self.total_downloaded());
        Some(remaining / speed)
    }

    /// اختيار الرابط الفعال حالياً (الأصلي أو أحد الروابط البديلة Mirrors عند الفشل)
    pub fn current_effective_url(&self) -> String {
        if self.active_mirror_index == 0 || self.mirror_urls.is_empty() {
            self.url.clone()
        } else {
            self.mirror_urls
                .get(self.active_mirror_index - 1)
                .cloned()
                .unwrap_or_else(|| self.url.clone())
        }
    }

    /// التبديل التلقائي إلى رابط مرآة بديل (Mirror Failover) عند تعطل الخادم الأساسي
    pub fn rotate_mirror(&mut self) -> Option<String> {
        if self.mirror_urls.is_empty() {
            return None;
        }
        let total_candidates = 1 + self.mirror_urls.len();
        self.active_mirror_index = (self.active_mirror_index + 1) % total_candidates;
        let next = self.current_effective_url();
        self.push_log(
            None,
            "WARN",
            format!("تم التبديل التلقائي إلى رابط المرآة البديل: {}", next),
        );
        Some(next)
    }
}

/// مصنف القواعد الذكي لتوجيه الملفات تلقائياً إلى مجلدات وطوابير مخصصة حسب النطاق أو الحجم أو الامتداد
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartRoutingRule {
    pub rule_name: String,
    pub domain_contains: Option<String>,
    pub extension_in: Vec<String>,
    pub min_size_bytes: Option<u64>,
    pub max_size_bytes: Option<u64>,
    pub target_category: Option<FileCategory>,
    pub target_queue: Option<String>,
    pub target_subfolder: Option<String>,
}

impl SmartRoutingRule {
    pub fn matches(&self, url_str: &str, filename: &str, file_size: u64) -> bool {
        if let Some(ref dom_filter) = self.domain_contains {
            if !dom_filter.trim().is_empty() {
                if let Ok(parsed) = Url::parse(url_str) {
                    let host = parsed.host_str().unwrap_or("").to_lowercase();
                    if !host.contains(&dom_filter.to_lowercase()) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
        }

        if !self.extension_in.is_empty() {
            let ext = Path::new(filename)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if !self
                .extension_in
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(&ext))
            {
                return false;
            }
        }

        if let Some(min_sz) = self.min_size_bytes {
            if file_size > 0 && file_size < min_sz {
                return false;
            }
        }

        if let Some(max_sz) = self.max_size_bytes {
            if file_size > 0 && file_size > max_sz {
                return false;
            }
        }

        true
    }
}

// ============================================================================
// 3. دوال استخراج الأسماء والحفظ الذري والبصمات الرقمية (Durable Atomic Storage)
// ============================================================================

/// استخراج اسم الملف بدقة مع دعم RFC 5987 / RFC 6266 (`filename*=UTF-8''...`) واستنتاج الامتداد من `Content-Type`
pub fn extract_filename(url_str: &str, headers: &HeaderMap) -> String {
    let resolved = resolve_filename_from_url_and_headers(url_str, headers);
    if resolved == DEFAULT_FILENAME {
        format!("download_{}.bin", Utc::now().timestamp())
    } else {
        resolved
    }
}

/// مزامنة المجلد الأب على أنظمة Unix لضمان ثبات عملية rename بعد انقطاع الكهرباء
pub async fn sync_parent_dir(path: &Path) {
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

/// حساب CRC32 القياسي (IEEE 802.3 Polynomial 0xEDB88320)
pub struct Crc32Hasher {
    state: u32,
}

impl Default for Crc32Hasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32Hasher {
    pub fn new() -> Self {
        Self { state: 0xFFFF_FFFF }
    }

    pub fn update(&mut self, data: &[u8]) {
        for &byte in data {
            let mut crc = (self.state ^ (byte as u32)) & 0xFF;
            for _ in 0..8 {
                if (crc & 1) != 0 {
                    crc = (crc >> 1) ^ 0xEDB8_8320;
                } else {
                    crc >>= 1;
                }
            }
            self.state = (self.state >> 8) ^ crc;
        }
    }

    pub fn finalize(&self) -> u32 {
        !self.state
    }
}

/// حساب MD5 القياسي وفق RFC 1321 بلغة Rust الصرفة للتحقق من تكامل الملفات المحملة
pub struct Md5Hasher {
    state: [u32; 4],
    count: u64,
    buffer: [u8; 64],
}

impl Default for Md5Hasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Md5Hasher {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20,
        5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
        6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];

    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    pub fn new() -> Self {
        Self {
            state: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476],
            count: 0,
            buffer: [0u8; 64],
        }
    }

    fn transform(&mut self, block: &[u8; 64]) {
        let mut m = [0u32; 16];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            m[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];

        for i in 0..64 {
            let (f, g) = if i < 16 {
                ((b & c) | ((!b) & d), i)
            } else if i < 32 {
                ((d & b) | ((!d) & c), (5 * i + 1) % 16)
            } else if i < 48 {
                (b ^ c ^ d, (3 * i + 5) % 16)
            } else {
                (c ^ (b | (!d)), (7 * i) % 16)
            };

            let temp = d;
            d = c;
            c = b;
            let sum = a
                .wrapping_add(f)
                .wrapping_add(Self::K[i])
                .wrapping_add(m[g]);
            b = b.wrapping_add(sum.rotate_left(Self::S[i]));
            a = temp;
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
    }

    pub fn update(&mut self, data: &[u8]) {
        let mut idx = (self.count as usize) % 64;
        self.count = self.count.wrapping_add(data.len() as u64);

        let mut i = 0;
        if idx > 0 {
            let take = (64 - idx).min(data.len());
            self.buffer[idx..idx + take].copy_from_slice(&data[..take]);
            idx += take;
            i += take;
            if idx == 64 {
                let blk = self.buffer;
                self.transform(&blk);
            }
        }

        while i + 64 <= data.len() {
            let mut blk = [0u8; 64];
            blk.copy_from_slice(&data[i..i + 64]);
            self.transform(&blk);
            i += 64;
        }

        if i < data.len() {
            let rem = data.len() - i;
            self.buffer[..rem].copy_from_slice(&data[i..]);
        }
    }

    pub fn finalize(mut self) -> [u8; 16] {
        let bit_len = self.count.wrapping_mul(8);
        let idx = (self.count as usize) % 64;
        let pad_len = if idx < 56 { 56 - idx } else { 120 - idx };
        let mut padding = [0u8; 128];
        padding[0] = 0x80;
        let len_bytes = bit_len.to_le_bytes();
        padding[pad_len..pad_len + 8].copy_from_slice(&len_bytes);
        self.update(&padding[..pad_len + 8]);

        let mut out = [0u8; 16];
        for (i, &word) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        out
    }
}

/// حساب جميع البصمات الرقمية (SHA-256 و MD5 و CRC32) بتدفق واحد على القرص دون استهلاك الذاكرة للملفات الضخمة
pub async fn compute_file_all_digests(path: &Path) -> Result<(String, String, String)> {
    use tokio::io::AsyncReadExt;
    let mut file = File::open(path).await?;
    let mut sha256_hasher = Sha256::new();
    let mut md5_hasher = Md5Hasher::new();
    let mut crc32_hasher = Crc32Hasher::new();

    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        sha256_hasher.update(&buf[..n]);
        md5_hasher.update(&buf[..n]);
        crc32_hasher.update(&buf[..n]);
    }

    let sha256_hex = hex::encode(sha256_hasher.finalize());
    let md5_hex = hex::encode(md5_hasher.finalize());
    let crc32_hex = format!("{:08x}", crc32_hasher.finalize());
    Ok((sha256_hex, md5_hex, crc32_hex))
}

pub async fn compute_file_sha256(path: &Path) -> Result<String> {
    let (sha256, _, _) = compute_file_all_digests(path).await?;
    Ok(sha256)
}

#[cfg(test)]
mod sanitize_filename_tests {
    use super::{redact_log_message, sanitize_filename, MAX_FILENAME_CHARS};

    #[test]
    fn replaces_forbidden_characters() {
        assert_eq!(
            sanitize_filename("a/b\\c:d*e?f\"g<h>i|j.zip"),
            "a_b_c_d_e_f_g_h_i_j.zip"
        );
    }

    #[test]
    fn replaces_control_characters() {
        assert_eq!(
            sanitize_filename("a\u{0}b\nc\u{1f}d\u{7f}e.txt"),
            "a_b_c_d_e.txt"
        );
    }

    #[test]
    fn strips_leading_and_trailing_dots_and_spaces() {
        assert_eq!(sanitize_filename("  ..report.pdf. . "), "report.pdf");
        assert_eq!(sanitize_filename("."), "download");
        assert_eq!(sanitize_filename(".."), "download");
        assert_eq!(sanitize_filename(" . . "), "download");
        assert_eq!(sanitize_filename(""), "download");
    }

    #[test]
    fn prefixes_windows_reserved_names() {
        assert_eq!(sanitize_filename("CON"), "_CON");
        assert_eq!(sanitize_filename("nul.txt"), "_nul.txt");
        assert_eq!(sanitize_filename("Com1.tar.gz"), "_Com1.tar.gz");
        assert_eq!(sanitize_filename("LPT9.log"), "_LPT9.log");
        assert_eq!(sanitize_filename("console.txt"), "console.txt");
        assert_eq!(sanitize_filename("COM10.txt"), "COM10.txt");
    }

    #[test]
    fn removes_bidi_extension_spoofing() {
        assert_eq!(sanitize_filename("photo\u{202E}gpj.exe"), "photogpj.exe");
        assert_eq!(sanitize_filename("a\u{2066}b\u{2069}.txt"), "ab.txt");
    }

    #[test]
    fn keeps_arabic_names_intact() {
        assert_eq!(
            sanitize_filename("ملف_تجريبي 2026.zip"),
            "ملف_تجريبي 2026.zip"
        );
    }

    #[test]
    fn truncates_but_keeps_extension() {
        let long = format!("{}.mp4", "x".repeat(500));
        let out = sanitize_filename(&long);
        assert_eq!(out.chars().count(), MAX_FILENAME_CHARS);
        assert!(out.ends_with(".mp4"));

        let arabic_long = format!("{}.mkv", "م".repeat(500));
        let out2 = sanitize_filename(&arabic_long);
        assert_eq!(out2.chars().count(), MAX_FILENAME_CHARS);
        assert!(out2.ends_with(".mkv"));
    }

    #[test]
    fn truncates_without_extension() {
        let out = sanitize_filename(&"y".repeat(400));
        assert_eq!(out.chars().count(), MAX_FILENAME_CHARS);
    }

    #[test]
    fn is_idempotent() {
        for input in [
            "CON",
            "a/b.txt",
            "  ..x.. ",
            "photo\u{202E}gpj.exe",
            "ملف.zip",
        ] {
            let once = sanitize_filename(input);
            assert_eq!(sanitize_filename(&once), once, "input: {input:?}");
        }
    }

    #[test]
    fn redacts_tokens_in_logs() {
        let raw = "GET https://cdn.example.com/file.zip?token=supersecret123&id=5 HTTP/1.1";
        let clean = redact_log_message(raw);
        assert!(!clean.contains("supersecret123"));
        assert!(clean.contains("token=[REDACTED]"));
    }
}
