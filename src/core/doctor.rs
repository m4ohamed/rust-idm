//! الميزة 6 من `VeloxFetch`: طبيب التحميل وذكاء ما قبل التحميل (`Download Doctor & Pre-Download Intelligence`)
//! مستلهم من `VeloxFetch/crates/download-manager/src/doctor.rs` و `src/intelligence.rs`.
//!
//! 1. **ذكاء ما قبل التحميل (`PreDownloadIntelligence`)**: يفحص الرابط قبل بدء التحميل ليكتشف:
//!    - هل أعاد الخادم صفحة ويب `HTML` (صفحة تسجيل دخول أو خطأ) بدلاً من الملف الحقيقي؟
//!    - هل الرابط مؤقت/موقّع (`X-Amz-Expires`, `expire=`, `sig=`, `token=`) ومتى ينتهي؟
//!    - هل حدث خفض أمان من `HTTPS` إلى `HTTP` أثناء التوجيه (`HttpsDowngrade`)؟
//!    - هل الملف مكرر في القائمة الحالية (بنفس الرابط، أو نفس `ETag` القوي + الحجم، أو نفس البصمة)؟
//! 2. **طبيب التحميل (`DownloadDoctor`)**: يشخص سبب فشل المهمة بدقة (انتهاء صلاحية الرابط، انتهاء جلسة الكوكيز،
//!    تقييد `429`، امتلاء القرص، رفض `Range`) ويقترح الإجراء العلاجي المباشر.

