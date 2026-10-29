//! الميزة 1 و 2 من `VeloxFetch`:
//! 1. الاستئناف الذكي بالعينات (`Sample-Based Resume Verification` من `crates/downloader-core/src/resume.rs`):
//!    عندما لا يرسل الخادم `ETag` قوياً (يرسل `W/"weak"` أو لا يرسل شيئاً)، بدلاً من إهدار الجيجابايتات المحمّلة
//!    وإعادة التحميل من الصفر، نقارن 4 عينات بحجم `64 KB` من ملف `.part` المحفوظ مع الخادم.
//! 2. بصمة `SHA-256` لكل قطاع مكتمل وختم سلامة `.part` (`Per-Segment SHA-256 & Stamp` من `crates/downloader-core/src/verify.rs`):
//!    حساب وحفظ `sha256:<hex>` لكل قطاع مكتمل مع ختم `<len>:<mtime_ns>`، وعند الاستئناف بعد انهيار أو تغير خارجي
//!    يُعاد فحص القطاعات المكتملة وتفريغ القطاع التالف فقط دون المساس ببقية الملف.

use crate::core::storage::{FileSegment, SegmentStatus};
use anyhow::{anyhow, bail, Result};
use reqwest::{
    header::{HeaderMap, RANGE},
    Client,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

/// حجم كل عينة مقارنة عند الاستئناف بدون ETag قوي (64 كيلوبايت)
pub const SAMPLE_LEN: u64 = 64 * 1024;
/// عدد العينات المقارنة (بداية أول قطاع، نهاية أبعد قطاع، ومنتصف أكبر قطاعين)
pub const RESUME_SAMPLES: usize = 4;
/// حجم مخزن القراءة المؤقت أثناء حساب تجزئة القطاعات (1 ميجابايت)
const HASH_BUF_SIZE: usize = 1 << 20;

/// درجة إثبات تطابق الملف على الخادم مع البيانات الجزئية المحفوظة
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResumeProof {
    /// تطابق تام في `ETag` القوي والحجم: نفس النسخة قطعيّاً
    Proven,
    /// لا يوجد تعارض، وتطابقت `ETag` الضعيفة أو `Last-Modified`: يُستأنف بعد مطابقة العينات
    Likely,
    /// الخادم لا يرسل `ETag` ولا `Last-Modified`: العينات الأربع هي الحكم الفصل
    Unproven,
}

fn strong_etag(e: Option<&str>) -> Option<&str> {
    let t = e?.trim();
    if !t.starts_with("W/") && !t.starts_with("w/") && t.len() > 2 {
        Some(t)
    } else {
        None
    }
}

fn opaque_etag(e: &str) -> &str {
    let t = e.trim();
    t.strip_prefix("W/")
        .or_else(|| t.strip_prefix("w/"))
        .unwrap_or(t)
}

/// مقارنة مُعرّفات نسخة الملف القديمة (`old_*`) مع إجابة الخادم الحالية (`new_*`)
/// مستلهمة من `VeloxFetch/crates/downloader-core/src/resume.rs:validators`
pub fn evaluate_resume_validators(
    old_etag: Option<&str>,
    old_date: Option<&str>,
    new_etag: Option<&str>,
    new_date: Option<&str>,
) -> std::result::Result<ResumeProof, &'static str> {
    if let (Some(a), Some(b)) = (strong_etag(old_etag), strong_etag(new_etag)) {
        return if a == b {
            Ok(ResumeProof::Proven)
        } else {
            Err("تغيرت بصمة ETag القوية للملف على الخادم")
        };
    }
    let etags = old_etag
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .zip(new_etag.map(str::trim).filter(|s| !s.is_empty()));
    if etags.is_some_and(|(a, b)| opaque_etag(a) != opaque_etag(b)) {
        return Err("تغيرت قيمة ETag للملف على الخادم");
    }
    let dates = old_date
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .zip(new_date.map(str::trim).filter(|s| !s.is_empty()));
    match dates {
        Some((a, b)) if a != b => Err("تغير تاريخ Last-Modified للملف على الخادم"),
        Some(_) => Ok(ResumeProof::Likely),
        None if etags.is_some() => Ok(ResumeProof::Likely),
        None => Ok(ResumeProof::Unproven),
    }
}

