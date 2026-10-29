//! بروتوكول Chrome Native Messaging Host القياسي (`4-Byte Length-Prefixed Stdio`)،
//! مصادقة الجلسة المشفرة (`bridge.json` + `constant_time_token_eq` + التشغيل التلقائي لسطح المكتب)،
//! والاكتشاف والربط التلقائي لإضافات المتصفح (`Chrome`, `Edge`, `Brave`, `Firefox`) في النظام والسجل (`Registry`).
//! (مستلهم من `awesome-downloader/core/bridge.mjs`, `integration/native-host.cjs`, و `desktop/browser-registration.cjs`)

use crate::{
    core::{
        storage::{default_download_directory, MAX_NATIVE_MSG_BYTES},
        tools::hide_console_window,
    },
    media::ytdlp::SabrSessionPayload,
};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use url::Url;
use uuid::Uuid;

pub const NATIVE_HOST_IDENTIFIER: &str = "com.rustidm.native_host";
pub const DEFAULT_BRIDGE_PORT: u16 = 16888;

// ============================================================================
// 1. رسائل وبروتوكول Chrome Native Messaging (4-Byte Little-Endian Framing)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeIncomingMessage {
    pub action: String, // "PING" | "INTERCEPT_DOWNLOAD" | "VIDEO_STREAM" | "PROBE_FORMATS"
    pub url: Option<String>,
    #[serde(default)]
    pub audio_url: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub format_id: Option<String>,
    #[serde(default)]
    pub sabr_session: Option<SabrSessionPayload>,
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

/// التحقق الأمني من رسالة Chrome ومنع تمرير روابط غير HTTP/HTTPS أو بيانات اعتماد مضمنة في الرابط
pub fn validate_native_incoming(msg: &NativeIncomingMessage) -> Result<()> {
    match msg.action.as_str() {
        "PING" => Ok(()),
        "INTERCEPT_DOWNLOAD" | "VIDEO_STREAM" | "PROBE_FORMATS" => {
            let raw_url = msg
                .url
                .as_ref()
                .ok_or_else(|| anyhow!("الرسالة تفتقر إلى حقل الرابط url"))?;
            if raw_url.len() > 16_384 {
                bail!("يتجاوز طول الرابط الحد الأقصى المسموح (16384 حرفاً)");
            }
            let parsed = Url::parse(raw_url).context("صيغة الرابط غير صالحة")?;
            if parsed.scheme() != "http" && parsed.scheme() != "https" {
                return Err(anyhow!(
                    "بروتوكول الرابط غير مسموح به عبر Native Messaging: {}",
                    parsed.scheme()
                ));
            }
            if !parsed.username().is_empty() || parsed.password().is_some() {
                bail!("يرفض المحرك الروابط التي تتضمن اسم مستخدم أو كلمة مرور مباشرة؛ استخدم الترويسات");
            }
            if let Some(ref audio_u) = msg.audio_url {
                let parsed_a = Url::parse(audio_u).context("صيغة رابط الصوت المرافق غير صالحة")?;
                if parsed_a.scheme() != "http" && parsed_a.scheme() != "https" {
                    bail!("بروتوكول رابط الصوت غير مسموح");
                }
            }
            Ok(())
        }
        other => Err(anyhow!("إجراء غير معروف في Native Messaging: {}", other)),
    }
}

/// توليد ملف تعريف المضيف الأصلي لمتصفح Google Chrome (`com.rustidm.native_host.json`)
pub fn generate_native_host_manifest(binary_path: &Path, extension_id: &str) -> serde_json::Value {
    generate_native_host_manifest_multi(binary_path, &[extension_id.to_string()], false)
}

/// توليد ملف تعريف المضيف الأصلي مع دعم تعدد معرّفات الإضافة (تعدد ملفات تعريف المتصفح) أو Firefox
pub fn generate_native_host_manifest_multi(
    binary_path: &Path,
    extension_ids: &[String],
    is_firefox: bool,
) -> serde_json::Value {
    if is_firefox {
        let exts: Vec<String> = if extension_ids.is_empty() {
            vec!["extension@rustidm.local".to_string()]
        } else {
            extension_ids.to_vec()
        };
        serde_json::json!({
            "name": NATIVE_HOST_IDENTIFIER,
            "description": "StormGet (RustIDM) Native Messaging Host",
            "path": binary_path.to_string_lossy(),
            "type": "stdio",
            "allowed_extensions": exts
        })
    } else {
        let origins: Vec<String> = extension_ids
            .iter()
            .map(|id| format!("chrome-extension://{}/", id.trim_matches('/')))
            .collect();
        serde_json::json!({
            "name": NATIVE_HOST_IDENTIFIER,
            "description": "StormGet (RustIDM) Native Messaging Host",
            "path": binary_path.to_string_lossy(),
            "type": "stdio",
            "allowed_origins": origins
        })
    }
}

