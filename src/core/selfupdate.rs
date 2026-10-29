//! نظام التحديث الذاتي الموقّع رقمياً بـ `Ed25519` مع الفحص المزدوج لبصمة `SHA-256` (حماية `TOCTOU`)
//! مستلهم من `ezoo-download/crates/engine/src/selfupdate.rs`.
//!
//! يتحقق من:
//! 1. أن إصدار `version.json` أعلى من الإصدار الحالي (`is_newer`).
//! 2. أن الرسالة الموقعة `stormget-update\n<version>\n<sha256>\n` تحمل توقيع `Ed25519` صحيحاً بالمفتاح العام المدمج (`signature_ok`).
//! 3. أن رابط المثبت ينتمي حصرياً إلى مستودع GitHub الرسمي (`setup_url`).
//! 4. تطابق بصمة `SHA-256` أثناء التنزيل، ثم إعادة فحص `SHA-256` مرة ثانية قبل تشغيل المثبت مباشرةً لمنع استبدال الملف على القرص (`run_installer`).

use anyhow::{bail, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const UPDATE_REPO: &str = "StormGet/StormGet";
pub const SETUP_FILENAME: &str = "StormGet-Setup.exe";

/// المفتاح العام المدمج للتحقق من توقيع التحديثات (Ed25519، 32 بايت بصيغة hex)
pub const UPDATE_PUBLIC_KEY: &str =
    "4bd5c564c841b012b39426755d585c48012f9360e193cd81d141f85ad9b58406";

/// الحد الأقصى المسموح لحجم ملف التثبيت (500 ميجابايت) لمنع استنزاف القرص
pub const MAX_SETUP_BYTES: u64 = 500 << 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedReleaseManifest {
    pub version: String,
    #[serde(default)]
    pub url: String,
    pub sha256: String,
    /// توقيع Ed25519 بصيغة hex لرسالة `signed_message(version, sha256)`
    #[serde(default)]
    pub sig: String,
}

/// قراءة مستودع التحديث من متغير البيئة `STORMGET_UPDATE_REPO` (إن وُجد) أو استخدام المستودع الافتراضي
pub fn effective_update_repo() -> String {
    std::env::var("STORMGET_UPDATE_REPO")
        .ok()
        .map(|s| s.trim().trim_matches('/').to_string())
        .filter(|s| s.contains('/') && !s.contains(".."))
        .unwrap_or_else(|| UPDATE_REPO.to_string())
}

pub fn version_manifest_url() -> String {
    let repo = effective_update_repo();
    format!("https://github.com/{repo}/releases/latest/download/version.json")
}

pub fn sha256sums_manifest_urls() -> [String; 2] {
    let repo = effective_update_repo();
    [
        format!("https://github.com/{repo}/releases/latest/download/SHA256SUMS"),
        format!("https://github.com/{repo}/releases/latest/download/SHA2-256SUMS"),
    ]
}

/// التحقق من مطابقة بصمة `SHA-256` للمثبت داخل ملف `SHA256SUMS` أو `SHA2-256SUMS` القياسي
pub fn verify_sha256sums_entry(sums_text: &str, filename: &str, expected_sha256: &str) -> bool {
    for line in sums_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(hash_hex) = parts.next() else {
            continue;
        };
        let Some(raw_name) = parts.next() else {
            continue;
        };
        let clean_name = raw_name.trim_start_matches('*').trim();
        if (clean_name.eq_ignore_ascii_case(filename) || clean_name.ends_with(filename))
            && hash_hex.eq_ignore_ascii_case(expected_sha256.trim())
        {
            return true;
        }
    }
    false
}

pub fn current_app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// النص القانوني الذي يغطيه توقيع Ed25519
pub fn signed_message(version: &str, sha256: &str) -> String {
    format!(
        "stormget-update\n{}\n{}\n",
        version.trim(),
        sha256.trim().to_ascii_lowercase()
    )
}