/// تحديد نوافذ العينات غير المتداخلة `(offset, len)` داخل البايتات المحمّلة فعلياً لمقارنتها مع الخادم
/// مستلهمة من `VeloxFetch/crates/downloader-core/src/resume.rs:sample_windows`
pub fn compute_sample_windows(segments: &[FileSegment], max_samples: usize) -> Vec<(u64, u64)> {
    let done: Vec<(u64, u64)> = segments
        .iter()
        .filter_map(|s| {
            let dl = s.downloaded_bytes();
            (dl > 0).then_some((s.start_byte, dl))
        })
        .collect();
    let mut out: Vec<(u64, u64)> = Vec::new();
    let add = |w: (u64, u64), out: &mut Vec<(u64, u64)>| {
        if out.len() < max_samples
            && w.1 > 0
            && !out
                .iter()
                .any(|&(o, l)| w.0 < o + l && o < w.0 + w.1)
        {
            out.push(w);
        }
    };
    if let Some(&(start, len)) = done.iter().min_by_key(|(s, _)| *s) {
        add((start, len.min(SAMPLE_LEN)), &mut out);
    }
    if let Some(&(start, len)) = done.iter().max_by_key(|(s, l)| s + l) {
        let w = len.min(SAMPLE_LEN);
        add((start + len - w, w), &mut out);
    }
    let mut largest = done;
    largest.sort_by_key(|&(s, l)| (std::cmp::Reverse(l), s));
    for (start, len) in largest {
        let w = len.min(SAMPLE_LEN);
        add((start + (len - w) / 2, w), &mut out);
    }
    out
}

/// التحقق الفعلي من تطابق نوافذ العينات المحفوظة في ملف `.part` مع البايتات التي يرسلها الخادم لنفس الإزاحات
pub async fn verify_saved_samples_against_server(
    client: &Client,
    url: &str,
    custom_headers: &HeaderMap,
    part_path: &Path,
    segments: &[FileSegment],
) -> Result<bool> {
    let windows = compute_sample_windows(segments, RESUME_SAMPLES);
    if windows.is_empty() {
        return Ok(true);
    }
    if !part_path.exists() {
        return Ok(false);
    }

    for (offset, len) in windows {
        let end_inclusive = offset + len - 1;
        let part_clone = part_path.to_path_buf();
        let local_bytes = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
            let mut f = std::fs::File::open(part_clone)?;
            f.seek(SeekFrom::Start(offset))?;
            let mut buf = vec![0u8; len as usize];
            f.read_exact(&mut buf)?;
            Ok(buf)
        })
        .await??;

        let resp = tokio::time::timeout(
            Duration::from_secs(12),
            client
                .get(url)
                .headers(custom_headers.clone())
                .header(RANGE, format!("bytes={offset}-{end_inclusive}"))
                .send(),
        )
        .await
        .map_err(|_| anyhow!("انتهت مهلة طلب عينة التحقق من الخادم"))??;

        if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            return Ok(false);
        }
        let remote_bytes = resp.bytes().await?;
        if remote_bytes.len() < local_bytes.len()
            || remote_bytes[..local_bytes.len()] != local_bytes[..]
        {
            return Ok(false);
        }
    }
    Ok(true)
}

// ============================================================================
// 2. تجزئة القطاعات المكتملة وختم سلامة ملف .part (Per-Segment SHA-256 & Stamp)
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PartVerifyManifest {
    pub stamp: Option<String>,
    pub segment_hashes: HashMap<usize, String>,
}

impl PartVerifyManifest {
    pub fn path_for_part(part_path: &Path) -> PathBuf {
        part_path.with_extension("part.verify.json")
    }

    pub fn load(part_path: &Path) -> Self {
        let p = Self::path_for_part(part_path);
        std::fs::read(&p)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, part_path: &Path) {
        let p = Self::path_for_part(part_path);
        if let Ok(bytes) = serde_json::to_vec(self) {
            let _ = std::fs::write(p, bytes);
        }
    }

    pub fn remove(part_path: &Path) {
        let _ = std::fs::remove_file(Self::path_for_part(part_path));
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReverifyReport {
    pub checked: usize,
    pub invalid: usize,
    pub unhashed: usize,
    pub invalid_bytes: u64,
}

/// حساب ختم ملف `.part` بصيغة `<len>:<mtime_ns>` لاكتشاف أي تعديل أو انهيار غير نظيف
pub fn compute_part_file_stamp(part: &Path) -> Option<String> {
    let m = std::fs::metadata(part).ok()?;
    let t = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!("{}:{}", m.len(), t.as_nanos()))
}

/// حساب بصمة `sha256:<hex>` لنطاق محدد `[start, start + len)` داخل ملف `.part`
pub fn hash_file_range(file: &mut std::fs::File, start: u64, len: u64) -> std::io::Result<String> {
    file.seek(SeekFrom::Start(start))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; HASH_BUF_SIZE.min(len.max(1) as usize)];
    let mut left = len;
    while left > 0 {
        let n = (left as usize).min(buf.len());
        file.read_exact(&mut buf[..n])?;
        h.update(&buf[..n]);
        left -= n as u64;
    }
    let hex_str: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("sha256:{hex_str}"))
}