// ============================================================================
// 2. مصادقة الجسر المحلي المشفرة (Cryptographic Session Token `bridge.json` & Constant-Time Eq)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeSessionConfig {
    pub port: u16,
    pub token: String,
    #[serde(default)]
    pub pid: u32,
    #[serde(default)]
    pub executable: Option<String>,
}

/// توليد رمز جلسة عشوائي بطول 256-bit (64 محرف ست عشري) عند إقلاع المحرك (على غرار `core/bridge.mjs`)
pub fn generate_bridge_token() -> String {
    let u1 = Uuid::new_v4();
    let u2 = Uuid::new_v4();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();

    let mut hasher = Sha256::new();
    hasher.update(u1.as_bytes());
    hasher.update(u2.as_bytes());
    hasher.update(nanos.to_le_bytes());
    hasher.update(pid.to_le_bytes());
    hex::encode(hasher.finalize())
}

/// مقارنة زمنية ثابتة (Constant-Time Token Comparison) لمنع هجمات التوقيت (توازي `crypto.timingSafeEqual` في `core/bridge.mjs`)
pub fn constant_time_token_eq(provided: &str, expected: &str) -> bool {
    let a = provided.as_bytes();
    let b = expected.as_bytes();
    if a.len() != b.len() || a.is_empty() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// مسارات حفظ ملف `bridge.json` المشترك بين محرك سطح المكتب و `NativeHost` وعمال الواجهة
pub fn candidate_bridge_config_paths(data_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(dir) = data_dir {
        paths.push(dir.join("bridge.json"));
    }
    let def_dir = default_download_directory();
    let def_bridge = def_dir.join("bridge.json");
    if !paths.contains(&def_bridge) {
        paths.push(def_bridge);
    }
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let home_bridge = PathBuf::from(home).join(".stormget").join("bridge.json");
        if !paths.contains(&home_bridge) {
            paths.push(home_bridge);
        }
    }
    paths
}

fn write_restricted_json_file(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content)?;
        f.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&tmp) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = fs::set_permissions(&tmp, perms);
        }
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

/// كتابة ملف `bridge.json` بصلاحيات مقيدة (`0o600`) عند إقلاع الخادم المحلي
pub fn write_bridge_config(data_dir: &Path, port: u16, token: &str) -> Result<PathBuf> {
    let exe_str = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().to_string());
    let cfg = BridgeSessionConfig {
        port,
        token: token.to_string(),
        pid: std::process::id(),
        executable: exe_str,
    };
    let bytes = serde_json::to_vec_pretty(&cfg)?;

    let primary_path = data_dir.join("bridge.json");
    write_restricted_json_file(&primary_path, &bytes)?;

    for extra_path in candidate_bridge_config_paths(Some(data_dir))
        .into_iter()
        .skip(1)
    {
        let _ = write_restricted_json_file(&extra_path, &bytes);
    }

    Ok(primary_path)
}

/// قراءة والتحقق من صلاحية ملف `bridge.json` (توازي `readBridge` في `integration/native-host.cjs`)
pub fn read_bridge_config(data_dir: Option<&Path>) -> Result<BridgeSessionConfig> {
    for path in candidate_bridge_config_paths(data_dir) {
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(cfg) = serde_json::from_str::<BridgeSessionConfig>(&raw) else {
            continue;
        };
        if cfg.port < 1024
            || cfg.token.len() < 20
            || cfg.token.len() > 256
            || !cfg
                .token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            continue;
        }
        return Ok(cfg);
    }
    Err(anyhow!(
        "ملف إعدادات الجسر المحلي (bridge.json) غير موجود أو غير صالح"
    ))
}