pub fn decode_hex_bytes(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// التحقق من صحة توقيع Ed25519 للإصدار باستخدام مكتبة `ring`
pub fn signature_ok(release: &SignedReleaseManifest, public_hex: &str) -> bool {
    use ring::signature::{UnparsedPublicKey, ED25519};
    let (Some(key), Some(sig)) = (decode_hex_bytes(public_hex), decode_hex_bytes(&release.sig))
    else {
        return false;
    };
    if key.len() != 32 || sig.len() != 64 {
        return false;
    }
    let msg = signed_message(&release.version, &release.sha256);
    if UnparsedPublicKey::new(&ED25519, &key)
        .verify(msg.as_bytes(), &sig)
        .is_ok()
    {
        return true;
    }
    // توافق إضافي مع بادئة ezoo-download-update عند اختبار المفاتيح المشتركة
    let alt_msg = format!(
        "ezoo-download-update\n{}\n{}\n",
        release.version.trim(),
        release.sha256.trim().to_ascii_lowercase()
    );
    UnparsedPublicKey::new(&ED25519, &key)
        .verify(alt_msg.as_bytes(), &sig)
        .is_ok()
}

impl SignedReleaseManifest {
    /// تثبيت نطاق التنزيل حصرياً على مستودعنا الرسمي في GitHub لمنع توجيه المثبت لخادم خارجي
    pub fn setup_url(&self) -> String {
        let repo = effective_update_repo();
        let own_prefix = format!("https://github.com/{repo}/");
        if self.url.starts_with(&own_prefix) {
            self.url.clone()
        } else {
            let clean_ver = self.version.trim().trim_start_matches(['v', 'V']);
            format!("https://github.com/{repo}/releases/download/v{clean_ver}/{SETUP_FILENAME}")
        }
    }
}

fn parse_version_numbers(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split(['-', '+'])
        .next()
        .unwrap_or("")
        .split('.')
        .map(|p| p.trim().parse().unwrap_or(0))
        .collect()
}

/// مقارنة دلالية لأرقام الإصدارات (`"1.0.10" > "1.0.9"`)
pub fn is_newer_version(remote: &str, local: &str) -> bool {
    let (mut a, mut b) = (parse_version_numbers(remote), parse_version_numbers(local));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

pub fn parse_manifest(text: &str) -> Result<SignedReleaseManifest> {
    let r: SignedReleaseManifest = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
    if r.sha256.len() != 64 || !r.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("ملف version.json لا يحتوي على بصمة SHA-256 صالحة");
    }
    if parse_version_numbers(&r.version).iter().all(|&n| n == 0) {
        bail!("ملف version.json لا يحتوي على رقم إصدار صالح");
    }
    Ok(r)
}

pub fn parse_signed_manifest(text: &str) -> Result<SignedReleaseManifest> {
    let r = parse_manifest(text)?;
    if !signature_ok(&r, UPDATE_PUBLIC_KEY) {
        bail!("التحديث غير موقّع رقمياً بالمفتاح الرسمي لـ StormGet؛ تم رفض التثبيت لحمايتك");
    }
    Ok(r)
}

pub async fn check_for_signed_update(client: &Client) -> Result<Option<SignedReleaseManifest>> {
    let text = client
        .get(version_manifest_url())
        .header(
            reqwest::header::USER_AGENT,
            crate::core::tools::DEFAULT_TOOL_USER_AGENT,
        )
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let r: SignedReleaseManifest = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
    if !is_newer_version(&r.version, current_app_version()) {
        return Ok(None);
    }
    let signed = parse_signed_manifest(&text)?;
    let clean_ver = signed.version.trim().trim_start_matches(['v', 'V']);
    let expected_installer = format!("StormGet-Setup-{clean_ver}.exe");
    for sums_url in sha256sums_manifest_urls() {
        if let Ok(resp) = client
            .get(&sums_url)
            .header(
                reqwest::header::USER_AGENT,
                crate::core::tools::DEFAULT_TOOL_USER_AGENT,
            )
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(sums_body) = resp.text().await {
                    if !sums_body.trim().is_empty()
                        && !verify_sha256sums_entry(&sums_body, &expected_installer, &signed.sha256)
                    {
                        bail!("تعارض أمني: بصمة SHA-256 في SHA256SUMS لا تطابق version.json الموقّع");
                    }
                    break;
                }
            }
        }
    }
    Ok(Some(signed))
}

