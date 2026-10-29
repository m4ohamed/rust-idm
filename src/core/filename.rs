//! وحدة استخلاص وتطهير وحل تعارض أسماء الملفات الذكية (`Smart Filename Resolution & MIME Inference`)
//!
//! تجمع هذه الوحدة أفضل الممارسات الهندسية من `StormGet` و `dlman`:
//! 1. تحليل ترويسة `Content-Disposition` وفق معياري RFC 6266 و RFC 5987 (`filename*=UTF-8''...` له الأولوية على `filename="..."`)
//!    مع محلل معاملات يحترم علامات التنصيص والفواصل المنقوطة داخل الأسماء (`filename="a;b.zip"`).
//! 2. استنتاج الامتداد التلقائي من ترويسة `Content-Type` لأكثر من 55 نوع MIME عندما يخلو الرابط من الامتداد
//!    (مثل روابط GitHub `/zip/refs/heads/main` أو روابط المثبتات `/installer/download/XYZ`).
//! 3. فصل جذر الاسم عن الامتداد بذكاء (`split_stem_ext`) بحيث لا تُعامل أرقام الإصدارات (مثل `app-v1.11.1` أو `archive.001`) كامتدادات.
//! 4. الحماية من تزييف الامتداد بمحارف BiDi العكسية (`U+202E`)، ومنع اختراق المسارات (`../../`)، وحماية أسماء ويندوز المحجوزة (`CON`, `NUL`, `COM1`..`LPT9`).
//! 5. حل تعارض الأسماء التلقائي (`resolve_unique_filename`) بإضافة لاحقة ` (1)`, ` (2)` قبل الامتداد الفعلي دون كسر أرقام الإصدارات.

use reqwest::header::{HeaderMap, CONTENT_DISPOSITION, CONTENT_TYPE};
use std::path::Path;
use url::Url;

/// الاسم الافتراضي عند تعذر استخراج أي اسم صالح من الترويسات أو الرابط
pub const DEFAULT_FILENAME: &str = "download";

/// الحد الأقصى لطول اسم الملف بالأحرف (يترك هامشاً آمنًا لـ `.part` و `.idm.json` ولاحقة ` (12)` ضمن حد 255 بايت)
pub const MAX_FILENAME_CHARS: usize = 120;

/// المحارف المحظورة في أسماء ملفات Windows و POSIX
const ILLEGAL_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// أسماء الأجهزة المحجوزة في نظام Windows (تُرفض حتى لو تبعها امتداد مثل `NUL.txt` أو `CON.tar.gz`)
const WINDOWS_RESERVED_STEMS: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    "COM\u{b9}", "COM\u{b2}", "COM\u{b3}", "LPT\u{b9}", "LPT\u{b2}", "LPT\u{b3}",
    "CONIN$", "CONOUT$",
];

/// أزواج الامتدادات المركبة الشائعة التي يجب الحفاظ عليها معاً عند التقصير أو إضافة رقم عدم التكرار
const COMPOUND_EXTENSIONS: &[&str] = &[
    "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz", "user.js",
];

/// فحص محارف التحكم باتجاه النص (BiDi Override) التي تُستغل أمنياً لتزييف امتداد الملفات التنفيذية
#[inline]
fn is_bidi_control(c: char) -> bool {
    crate::core::path_safety::is_invisible_or_bidi_spoofing_char(c)
}

/// فك ترميز النسبة المئوية (Percent-Decoding RFC 3986) لأسماء الملفات
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

/// استخلاص أفضل اسم ملف متاح من الرابط وترويسات الاستجابة (`Content-Disposition` ثم `URL` ثم `Content-Type`)
pub fn resolve_filename_from_url_and_headers(url_str: &str, headers: &HeaderMap) -> String {
    let content_disposition = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok());
    let content_type = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok());

    if let Ok(parsed_url) = Url::parse(url_str) {
        resolve_filename(&parsed_url, content_disposition, content_type)
    } else {
        let base = content_disposition
            .and_then(filename_from_content_disposition)
            .and_then(|n| sanitize_filename_opt(&n))
            .unwrap_or_else(|| DEFAULT_FILENAME.to_string());
        ensure_extension(base, content_type)
    }
}