/// تشغيل محرك StormGet الخلفي تلقائياً عند استدعاء `NativeHost` والبرنامج مغلق، مع الانتظار حتى الجاهزية (مستلهم من `native-host.cjs:145-158`)
pub async fn ensure_desktop_daemon_or_autolaunch(
    client: &reqwest::Client,
    data_dir: &Path,
) -> Result<BridgeSessionConfig> {
    if let Ok(cfg) = read_bridge_config(Some(data_dir)) {
        let health_url = format!("http://127.0.0.1:{}/api/v1/health", cfg.port);
        if let Ok(resp) = client
            .get(&health_url)
            .header("Authorization", format!("Bearer {}", cfg.token))
            .header("X-RustIDM", "1")
            .timeout(Duration::from_millis(800))
            .send()
            .await
        {
            if resp.status().is_success() {
                return Ok(cfg);
            }
        }
    }

    // المحرك غير نشط: نقوم بتشغيل العملية الخلفية (daemon) تلقائياً
    let exe_path = read_bridge_config(Some(data_dir))
        .ok()
        .and_then(|c| c.executable.map(PathBuf::from))
        .filter(|p| p.is_absolute() && p.exists())
        .or_else(|| std::env::current_exe().ok())
        .ok_or_else(|| anyhow!("تعذر تحديد المسار التنفيذي لتشغيل محرك StormGet تلقائياً"))?;

    let mut cmd = Command::new(&exe_path);
    cmd.args([
        "daemon",
        "--port",
        &DEFAULT_BRIDGE_PORT.to_string(),
        "--download-dir",
        &data_dir.to_string_lossy(),
    ]);
    hide_console_window(&mut cmd);
    let _ = cmd.spawn();

    // انتظار حتى 10 ثوانٍ (40 * 250ms) ريثما يبدأ الخادم ويكتب bridge.json
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if let Ok(cfg) = read_bridge_config(Some(data_dir)) {
            let health_url = format!("http://127.0.0.1:{}/api/v1/health", cfg.port);
            if let Ok(resp) = client
                .get(&health_url)
                .header("Authorization", format!("Bearer {}", cfg.token))
                .header("X-RustIDM", "1")
                .timeout(Duration::from_millis(800))
                .send()
                .await
            {
                if resp.status().is_success() {
                    return Ok(cfg);
                }
            }
        }
    }

    Err(anyhow!(
        "تعذر تشغيل محرك StormGet الخلفي تلقائياً؛ يرجى فتح البرنامج مرة واحدة ثم المحاولة مجدداً"
    ))
}



// ============================================================================
// 3. الاكتشاف والربط التلقائي لإضافات المتصفح (Auto Browser Discovery & Registry Registration)
//    مستلهم من `awesome-downloader/desktop/browser-registration.cjs`
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserIntegrationEntry {
    pub browser: String,
    pub connected: bool,
    pub extension_ids: Vec<String>,
    pub manifest_path: String,
    pub registry_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserIntegrationReport {
    pub host_name: String,
    pub bridge_port: u16,
    pub extension_directory: Option<String>,
    pub connected_browsers: Vec<BrowserIntegrationEntry>,
    pub errors: Vec<String>,
}

/// التحقق من صحة معرّف إضافة Chromium (`32 حرفاً بين a و p`: `^[a-p]{32}$`)
pub fn is_valid_chromium_extension_id(id: &str) -> bool {
    let trimmed = id.trim();
    trimmed.len() == 32 && trimmed.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// التحقق من صحة معرّف إضافة Firefox (`^[A-Za-z0-9@._{}-]{1,128}$`)
pub fn is_valid_firefox_extension_id(id: &str) -> bool {
    let trimmed = id.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 128
        && trimmed.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '@' | '.' | '_' | '{' | '}' | '-')
        })
}

