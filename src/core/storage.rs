//! نماذج الحالة، الحفظ الذري المتين، استخراج أسماء الملفات RFC 5987، وحساب البصمات الرقمية (SHA-256 / MD5 / CRC32)

use anyhow::Result;
use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, CONTENT_DISPOSITION};
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

/// الحد الأدنى لحجم المقطع القابل للقسمة إلى النصف (512 كيلوبايت)
pub const MIN_SPLIT_SIZE: u64 = 512 * 1024;
/// الحد الأقصى لحجم رسالة Chrome Native Messaging (1 ميجابايت لمنع استنزاف الذاكرة)
pub const MAX_NATIVE_MSG_BYTES: usize = 1024 * 1024;

/// الدليل الافتراضي الموحد لجميع تنزيلات وحالات RustIDM عبر النظام بالكامل
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

    /// نسخة محمية خالية من الكوكيز والترويسات السرية للعرض الآمن في الواجهة والسجلات
    pub fn redacted_clone(&self) -> Self {
        let mut copy = self.clone();
        copy.custom_headers = redact_sensitive_headers(&self.custom_headers);
        copy
    }

    /// إضافة سجل اتصال تشخيصي مع الحفاظ على سقف ثابت للذاكرة (أحدث 150 سجل)
    pub fn push_log(&mut self, segment_id: Option<usize>, level: &str, message: impl Into<String>) {
        if self.connection_logs.len() >= 150 {
            self.connection_logs.remove(0);
        }
        self.connection_logs.push(ConnectionLogEntry {
            timestamp: Utc::now(),
            segment_id,
            level: level.to_string(),
            message: message.into(),
        });
    }

    /// تسجيل قراءة سرعة لحظية لمنحنى السرعة الحي (أحدث 120 نقطة)
    pub fn push_speed_sample(&mut self, speed_bps: u64, active_connections: usize) {
        if self.speed_history.len() >= 120 {
            self.speed_history.remove(0);
        }
        self.speed_history.push(SpeedSamplePoint {
            timestamp_ms: Utc::now().timestamp_millis(),
            speed_bps,
            active_connections,
        });
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
// 3. دوال الحفظ الذري ومزامنة نظام الملفات والبصمات الرقمية (Durable Atomic Storage)
// ============================================================================

/// فك ترميز النسبة المئوية (Percent-Decoding RFC 3986) لأسماء الملفات المستخرجة من الروابط أو Content-Disposition
pub fn percent_decode_str(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex_str) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(val) = u8::from_str_radix(hex_str, 16) {
                    decoded.push(val);
                    i += 3;
                    continue;
                }
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&decoded).to_string()
}

/// استخراج اسم الملف بدقة مع دعم RFC 5987 / RFC 6266 (`filename*=UTF-8''...`) وفك ترميز `%20`
pub fn extract_filename(url_str: &str, headers: &HeaderMap) -> String {
    if let Some(cd) = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
    {
        for part in cd.split(';') {
            let trimmed = part.trim();
            let lower = trimmed.to_lowercase();
            if lower.starts_with("filename*=") {
                let raw_val =
                    &trimmed["filename*=".len()..].trim_matches(|c| c == '"' || c == '\'');
                let encoded_name = if let Some(idx) = raw_val.find("''") {
                    &raw_val[idx + 2..]
                } else {
                    raw_val
                };
                let decoded = percent_decode_str(encoded_name);
                if !decoded.trim().is_empty() {
                    return sanitize_filename(decoded.trim());
                }
            }
        }

        for part in cd.split(';') {
            let trimmed = part.trim();
            let lower = trimmed.to_lowercase();
            if lower.starts_with("filename=") {
                let raw_val = trimmed["filename=".len()..]
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'');
                let decoded = percent_decode_str(raw_val);
                if !decoded.trim().is_empty() {
                    return sanitize_filename(decoded.trim());
                }
            }
        }
    }

    if let Ok(parsed) = Url::parse(url_str) {
        if let Some(seg) = parsed.path_segments().and_then(|s| s.last()) {
            let decoded = percent_decode_str(seg);
            if !decoded.trim().is_empty() {
                return sanitize_filename(decoded.trim());
            }
        }
    }

    format!("download_{}.bin", Utc::now().timestamp())
}

pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
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