/// اختيار أفضل اسم ملف متاح بالترتيب القياسي:
/// 1. ترويسة `Content-Disposition` (`filename*` ثم `filename`)
/// 2. آخر مقطع غير فارغ في مسار الرابط (`URL path segment`) أو معاملات الاستعلام الشائعة (`file=`, `filename=`)
/// 3. الاسم الافتراضي `download`
/// ثم إضافة امتداد مستنتج من `Content-Type` إذا كان الاسم الناتج بلا امتداد.
pub fn resolve_filename(
    url: &Url,
    content_disposition: Option<&str>,
    content_type: Option<&str>,
) -> String {
    let name = content_disposition
        .and_then(filename_from_content_disposition)
        .and_then(|name| sanitize_filename_opt(&name))
        .or_else(|| filename_from_url(url))
        .unwrap_or_else(|| DEFAULT_FILENAME.to_string());

    ensure_extension(name, content_type)
}

/// استخراج اسم الملف من ترويسة `Content-Disposition` مع إعطاء الأولوية لصيغة RFC 5987 (`filename*=`)
pub fn filename_from_content_disposition(header: &str) -> Option<String> {
    let mut plain: Option<String> = None;
    let mut extended: Option<String> = None;

    for param in split_params(header) {
        let (key, value) = match param.split_once('=') {
            Some((key, value)) => (key.trim().to_ascii_lowercase(), value.trim()),
            None => continue,
        };

        match key.as_str() {
            "filename" if plain.is_none() => {
                let unquoted = unquote(value);
                let decoded = percent_decode_str(&unquoted);
                if !decoded.trim().is_empty() {
                    plain = Some(decoded);
                }
            }
            "filename*" if extended.is_none() => {
                extended = decode_extended_value(value);
            }
            _ => {}
        }
    }

    // وفق RFC 6266 §4.3: عند وجود الصيغتين معاً، تفوز `filename*` دائماً
    extended.or(plain).filter(|name| !name.trim().is_empty())
}

/// استخراج اسم الملف من آخر مقطع ذي معنى في مسار الرابط، أو من معاملات الاستعلام (`filename=`, `response-content-disposition=`)
pub fn filename_from_url(url: &Url) -> Option<String> {
    // 1. فحص معاملات الاستعلام الخاصة بـ S3 / CDN إذا وُجدت (مثل response-content-disposition أو filename)
    for (k, v) in url.query_pairs() {
        let k_low = k.to_ascii_lowercase();
        if k_low == "response-content-disposition" || k_low == "rscd" {
            if let Some(from_cd) = filename_from_content_disposition(&v) {
                if let Some(clean) = sanitize_filename_opt(&from_cd) {
                    return Some(clean);
                }
            }
        } else if k_low == "filename" || k_low == "file" || k_low == "title" {
            if let Some(clean) = sanitize_filename_opt(&v) {
                return Some(clean);
            }
        }
    }

    // 2. آخر مقطع غير فارغ في المسار
    let segment = url.path_segments()?.rfind(|s| !s.is_empty())?;
    let decoded = percent_decode_str(segment);
    sanitize_filename_opt(&decoded)
}