/// حساب وحفظ بصمة `sha256:<hex>` لكل قطاع مكتمل داخل `PartVerifyManifest`
pub fn hash_completed_segments_in_manifest(
    part: &Path,
    segs: &[FileSegment],
    manifest: &mut PartVerifyManifest,
) -> std::io::Result<usize> {
    manifest.segment_hashes.retain(|&id, _| {
        segs.iter()
            .any(|s| s.id == id && s.status == SegmentStatus::Completed)
    });
    if !segs.iter().any(|s| {
        s.status == SegmentStatus::Completed && !manifest.segment_hashes.contains_key(&s.id)
    }) {
        manifest.stamp = compute_part_file_stamp(part);
        return Ok(0);
    }
    let mut f = std::fs::File::open(part)?;
    let mut count = 0usize;
    for s in segs.iter().filter(|s| s.status == SegmentStatus::Completed) {
        if manifest.segment_hashes.contains_key(&s.id) {
            continue;
        }
        if s.end_byte >= s.start_byte && s.end_byte < u64::MAX {
            let seg_len = (s.end_byte - s.start_byte) + 1;
            if let Ok(digest) = hash_file_range(&mut f, s.start_byte, seg_len) {
                manifest.segment_hashes.insert(s.id, digest);
                count += 1;
            }
        }
    }
    manifest.stamp = compute_part_file_stamp(part);
    Ok(count)
}

/// إعادة فحص جميع القطاعات المكتملة التي تحمل بصمة `sha256:<hex>` عند الاستئناف؛
/// أي قطاع لا تطابق بايتاته بصمته المحفوظة يُفرَّغ وحده فقط (`current_offset = start_byte`, `Pending`) ليعاد تحميله!
pub fn reverify_completed_segments_with_manifest(
    part: &Path,
    segs: &mut [FileSegment],
    manifest: &mut PartVerifyManifest,
) -> std::io::Result<ReverifyReport> {
    let mut report = ReverifyReport::default();
    let mut f = std::fs::File::open(part)?;
    for s in segs
        .iter_mut()
        .filter(|s| s.status == SegmentStatus::Completed)
    {
        let Some(expected_hash) = manifest.segment_hashes.get(&s.id).cloned() else {
            report.unhashed += 1;
            continue;
        };
        if s.end_byte < s.start_byte || s.end_byte == u64::MAX {
            continue;
        }
        let seg_len = (s.end_byte - s.start_byte) + 1;
        report.checked += 1;
        let matches = hash_file_range(&mut f, s.start_byte, seg_len)
            .map(|got| got.eq_ignore_ascii_case(&expected_hash))
            .unwrap_or(false);
        if !matches {
            report.invalid += 1;
            report.invalid_bytes += seg_len;
            s.current_offset = s.start_byte;
            s.status = SegmentStatus::Pending;
            s.last_speed_bps = 0;
            manifest.segment_hashes.remove(&s.id);
        }
    }
    manifest.stamp = compute_part_file_stamp(part);
    Ok(report)
}

/// التحقق مما إذا كانت القطاعات تغطي النطاق `0..total` بالكامل بالترتيب وبدون أي فجوة أو تداخل
pub fn verify_exact_segment_coverage(segs: &[FileSegment], total: u64) -> Result<()> {
    if total == 0 {
        return Ok(());
    }
    let mut sorted = segs.to_vec();
    sorted.sort_by_key(|s| s.start_byte);
    let mut next = 0u64;
    for s in &sorted {
        if s.start_byte != next || s.end_byte < s.start_byte {
            bail!(
                "فجوة أو تداخل في القطاع #{}: يبدأ عند {} بينما المتوقع {}",
                s.id,
                s.start_byte,
                next
            );
        }
        next = s.end_byte + 1;
    }
    if next != total {
        bail!("نهاية القطاعات ({next}) لا تطابق حجم الملف الكلي ({total})");
    }
    Ok(())
}