use crate::core::storage::{DownloadStateMeta, DownloadStatus};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceKind {
    File,
    WebPage,
    Stream,
    VideoPage,
    Torrent,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResumeSupportLevel {
    /// يدعم `206 Partial Content` مع حجم معلوم و `ETag` قوي أو `Last-Modified`
    Supported,
    /// يدعم `206 Partial Content` بحجم معلوم ولكن بدون `ETag` (يعتمد على فحص العينات `Sample-Based Resume`)
    LikelySupportedViaSamples,
    NotSupported,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FindingCode {
    InsecureHttp,
    HttpsDowngrade,
    AuthRequired,
    SessionRequired,
    AccessRefused,
    TemporarySignedLink,
    ExpiredLink,
    WebPageInsteadOfFile,
    InsufficientDiskSpace,
    Fat32FourGigabyteLimit,
    NoResumeSupport,
    ResumeVerifiedBySamples,
    DuplicateDownload,
    StreamProtocolDetected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DuplicateMatchReason {
    SameLink,
    SameStrongEtagAndSize,
    SameChecksum,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateMatchInfo {
    pub task_id: String,
    pub filename: String,
    pub status: DownloadStatus,
    pub reason: DuplicateMatchReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkIntelligenceReport {
    pub url: String,
    pub resource_kind: ResourceKind,
    pub resume_support: ResumeSupportLevel,
    pub recommended_connections: usize,
    pub is_temporary_link: bool,
    pub expires_at_unix_secs: Option<u64>,
    pub findings: Vec<FindingCode>,
    pub duplicate: Option<DuplicateMatchInfo>,
    pub summary_arabic: String,
}

/// فحص ما إذا كان الرابط يحمل توقيعاً زمنياً مؤقتاً (AWS S3, Cloudflare, GoogleVideo, CDN tokens) واستخراج وقت الانتهاء إن وجد
pub fn inspect_temporary_signed_url(raw_url: &str) -> (bool, Option<u64>) {
    let Ok(parsed) = Url::parse(raw_url.trim()) else {
        return (false, None);
    };
    let mut is_signed = false;
    let mut expires_at: Option<u64> = None;

    for (k, v) in parsed.query_pairs() {
        let kl = k.to_ascii_lowercase();
        if matches!(
            kl.as_str(),
            "sig"
                | "signature"
                | "token"
                | "hdnea"
                | "x-amz-signature"
                | "policy"
                | "key-pair-id"
                | "sparams"
        ) {
            is_signed = true;
        }
        if matches!(
            kl.as_str(),
            "expire" | "expires" | "exp" | "e" | "oe" | "x-amz-expires"
        ) {
            is_signed = true;
            if let Ok(val) = v.trim().parse::<u64>() {
                if val > 1_600_000_000 && val < 9_999_999_999 {
                    expires_at = Some(val);
                }
            }
        }
    }
    (is_signed, expires_at)
}

/// كشف ما إذا كان الرابط مكرراً ضمن قائمة المهام الحالية (بمقارنة الرابط بدون `#fragment`، أو `ETag` القوي + الحجم، أو `SHA-256`)
pub fn find_smart_duplicate(
    url: &str,
    probed_size: u64,
    probed_etag: Option<&str>,
    expected_sha256: Option<&str>,
    existing_tasks: &[DownloadStateMeta],
) -> Option<DuplicateMatchInfo> {
    let normalize_u = |u: &str| u.trim().split('#').next().unwrap_or("").to_string();
    let target_norm = normalize_u(url);

    for task in existing_tasks {
        if normalize_u(&task.url) == target_norm {
            return Some(DuplicateMatchInfo {
                task_id: task.task_id.clone(),
                filename: task.filename.clone(),
                status: task.status,
                reason: DuplicateMatchReason::SameLink,
            });
        }
        if let (Some(e1), Some(e2)) = (probed_etag, task.etag.as_deref()) {
            let clean1 = e1.trim();
            let clean2 = e2.trim();
            if !clean1.is_empty()
                && !clean1.starts_with("W/")
                && clean1 == clean2
                && probed_size > 0
                && task.total_size == probed_size
            {
                return Some(DuplicateMatchInfo {
                    task_id: task.task_id.clone(),
                    filename: task.filename.clone(),
                    status: task.status,
                    reason: DuplicateMatchReason::SameStrongEtagAndSize,
                });
            }
        }
        if let (Some(h1), Some(h2)) = (
            expected_sha256.filter(|s| !s.trim().is_empty()),
            task.expected_sha256
                .as_deref()
                .or(task.computed_sha256.as_deref()),
        ) {
            if h1.trim().eq_ignore_ascii_case(h2.trim()) {
                return Some(DuplicateMatchInfo {
                    task_id: task.task_id.clone(),
                    filename: task.filename.clone(),
                    status: task.status,
                    reason: DuplicateMatchReason::SameChecksum,
                });
            }
        }
    }
    None
}

/// تحليل شامل للرابط قبل بدء التحميل (`Pre-Download Intelligence`)
#[allow(clippy::too_many_arguments)]
pub fn evaluate_link_intelligence(
    original_url: &str,
    final_url: &str,
    content_type: Option<&str>,
    total_size: u64,
    resumable: bool,
    etag: Option<&str>,
    last_modified: Option<&str>,
    existing_tasks: &[DownloadStateMeta],
) -> LinkIntelligenceReport {
    let mut findings = Vec::new();
    let lower_url = final_url.to_ascii_lowercase();

    if original_url.starts_with("https://") && final_url.starts_with("http://") {
        findings.push(FindingCode::HttpsDowngrade);
    } else if final_url.starts_with("http://") {
        findings.push(FindingCode::InsecureHttp);
    }

    let is_html = content_type
        .map(|ct| ct.to_ascii_lowercase().contains("text/html"))
        .unwrap_or(false)
        && !lower_url.ends_with(".html")
        && !lower_url.ends_with(".htm");

    let resource_kind = if crate::core::torrent::is_torrent_source(original_url) {
        ResourceKind::Torrent
    } else if lower_url.contains(".m3u8") || lower_url.contains(".mpd") {
        findings.push(FindingCode::StreamProtocolDetected);
        ResourceKind::Stream
    } else if is_html {
        findings.push(FindingCode::WebPageInsteadOfFile);
        ResourceKind::WebPage
    } else {
        ResourceKind::File
    };

    let has_validator = etag
        .map(|e| !e.trim().is_empty())
        .unwrap_or(false)
        || last_modified
            .map(|l| !l.trim().is_empty())
            .unwrap_or(false);

    let resume_support = if resumable && total_size > 0 && has_validator {
        ResumeSupportLevel::Supported
    } else if resumable && total_size > 0 {
        findings.push(FindingCode::ResumeVerifiedBySamples);
        ResumeSupportLevel::LikelySupportedViaSamples
    } else {
        findings.push(FindingCode::NoResumeSupport);
        ResumeSupportLevel::NotSupported
    };

    let (is_temp, expires_at) = inspect_temporary_signed_url(final_url);
    if is_temp {
        findings.push(FindingCode::TemporarySignedLink);
    }

    let duplicate = find_smart_duplicate(original_url, total_size, etag, None, existing_tasks);
    if duplicate.is_some() {
        findings.push(FindingCode::DuplicateDownload);
    }

    let recommended_connections = if !resumable || total_size < 512 * 1024 {
        1
    } else if total_size >= 100 * 1024 * 1024 {
        16
    } else {
        8
    };

    let summary_arabic = if is_html {
        "تنبيه: الرابط أعاد صفحة ويب (HTML) بدلاً من ملف ثنائي؛ قد يتطلب تسجيل دخول في المتصفح.".to_string()
    } else if let Some(ref dup) = duplicate {
        format!("هذا الملف موجود مسبقاً في القائمة باسم '{}' ({:?}).", dup.filename, dup.status)
    } else if resumable {
        format!(
            "الرابط جاهز للتحميل السريع متعدد الاتصالات ({} اتصالات مبدئية، قابل للاستئناف).",
            recommended_connections
        )
    } else {
        "الخادم لا يدعم تقسيم النطاقات (Range)؛ سيتم التحميل باتصال واحد مستقر.".to_string()
    };

    LinkIntelligenceReport {
        url: final_url.to_string(),
        resource_kind,
        resume_support,
        recommended_connections,
        is_temporary_link: is_temp,
        expires_at_unix_secs: expires_at,
        findings,
        duplicate,
        summary_arabic,
    }
}

// ============================================================================
// 2. طبيب التحميل (Download Doctor Diagnosis Engine)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DoctorCategory {
    SessionExpired,
    LinkExpired,
    AuthRequired,
    RateLimited,
    ServerTemporaryError,
    RangeRejectedMidTransfer,
    SourceFileMutated,
    DiskFullOrFat32Limit,
    ChecksumMismatch,
    NetworkConnectionProblem,
    CancelledByUser,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DoctorAction {
    RetryNow,
    WaitAndRetryLater,
    RefreshLinkFromBrowserPage,
    ReauthenticateInBrowser,
    FreeDiskSpaceOrChangeDrive,
    RestartDownloadFromZero,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorDiagnosis {
    pub category: DoctorCategory,
    pub worked_before: bool,
    pub temporary_link: bool,
    pub session_used: bool,
    pub can_preserve_saved_bytes: bool,
    pub recommended_actions: Vec<DoctorAction>,
    pub explanation_arabic: String,
}

/// تشخيص سبب فشل أو توقف مهمة التحميل واقتراح الحل الدقيق (`Download Doctor`)
pub fn diagnose_task_with_doctor(meta: &DownloadStateMeta) -> DoctorDiagnosis {
    let err = meta.last_error.as_deref().unwrap_or("").to_ascii_lowercase();
    let worked_before = meta.total_downloaded() > 0;
    let (temporary_link, _) = inspect_temporary_signed_url(&meta.url);
    let session_used = meta.custom_headers.keys().any(|k| {
        let kl = k.to_ascii_lowercase();
        kl.contains("cookie") || kl.contains("authorization")
    });

    if err.contains("cancelled by user") || err.contains("إلغاء") {
        return DoctorDiagnosis {
            category: DoctorCategory::CancelledByUser,
            worked_before,
            temporary_link,
            session_used,
            can_preserve_saved_bytes: meta.resumable && worked_before,
            recommended_actions: vec![DoctorAction::RetryNow],
            explanation_arabic: "تم إيقاف المهمة يدوياً؛ يمكنك استئنافها في أي وقت.".to_string(),
        };
    }

    if err.contains("401") || err.contains("403") || err.contains("410") || err.contains("انتهت صلاحية") {
        if worked_before && session_used {
            return DoctorDiagnosis {
                category: DoctorCategory::SessionExpired,
                worked_before,
                temporary_link,
                session_used,
                can_preserve_saved_bytes: true,
                recommended_actions: vec![
                    DoctorAction::ReauthenticateInBrowser,
                    DoctorAction::RefreshLinkFromBrowserPage,
                ],
                explanation_arabic: "انتهت صلاحية جلسة تسجيل الدخول (الكوكيز) بعد تحميل جزء من الملف. افتح صفحة الموقع في المتصفح لتجديد الرابط مع الحفاظ على البايتات المحمّلة.".to_string(),
            };
        }
        if temporary_link || worked_before {
            return DoctorDiagnosis {
                category: DoctorCategory::LinkExpired,
                worked_before,
                temporary_link,
                session_used,
                can_preserve_saved_bytes: true,
                recommended_actions: vec![DoctorAction::RefreshLinkFromBrowserPage],
                explanation_arabic: "انتهت صلاحية التوقيع الزمني للرابط المؤقت. استخدم ميزة تجديد الرابط (Refresh URL) لاستكمال التحميل من حيث توقف.".to_string(),
            };
        }
        return DoctorDiagnosis {
            category: DoctorCategory::AuthRequired,
            worked_before,
            temporary_link,
            session_used,
            can_preserve_saved_bytes: false,
            recommended_actions: vec![DoctorAction::ReauthenticateInBrowser],
            explanation_arabic: "يرفض الخادم الوصول (401/403)؛ يتطلب الرابط تسجيل الدخول أو تمرير كوكيز المتصفح.".to_string(),
        };
    }

    if err.contains("429") || err.contains("too many requests") {
        return DoctorDiagnosis {
            category: DoctorCategory::RateLimited,
            worked_before,
            temporary_link,
            session_used,
            can_preserve_saved_bytes: true,
            recommended_actions: vec![DoctorAction::WaitAndRetryLater],
            explanation_arabic: "قيّد الخادم الطلبات مؤقتاً بسبب كثرة الاتصالات (HTTP 429). سيقوم المتحكم التكيفي بتخفيض عدد الاتصالات والانتظار قبل الاستئناف.".to_string(),
        };
    }

    if err.contains("fat32") || err.contains("المساحة الحرة") || err.contains("space") {
        return DoctorDiagnosis {
            category: DoctorCategory::DiskFullOrFat32Limit,
            worked_before,
            temporary_link,
            session_used,
            can_preserve_saved_bytes: true,
            recommended_actions: vec![DoctorAction::FreeDiskSpaceOrChangeDrive],
            explanation_arabic: "المساحة الحرة على القرص غير كافية أو أن القرص بنظام FAT32 لا يقبل ملفات أكبر من 4 جيجابايت.".to_string(),
        };
    }

    if err.contains("sha-256") || err.contains("md5") || err.contains("crc32") || err.contains("بصمة") {
        return DoctorDiagnosis {
            category: DoctorCategory::ChecksumMismatch,
            worked_before,
            temporary_link,
            session_used,
            can_preserve_saved_bytes: false,
            recommended_actions: vec![DoctorAction::RestartDownloadFromZero],
            explanation_arabic: "البصمة الرقمية للملف المحمّل لا تطابق البصمة المتوقعة؛ يوصى بإعادة التحميل لضمان سلامة الملف.".to_string(),
        };
    }

    if err.contains("resource_mutated") || err.contains("تغير حجم الملف") {
        return DoctorDiagnosis {
            category: DoctorCategory::SourceFileMutated,
            worked_before,
            temporary_link,
            session_used,
            can_preserve_saved_bytes: false,
            recommended_actions: vec![DoctorAction::RestartDownloadFromZero],
            explanation_arabic: "تغيرت نسخة الملف على الخادم أثناء التحميل؛ يجب البدء من الصفر لمنع دمج نسختين مختلفتين.".to_string(),
        };
    }

    DoctorDiagnosis {
        category: DoctorCategory::NetworkConnectionProblem,
        worked_before,
        temporary_link,
        session_used,
        can_preserve_saved_bytes: meta.resumable,
        recommended_actions: vec![DoctorAction::RetryNow],
        explanation_arabic: "حدث انقطاع في الشبكة أو مهلة الاتصال؛ البايتات المحمّلة محفوظة بأمان ويمكنك الاستئناف فوراً.".to_string(),
    }
}