/// تطهير اسم الملف وإرجاع `Option<String>` (`None` إذا لم يتبقَّ أي حرف صالح)
pub fn sanitize_filename_opt(raw: &str) -> Option<String> {
    let _hardened = crate::core::path_safety::harden_filename_against_spoofing(raw);
    // منع اختراق المسارات (`../../etc/passwd` أو `..\..\evil.dll`): نأخذ فقط المكون الأخير
    // فقط إذا كان النص يبدو كمسار نسبي خطير (`..`)، أما المحارف العادية فتُستبدل بـ `_`
    let target = if raw.contains("../") || raw.contains("..\\") || raw.starts_with('/') || raw.starts_with('\\') {
        raw.rsplit(['/', '\\']).find(|s| !s.trim().is_empty()).unwrap_or(raw).trim()
    } else {
        raw.trim()
    };

    if target.is_empty() || target == "." || target == ".." {
        return None;
    }

    let replaced: String = target
        .chars()
        .filter(|c| !is_bidi_control(*c))
        .map(|c| {
            if ILLEGAL_CHARS.contains(&c) || (c as u32) < 0x20 || (c as u32) == 0x7F {
                '_'
            } else {
                c
            }
        })
        .collect();

    let trimmed = replaced.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        return None;
    }

    let mut out = truncate_keeping_extension(trimmed, MAX_FILENAME_CHARS);

    // فحص أسماء الأجهزة المحجوزة في Windows على الجزء الواقع قبل أول نقطة (`CON` و `CON.tar.gz` و `nul.txt`)
    let first_stem = out.split('.').next().unwrap_or("").trim_end();
    if WINDOWS_RESERVED_STEMS
        .iter()
        .any(|reserved| first_stem.eq_ignore_ascii_case(reserved))
    {
        out.insert(0, '_');
    }

    let finalized = out.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    if finalized.is_empty() {
        None
    } else {
        Some(finalized.to_string())
    }
}

/// تطهير اسم الملف وإرجاع `"download"` كقيمة افتراضية آمنة إذا كان الاسم فارغاً
pub fn sanitize_filename(name: &str) -> String {
    sanitize_filename_opt(name).unwrap_or_else(|| DEFAULT_FILENAME.to_string())
}

/// فصل اسم الملف إلى `(stem, extension)` دون الخلط بين أرقام الإصدارات (`v1.11.1`) والامتدادات الحقيقية
pub fn split_stem_ext(filename: &str) -> (&str, Option<&str>) {
    let lower = filename.to_ascii_lowercase();
    for &compound in COMPOUND_EXTENSIONS {
        if lower.len() > compound.len() + 1 && lower.ends_with(compound) {
            let dot_pos = filename.len() - compound.len() - 1;
            if filename.as_bytes().get(dot_pos) == Some(&b'.') && dot_pos > 0 {
                return (&filename[..dot_pos], Some(&filename[dot_pos + 1..]));
            }
        }
    }

    match filename.rfind('.') {
        Some(index) if index > 0 => {
            let extension = &filename[index + 1..];
            if is_plausible_extension(extension) {
                (&filename[..index], Some(extension))
            } else {
                (filename, None)
            }
        }
        _ => (filename, None),
    }
}

/// التحقق مما إذا كان النص الواقع بعد النقطة امتداد ملف حقيقياً وليس رقم إصدار (`v1.11.1` أو `001`)
pub fn is_plausible_extension(extension: &str) -> bool {
    !extension.is_empty()
        && extension.len() <= 10
        && extension.chars().all(|c| c.is_ascii_alphanumeric())
        && !extension.chars().all(|c| c.is_ascii_digit())
}