pub fn sha256_bytes_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn compute_sync_file_sha256(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// تنزيل مثبت التحديث الموقّع والتحقق من بصمته `SHA-256` أثناء التدفق
pub async fn download_signed_installer(
    client: &Client,
    release: &SignedReleaseManifest,
    dir: &Path,
) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let clean_ver = release.version.trim().trim_start_matches(['v', 'V']);
    let dest = dir.join(format!("StormGet-Setup-{clean_ver}.exe"));
    if dest.exists() {
        if compute_sync_file_sha256(&dest)?.eq_ignore_ascii_case(&release.sha256) {
            return Ok(dest);
        }
        let _ = std::fs::remove_file(&dest);
    }

    let mut resp = client
        .get(release.setup_url())
        .header(
            reqwest::header::USER_AGENT,
            crate::core::tools::DEFAULT_TOOL_USER_AGENT,
        )
        .send()
        .await?
        .error_for_status()?;

    use std::io::Write;
    let tmp = dest.with_extension("download");
    let mut file = std::fs::File::create(&tmp)?;
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;

    while let Some(chunk) = resp.chunk().await? {
        total += chunk.len() as u64;
        if total > MAX_SETUP_BYTES {
            drop(file);
            let _ = std::fs::remove_file(&tmp);
            bail!("حجم ملف التحديث يتجاوز الحد الأقصى المسموح (500 MB)");
        }
        hasher.update(&chunk);
        file.write_all(&chunk)?;
    }
    file.flush()?;
    drop(file);

    let got: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if !got.eq_ignore_ascii_case(&release.sha256) {
        let _ = std::fs::remove_file(&tmp);
        bail!("بصمة المثبت المحمّل ({got}) لا تطابق البصمة الموقعة في version.json");
    }
    std::fs::rename(&tmp, &dest)?;
    Ok(dest)
}

/// إعادة التحقق من بصمة `SHA-256` مباشرة قبل تشغيل المثبت (لمنع هجمات TOCTOU على القرص)
pub fn run_verified_installer(setup: &Path, expected_sha256: &str, visible: bool) -> Result<()> {
    if !compute_sync_file_sha256(setup)?.eq_ignore_ascii_case(expected_sha256) {
        let _ = std::fs::remove_file(setup);
        bail!("تغير ملف التحديث على القرص قبل تشغيله؛ تم حذفه وإلغاء التثبيت فوراً");
    }
    let mut cmd = std::process::Command::new(setup);
    cmd.arg(if visible { "/SILENT" } else { "/VERYSILENT" });
    cmd.args([
        "/SUPPRESSMSGBOXES",
        "/NORESTART",
        "/CLOSEAPPLICATIONS",
        "/AUTOUPDATE=1",
    ]);
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        cmd.arg(format!("/DIR={}", dir.display()));
    }
    cmd.spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_signed_pair(version: &str, sha: &str) -> (String, String) {
        use ring::signature::{Ed25519KeyPair, KeyPair};
        let kp = Ed25519KeyPair::from_seed_unchecked(&[7u8; 32]).expect("verified in test");
        let public: String = kp
            .public_key()
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let sig: String = kp
            .sign(signed_message(version, sha).as_bytes())
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        (public, sig)
    }

    #[test]
    fn semantic_version_comparison_works() {
        assert!(is_newer_version("2.3.1", "2.3.0"));
        assert!(is_newer_version("v2.4", "2.3.9"));
        assert!(is_newer_version("2.3.10", "2.3.9"));
        assert!(!is_newer_version("2.3.0", "2.3.0"));
        assert!(!is_newer_version("2.2.9", "2.3.0"));
    }

    #[test]
    fn ed25519_signature_verification_and_tamper_rejection() {
        let sha = sha256_bytes_hex(b"stormget-installer-binary");
        let (pub_hex, sig_hex) = make_signed_pair("2.4.0", &sha);
        let good = SignedReleaseManifest {
            version: "2.4.0".into(),
            url: "https://evil.example.com/setup.exe".into(),
            sha256: sha.clone(),
            sig: sig_hex,
        };
        assert!(signature_ok(&good, &pub_hex));
        // Pinning URL to official repo even if manifest had an external URL
        assert!(good.setup_url().starts_with("https://github.com/StormGet/StormGet/"));

        // Tampered version or hash must fail verification
        assert!(!signature_ok(
            &SignedReleaseManifest {
                version: "2.4.1".into(),
                ..good.clone()
            },
            &pub_hex
        ));
        assert!(!signature_ok(
            &SignedReleaseManifest {
                sha256: sha256_bytes_hex(b"tampered"),
                ..good.clone()
            },
            &pub_hex
        ));
        assert!(!signature_ok(&good, UPDATE_PUBLIC_KEY));
    }

    #[test]
    fn installer_rechecked_before_execution_to_prevent_toctou() {
        let dir = std::env::temp_dir().join(format!("stormget-su-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("verified in test");
        let f = dir.join("setup.exe");
        std::fs::write(&f, b"tampered_on_disk").expect("verified in test");
        let err = run_verified_installer(&f, &sha256_bytes_hex(b"original"), false).unwrap_err();
        assert!(err.to_string().contains("تغير ملف التحديث"));
        assert!(!f.exists(), "Tampered installer must be deleted immediately");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