/// قراءة تسجيلات الإضافات فقط من محتوى ملف `Preferences` أو `Secure Preferences` لمتصفحات Chromium
/// دون المساس بسجل التصفح أو الكوكيز، واكتشاف معرّفات إضافة StormGet تلقائياً!
pub fn discover_extension_ids_in_preferences(
    preferences_json: &str,
    expected_extension_dir: Option<&Path>,
) -> Vec<String> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(preferences_json) else {
        return Vec::new();
    };
    let Some(settings) = root
        .get("extensions")
        .and_then(|e| e.get("settings"))
        .and_then(|s| s.as_object())
    else {
        return Vec::new();
    };

    let expected_canon = expected_extension_dir
        .and_then(|p| fs::canonicalize(p).ok())
        .map(|p| p.to_string_lossy().to_ascii_lowercase());

    let mut matched_ids = Vec::new();

    for (ext_id, entry) in settings {
        if !is_valid_chromium_extension_id(ext_id) {
            continue;
        }
        let mut is_our_extension = false;

        // 1. فحص تطابق اسم الإضافة أو وصفها في manifest المضمن داخل Preferences
        if let Some(manifest) = entry.get("manifest") {
            let name = manifest
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if name.contains("StormGet") || name.contains("RustIDM") {
                is_our_extension = true;
            }
        }

        // 2. فحص مسار الإضافة على القرص (للإضافات المحملة عبر Load unpacked)
        if !is_our_extension {
            if let Some(path_str) = entry.get("path").and_then(|v| v.as_str()) {
                let entry_path = Path::new(path_str);
                if let Some(ref exp) = expected_canon {
                    if let Ok(installed_canon) = fs::canonicalize(entry_path) {
                        if installed_canon.to_string_lossy().to_ascii_lowercase() == *exp {
                            is_our_extension = true;
                        }
                    }
                }
                if !is_our_extension && entry_path.is_absolute() {
                    if let Ok(bg_code) = fs::read_to_string(entry_path.join("background.js")) {
                        if bg_code.contains(NATIVE_HOST_IDENTIFIER)
                            || bg_code.contains("StormGet")
                            || bg_code.contains("X-RustIDM")
                        {
                            is_our_extension = true;
                        }
                    }
                }
            }
        }

        if is_our_extension && !matched_ids.contains(ext_id) {
            matched_ids.push(ext_id.clone());
        }
    }

    matched_ids
}

/// تسجيل متصفح محدد (`Chrome`, `Edge`, `Brave`, `Firefox`) مع أخذ نسخ احتياطية للملف والسجل قبل التعديل
/// (مطابق لسلوك `registerBrowser` في `awesome-downloader/desktop/browser-registration.cjs`)
pub fn register_browser_native_host(
    browser: &str,
    extension_ids: &[String],
    data_dir: &Path,
    binary_path: &Path,
) -> Result<BrowserIntegrationEntry> {
    let browser_norm = match browser.trim().to_ascii_lowercase().as_str() {
        "chrome" | "google chrome" => "Chrome",
        "edge" | "microsoft edge" => "Edge",
        "brave" | "brave browser" => "Brave",
        "firefox" | "mozilla firefox" => "Firefox",
        other => bail!("متصفح غير مدعوم للتسجيل: {other} (المتاح: Chrome, Edge, Brave, Firefox)"),
    };

    let is_firefox = browser_norm == "Firefox";
    for id in extension_ids {
        if is_firefox {
            if !is_valid_firefox_extension_id(id) {
                bail!("معرف إضافة Firefox غير صالح: {id}");
            }
        } else if !is_valid_chromium_extension_id(id) {
            bail!(
                "معرف إضافة {browser_norm} غير صالح ({id})؛ يجب أن يتكون من 32 حرفاً بين a و p"
            );
        }
    }

    let nm_folder = data_dir.join("NativeMessaging");
    fs::create_dir_all(&nm_folder)?;

    let manifest_path = nm_folder.join(format!("{browser_norm}.json"));
    let manifest_val = generate_native_host_manifest_multi(binary_path, extension_ids, is_firefox);
    let content = serde_json::to_string_pretty(&manifest_val)?;

    let stamp = Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
    if let Ok(prev) = fs::read_to_string(&manifest_path) {
        if prev != content {
            let bak_path = nm_folder.join(format!("{browser_norm}.json.{stamp}.bak"));
            let _ = fs::write(bak_path, &prev);
        }
    }
    fs::write(&manifest_path, &content)?;

    // تحديث ملف التوافق العام أيضاً
    let legacy_manifest_path = data_dir.join("com.rustidm.native_host.json");
    if !is_firefox {
        let _ = fs::write(&legacy_manifest_path, &content);
    }

    let reg_root = match browser_norm {
        "Chrome" => "Google\\Chrome",
        "Edge" => "Microsoft\\Edge",
        "Brave" => "BraveSoftware\\Brave-Browser",
        "Firefox" => "Mozilla",
        _ => "Google\\Chrome",
    };
    let reg_key = format!(
        "HKCU\\Software\\{reg_root}\\NativeMessagingHosts\\{NATIVE_HOST_IDENTIFIER}"
    );

    #[cfg(target_os = "windows")]
    {
        // فحص القيمة السابقة في الـ Registry وأخذ نسخة احتياطية قبل الكتابة
        let mut query_cmd = Command::new("reg.exe");
        query_cmd.args(["QUERY", &reg_key, "/ve"]);
        hide_console_window(&mut query_cmd);
        let prev_reg_out = query_cmd
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string());

        let manifest_str = manifest_path.to_string_lossy().to_string();
        let already_matches = prev_reg_out
            .as_ref()
            .map(|out| out.contains(&manifest_str))
            .unwrap_or(false);

        if let Some(ref prev_text) = prev_reg_out {
            if !already_matches {
                let reg_bak =
                    nm_folder.join(format!("{browser_norm}.{stamp}.registry-backup.txt"));
                let _ = fs::write(reg_bak, prev_text);
            }
        }

        if !already_matches {
            let mut add_cmd = Command::new("reg.exe");
            add_cmd.args([
                "ADD",
                &reg_key,
                "/ve",
                "/t",
                "REG_SZ",
                "/d",
                &manifest_str,
                "/f",
            ]);
            hide_console_window(&mut add_cmd);
            let _ = add_cmd.output();
        }
    }

    Ok(BrowserIntegrationEntry {
        browser: browser_norm.to_string(),
        connected: true,
        extension_ids: extension_ids.to_vec(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        registry_key: Some(reg_key),
    })
}