/// استنتاج الامتداد القياسي من ترويسة `Content-Type` لأكثر من 55 نوع MIME شائع
pub fn extension_for_mime(content_type: &str) -> Option<&'static str> {
    let mime = content_type.split(';').next()?.trim().to_ascii_lowercase();

    let ext = match mime.as_str() {
        // الأرشيفات والملفات المضغوطة
        "application/zip" | "application/x-zip-compressed" | "application/x-zip" => "zip",
        "application/gzip" | "application/x-gzip" => "gz",
        "application/x-tar" => "tar",
        "application/x-gtar" | "application/x-tgz" => "tar.gz",
        "application/x-7z-compressed" => "7z",
        "application/vnd.rar" | "application/x-rar-compressed" | "application/x-rar" => "rar",
        "application/x-bzip2" | "application/x-bzip" => "bz2",
        "application/x-xz" => "xz",
        "application/zstd" => "zst",
        // البرامج والمثبتات وصور الأقراص
        "application/x-msdownload"
        | "application/x-msdos-program"
        | "application/vnd.microsoft.portable-executable"
        | "application/x-dosexec" => "exe",
        "application/x-msi" | "application/x-ms-installer" => "msi",
        "application/x-apple-diskimage" => "dmg",
        "application/vnd.debian.binary-package" | "application/x-deb" => "deb",
        "application/x-rpm" | "application/x-redhat-package-manager" => "rpm",
        "application/vnd.android.package-archive" => "apk",
        "application/x-iso9660-image" => "iso",
        "application/x-appimage" => "AppImage",
        // المستندات والكتب
        "application/pdf" => "pdf",
        "application/epub+zip" => "epub",
        "application/rtf" => "rtf",
        "application/msword" => "doc",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "application/vnd.ms-excel" => "xls",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        "application/vnd.ms-powerpoint" => "ppt",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => "pptx",
        // النصوص والبيانات والبرمجة
        "text/plain" => "txt",
        "text/html" => "html",
        "text/css" => "css",
        "text/csv" => "csv",
        "text/markdown" => "md",
        "application/json" => "json",
        "application/xml" | "text/xml" => "xml",
        "application/javascript" | "text/javascript" => "js",
        "application/wasm" => "wasm",
        // الصور
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/avif" => "avif",
        "image/svg+xml" => "svg",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        "image/bmp" => "bmp",
        // الفيديو والبث
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/x-matroska" => "mkv",
        "video/quicktime" => "mov",
        "video/x-msvideo" => "avi",
        "video/x-flv" => "flv",
        "video/mp2t" => "ts",
        // الصوتيات
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/ogg" | "application/ogg" => "ogg",
        "audio/flac" | "audio/x-flac" => "flac",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/aac" => "aac",
        "audio/opus" => "opus",
        _ => return None,
    };

    Some(ext)
}

/// إلحاق امتداد مستنتج من `Content-Type` بالاسم إذا لم يكن يحمل امتداداً صالحاً بالفعل
pub fn ensure_extension(filename: String, content_type: Option<&str>) -> String {
    if split_stem_ext(&filename).1.is_some() {
        return filename;
    }

    match content_type.and_then(extension_for_mime) {
        Some(ext) => {
            let clean = filename.trim_end_matches(['.', ' ']);
            let combined = format!("{clean}.{ext}");
            truncate_keeping_extension(&combined, MAX_FILENAME_CHARS)
        }
        None => filename,
    }
}

/// حل تعارض اسم الملف في مجلد الحفظ: إذا كان الملف (أو ملف `.part` الخاص به) موجوداً مسبقاً،
/// يولّد اسماً فريداً بصيغة `stem (1).ext`, `stem (2).ext` مع الحفاظ على أرقام الإصدارات (`app-v1.11.1 (1)`).
pub fn resolve_unique_filename(save_dir: &Path, filename: &str, overwrite: bool) -> String {
    let sanitized = sanitize_filename(filename);
    if overwrite {
        return sanitized;
    }

    let final_exists = |candidate: &str| -> bool {
        save_dir.join(candidate).exists()
            || save_dir.join(format!("{candidate}.part")).exists()
            || save_dir.join(format!("{candidate}.idm.json")).exists()
    };

    if !final_exists(&sanitized) {
        return sanitized;
    }

    let (stem, ext) = split_stem_ext(&sanitized);
    for counter in 1..=9999 {
        let candidate = match ext {
            Some(e) => format!("{stem} ({counter}).{e}"),
            None => format!("{stem} ({counter})"),
        };
        if !final_exists(&candidate) {
            return candidate;
        }
    }

    sanitized
}

