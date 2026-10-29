//! الميزة 3 من `VeloxFetch`: الاكتشاف التلقائي لبصمة الناشر (`Auto Publisher Checksum Discovery`)
//! مستلهم من `VeloxFetch/crates/download-manager/src/manager/publisher.rs` و `crates/downloader-core/src/integrity.rs`.
//!
//! يبحث تلقائياً أثناء التحميل في نفس مجلد الرابط على الخادم عن قوائم البصمات القياسية
//! (`<file>.sha256`, `<file>.sha512`, `SHA512SUMS`, `SHA256SUMS`, `SHA256SUMS.txt`, `SHASUMS256.txt`, `sha256sums.asc`)
//! بدون إرسال كوكيز أو بيانات جلسة، ويحلل صيغ GNU و BSD وملفات PGP الموقعة، ثم يتحقق من الملف قبل إعلانه مكتملاً!

use anyhow::{bail, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::io::Read;
use std::path::Path;
use std::time::Duration;
use url::Url;

const MAX_LIST_BYTES: usize = 1024 * 1024;
const MAX_LIST_LINES: usize = 20_000;

/// اللواحق القياسية لملفات البصمة الخاصة بملف واحد (`<file>.sha256`)
pub const SINGLE_FILE_CHECKSUM_EXTS: &[&str] = &["sha256", "sha512", "sha256sum", "sha512sum"];

/// الأسماء القياسية لقوائم البصمات المشتركة في مجلد الإصدار (الأقوى أولاً)
pub const FOLDER_CHECKSUM_LISTS: &[&str] = &[
    "SHA512SUMS",
    "SHA256SUMS",
    "SHA256SUMS.txt",
    "sha256sums.txt",
    "sha256sum.txt",
    "SHASUMS256.txt",
    "sha256sums.asc",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PublisherHashAlgo {
    Sha256,
    Sha512,
}

impl PublisherHashAlgo {
    pub fn hex_len(self) -> usize {
        match self {
            Self::Sha256 => 64,
            Self::Sha512 => 128,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
            Self::Sha512 => "sha512",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublisherChecksum {
    pub algo: PublisherHashAlgo,
    pub hex: String,
    pub source_list: String,
}

/// استخراج اسم الملف الأصلي من مسار الرابط
pub fn extract_url_leaf_name(url: &Url) -> Option<String> {
    let last = url.path_segments()?.next_back()?;
    let decoded = crate::core::filename::percent_decode_str(last);
    (!decoded.is_empty()
        && decoded.len() <= 255
        && !decoded.contains(['/', '\\', '\0'])
        && decoded != "."
        && decoded != "..")
        .then_some(decoded)
}

/// بناء قائمة الروابط المرشحة للبحث عن بصمة الناشر في نفس مجلد الملف على الخادم
pub fn publisher_checksum_candidates(url: &Url, allow_http: bool) -> Vec<(Url, String, bool)> {
    let Some(file) = extract_url_leaf_name(url) else {
        return Vec::new();
    };
    if !(url.scheme() == "https" || (allow_http && url.scheme() == "http"))
        || url.host_str().is_none()
    {
        return Vec::new();
    }
    let mut folder = url.clone();
    folder.set_query(None);
    folder.set_fragment(None);

    let mut out = Vec::new();
    for ext in SINGLE_FILE_CHECKSUM_EXTS {
        let name = format!("{file}.{ext}");
        if let Ok(u) = folder.join(&name) {
            out.push((u, name, true));
        }
    }
    for name in FOLDER_CHECKSUM_LISTS {
        if let Ok(u) = folder.join(name) {
            out.push((u, (*name).to_string(), false));
        }
    }
    out
}

/// تحليل محتوى قائمة بصمات الناشر (`GNU sha256sum`, `BSD SHA256 (name) = hex`, أو بصمة مفردة في `<file>.sha256`)
/// مستلهم من `VeloxFetch/crates/downloader-core/src/integrity.rs:find_in_list`
pub fn find_checksum_in_publisher_list(
    text: &str,
    file_name: &str,
    single_file_list: bool,
    source_list_name: &str,
) -> Option<PublisherChecksum> {
    if file_name.is_empty() || file_name.contains(['/', '\\', '\n', '\r']) {
        return None;
    }
    // رفض صفحات HTML الخادعة
    let lower_head = text.chars().take(256).collect::<String>().to_ascii_lowercase();
    if lower_head.contains("<!doctype html") || lower_head.contains("<html") {
        return None;
    }

    let parse_digest = |h: &str| -> Option<(PublisherHashAlgo, String)> {
        let algo = if h.len() == PublisherHashAlgo::Sha256.hex_len() {
            PublisherHashAlgo::Sha256
        } else if h.len() == PublisherHashAlgo::Sha512.hex_len() {
            PublisherHashAlgo::Sha512
        } else {
            return None
        };
        h.bytes()
            .all(|b| b.is_ascii_hexdigit())
            .then(|| (algo, h.to_ascii_lowercase()))
    };

    let matches_name = |n: &str| {
        let n = n.trim_end();
        let n = n.strip_prefix("./").unwrap_or(n);
        n == file_name || n.rsplit_once('/').is_some_and(|(_, last)| last == file_name)
    };

    let mut found: Vec<(PublisherHashAlgo, String)> = Vec::new();
    let mut lone: Vec<(PublisherHashAlgo, String)> = Vec::new();

    for line in text.lines().take(MAX_LIST_LINES) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // صيغة BSD: `SHA256 (name) = <hex>` أو `SHA512 (name) = <hex>`
        if let Some((head, hex_part)) = line.rsplit_once(") = ") {
            if let Some((tag, name)) = head.split_once(" (") {
                if let Some((algo, hex_str)) = parse_digest(hex_part.trim()) {
                    let tag_ok = matches!(
                        (tag.trim().to_ascii_uppercase().as_str(), algo),
                        ("SHA256" | "SHA2-256", PublisherHashAlgo::Sha256)
                            | ("SHA512" | "SHA2-512", PublisherHashAlgo::Sha512)
                    );
                    if tag_ok && matches_name(name) {
                        found.push((algo, hex_str));
                    }
                }
                continue;
            }
        }
        // صيغة GNU: `<hex>  name` أو `<hex> *name` أو بصمة وحيدة
        let (hex_part, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let Some((algo, hex_str)) = parse_digest(hex_part) else {
            continue;
        };
        let rest = rest.trim_start();
        let name = rest.strip_prefix('*').unwrap_or(rest);
        if name.is_empty() {
            lone.push((algo, hex_str));
        } else if matches_name(name) {
            found.push((algo, hex_str));
        }
    }

    if found.is_empty() && single_file_list {
        found = lone;
    }

    let pick_unambiguous =
        |target_algo: PublisherHashAlgo| -> std::result::Result<Option<String>, ()> {
            let mut matching: Vec<&str> = found
                .iter()
                .filter(|(a, _)| *a == target_algo)
                .map(|(_, h)| h.as_str())
                .collect();
            matching.sort_unstable();
            matching.dedup();
            match matching.as_slice() {
                [] => Ok(None),
                [only_one] => Ok(Some((*only_one).to_string())),
                _ => Err(()), // بصمتان مختلفتان لنفس الملف -> غموض مرفوض
            }
        };

    match (
        pick_unambiguous(PublisherHashAlgo::Sha512),
        pick_unambiguous(PublisherHashAlgo::Sha256),
    ) {
        (Err(()), _) | (_, Err(())) => None,
        (Ok(Some(hex)), _) => Some(PublisherChecksum {
            algo: PublisherHashAlgo::Sha512,
            hex,
            source_list: source_list_name.to_string(),
        }),
        (Ok(None), Ok(Some(hex))) => Some(PublisherChecksum {
            algo: PublisherHashAlgo::Sha256,
            hex,
            source_list: source_list_name.to_string(),
        }),
        _ => None,
    }
}

/// البحث التلقائي في الخلفية عن بصمة الناشر للملف على نفس المضيف
pub async fn discover_publisher_checksum(
    client: &Client,
    raw_file_url: &str,
    allow_http: bool,
) -> Option<PublisherChecksum> {
    let url = Url::parse(raw_file_url.trim()).ok()?;
    let file_name = extract_url_leaf_name(&url)?;
    let expected_host = url.host_str()?.to_ascii_lowercase();
    let candidates = publisher_checksum_candidates(&url, allow_http);
    if candidates.is_empty() {
        return None;
    }

    for (cand_url, list_name, is_single) in candidates {
        let Ok(Ok(resp)) =
            tokio::time::timeout(Duration::from_secs(4), client.get(cand_url).send()).await
        else {
            continue;
        };
        if !resp.status().is_success() {
            continue;
        }
        // رفض أي إعادة توجيه لمضيف آخر مختلف عن مضيف الملف الأصلي
        let final_host = resp
            .url()
            .host_str()
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if final_host != expected_host {
            continue;
        }
        if let Some(cl) = resp.content_length() {
            if cl == 0 || cl as usize > MAX_LIST_BYTES {
                continue;
            }
        }
        let Ok(bytes) = resp.bytes().await else {
            continue;
        };
        if bytes.is_empty() || bytes.len() > MAX_LIST_BYTES {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        if let Some(found) =
            find_checksum_in_publisher_list(text, &file_name, is_single, &list_name)
        {
            return Some(found);
        }
    }
    None
}

/// التحقق من مطابقة الملف النهائي لبصمة الناشر المكتشفة (`SHA-256` أو `SHA-512`)
pub async fn verify_file_against_publisher_checksum(
    path: &Path,
    checksum: &PublisherChecksum,
) -> Result<()> {
    let p = path.to_path_buf();
    let algo = checksum.algo;
    let actual_hex = tokio::task::spawn_blocking(move || -> std::io::Result<String> {
        let mut f = std::fs::File::open(p)?;
        let mut buf = vec![0u8; 1 << 20];
        match algo {
            PublisherHashAlgo::Sha256 => {
                let mut h = Sha256::new();
                loop {
                    let n = f.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    h.update(&buf[..n]);
                }
                Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
            }
            PublisherHashAlgo::Sha512 => {
                let mut h = Sha512::new();
                loop {
                    let n = f.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    h.update(&buf[..n]);
                }
                Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
            }
        }
    })
    .await??;

    if !actual_hex.eq_ignore_ascii_case(&checksum.hex) {
        bail!(
            "فشل التحقق من بصمة الناشر ({} من {}): المتوقع {} والفعلي {}",
            checksum.algo.name(),
            checksum.source_list,
            checksum.hex,
            actual_hex
        );
    }
    Ok(())
}