/// تحديد مسار مجلد `chrome-extension` المرفق مع المشروع أو البرنامج تلقائياً
pub fn locate_bundled_extension_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("chrome-extension"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("chrome-extension"));
            candidates.push(parent.join("browser").join("extension"));
            if let Some(grand) = parent.parent() {
                candidates.push(grand.join("chrome-extension"));
                if let Some(root) = grand.parent() {
                    candidates.push(root.join("chrome-extension"));
                }
            }
        }
    }
    candidates
        .into_iter()
        .find(|p| p.join("manifest.json").is_file() && p.join("background.js").is_file())
}

/// الفحص والربط التلقائي لجميع المتصفحات المثبتة (`Chrome`, `Edge`, `Brave`, `Firefox`)
/// عبر مسح ملفات `Preferences` و `Secure Preferences` لجميع الحسابات (`Default`, `Profile *`)
pub fn connect_installed_browsers(
    data_dir: &Path,
    binary_path: &Path,
    extension_dir: Option<&Path>,
) -> BrowserIntegrationReport {
    let resolved_ext_dir = extension_dir
        .map(Path::to_path_buf)
        .or_else(locate_bundled_extension_dir);

    let mut connected_browsers = Vec::new();
    let mut errors = Vec::new();

    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        let browser_targets = [
            ("Chrome", "Google\\Chrome\\User Data"),
            ("Edge", "Microsoft\\Edge\\User Data"),
            ("Brave", "BraveSoftware\\Brave-Browser\\User Data"),
        ];

        for (browser_name, rel_path) in browser_targets {
            let user_data_root = local_app_data.join(rel_path);
            let Ok(entries) = fs::read_dir(&user_data_root) else {
                continue;
            };

            let mut discovered_ids = HashSet::new();
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(folder_name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if folder_name != "Default" && !folder_name.starts_with("Profile ") {
                    continue;
                }

                for pref_file in ["Preferences", "Secure Preferences"] {
                    let pref_path = path.join(pref_file);
                    if let Ok(json_text) = fs::read_to_string(&pref_path) {
                        for id in discover_extension_ids_in_preferences(
                            &json_text,
                            resolved_ext_dir.as_deref(),
                        ) {
                            discovered_ids.insert(id);
                        }
                    }
                }
            }

            if !discovered_ids.is_empty() {
                let mut id_list: Vec<String> = discovered_ids.into_iter().collect();
                id_list.sort();
                match register_browser_native_host(browser_name, &id_list, data_dir, binary_path) {
                    Ok(entry) => connected_browsers.push(entry),
                    Err(e) => errors.push(format!("{browser_name}: {e}")),
                }
            }
        }
    }

    // تسجيل مضيف Firefox تلقائياً لمعرف الإضافة القياسي `extension@rustidm.local`
    if let Ok(ff_entry) = register_browser_native_host(
        "Firefox",
        &["extension@rustidm.local".to_string()],
        data_dir,
        binary_path,
    ) {
        connected_browsers.push(ff_entry);
    }

    let bridge_port = read_bridge_config(Some(data_dir))
        .map(|c| c.port)
        .unwrap_or(DEFAULT_BRIDGE_PORT);

    BrowserIntegrationReport {
        host_name: NATIVE_HOST_IDENTIFIER.to_string(),
        bridge_port,
        extension_directory: resolved_ext_dir.map(|p| p.to_string_lossy().to_string()),
        connected_browsers,
        errors,
    }
}