/// تقسيم ترويسة `Content-Disposition` على `;` مع تجاهل الفواصل المنقوطة الواقعة داخل علامات التنصيص
fn split_params(header: &str) -> Vec<&str> {
    let mut params = Vec::new();
    let mut start = 0;
    let mut in_quotes = false;
    let mut escaped = false;

    for (index, c) in header.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_quotes => escaped = true,
            '"' => in_quotes = !in_quotes,
            ';' if !in_quotes => {
                params.push(&header[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    params.push(&header[start..]);
    params
}

/// إزالة علامات التنصيص المحيطة وفك تهريب المحارف (`\"` -> `"`)
fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        let inner = &value[1..value.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(escaped) = chars.next() {
                    out.push(escaped);
                }
            } else {
                out.push(c);
            }
        }
        out
    } else {
        value.trim_matches('\'').to_string()
    }
}

/// فك ترميز قيمة RFC 5987 الممتدة: `charset'language'pct-encoded` (مثل `UTF-8''%D9%85%D9%84%D9%81.zip`)
fn decode_extended_value(value: &str) -> Option<String> {
    let value = unquote(value);
    let mut parts = value.splitn(3, '\'');

    let charset = parts.next()?.trim().to_ascii_lowercase();
    let _language = parts.next()?;
    let encoded = parts.next()?;

    if !charset.is_empty() && charset != "utf-8" && charset != "utf8" && charset != "us-ascii" {
        return None;
    }

    let decoded = percent_decode_str(encoded);
    (!decoded.trim().is_empty()).then_some(decoded)
}

/// تقصير اسم الملف إلى `max_chars` حرفاً مع الحفاظ على الامتداد
pub fn truncate_keeping_extension(name: &str, max_chars: usize) -> String {
    let total = name.chars().count();
    if total <= max_chars {
        return name.to_string();
    }

    let (stem, ext) = split_stem_ext(name);
    if let Some(ext_str) = ext {
        let ext_with_dot_len = ext_str.chars().count() + 1;
        if ext_with_dot_len < max_chars {
            let keep = max_chars - ext_with_dot_len;
            let base: String = stem.chars().take(keep).collect();
            return format!("{}.{}", base.trim_end(), ext_str);
        }
    }

    // fallback: if there is a short raw extension up to 16 chars
    if let Some(i) = name.rfind('.').filter(|&idx| idx > 0) {
        let ext_part = &name[i..];
        let ext_chars = ext_part.chars().count();
        if ext_chars <= 16 && ext_chars < max_chars {
            let keep = max_chars - ext_chars;
            let base: String = name[..i].chars().take(keep).collect();
            return format!("{}{}", base, ext_part);
        }
    }

    name.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_disposition_quoted_semicolon_and_rfc5987_precedence() {
        assert_eq!(
            filename_from_content_disposition("attachment; filename=\"a;b.zip\"; size=42")
                .as_deref(),
            Some("a;b.zip")
        );

        let header = "attachment; filename=\"fallback.zip\"; filename*=UTF-8''caf%C3%A9.zip";
        assert_eq!(
            filename_from_content_disposition(header).as_deref(),
            Some("café.zip")
        );
    }

    #[test]
    fn infers_extension_from_mime_when_url_has_no_extension() {
        let url = Url::parse("https://codeload.github.com/novincode/dlman/zip/refs/heads/main").expect("verified in test");
        let resolved = resolve_filename(&url, None, Some("application/zip"));
        assert_eq!(resolved, "main.zip");

        let ms_url = Url::parse("https://get.microsoft.com/installer/download/9N7JSXC1SJK6").expect("verified in test");
        let resolved_exe = resolve_filename(&ms_url, None, Some("application/x-msdownload"));
        assert_eq!(resolved_exe, "9N7JSXC1SJK6.exe");
    }

    #[test]
    fn version_numbers_are_not_treated_as_extensions() {
        assert_eq!(
            split_stem_ext("dlman-extension-chrome-v1.11.1"),
            ("dlman-extension-chrome-v1.11.1", None)
        );
        assert_eq!(split_stem_ext("archive.001"), ("archive.001", None));
        assert_eq!(split_stem_ext("backup.tar.gz"), ("backup", Some("tar.gz")));
    }
}
