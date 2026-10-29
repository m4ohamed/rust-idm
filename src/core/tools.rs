//! إدارة الأدوات الخارجية (`yt-dlp` و`ffmpeg`)، التثبيت والتحديث التلقائي الصامت من GitHub Releases،
//! الدمج الآمن لمسارات الصوت والفيديو بدون إعادة ترميز (`ffmpeg -c copy +faststart`) مع التحقق من صلاحية التدفقات الناتجة،
//! وفحص نظام الملفات والمساحة الحرة ومنع سكون النظام (`SetThreadExecutionState`).

use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime},
};

/// رابط التنزيل الرسمي لأحدث إصدار من أداة `yt-dlp.exe`
pub const YTDLP_LATEST_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";

/// ملف البصمات الرسمي المنشور مع كل إصدار من `yt-dlp` (بصيغة `sha256sum`)
pub const YTDLP_SHA256_SUMS_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS";

/// واجهة GitHub Releases الرسمية لتجميعات `FFmpeg` القياسية لنظام ويندوز (64-bit GPL)
pub const FFMPEG_RELEASE_API: &str =
    "https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/tags/latest";

pub const DEFAULT_TOOL_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) RustIDM/2.2";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhRelease {
    pub assets: Vec<GhAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsStatusReport {
    pub ytdlp_installed: bool,
    pub ytdlp_path: Option<String>,
    pub ffmpeg_installed: bool,
    pub ffmpeg_path: Option<String>,
    pub ffmpeg_version_tag: Option<String>,
    pub tools_directory: String,
}

/// إخفاء نافذة موجه الأوامر السوداء (Console Window) عند تشغيل العمليات المساعدة على ويندوز
pub fn hide_console_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// إخفاء نافذة موجه الأوامر لعمليات Tokio غير المتزامنة على ويندوز
pub fn hide_tokio_console_window(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        #[allow(unused_imports)]
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// اسم الملف التنفيذي للأداة حسب نظام التشغيل
pub fn tool_exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// اختيار أحدث إصدار مستقر 64-bit من تجميعات FFmpeg على GitHub (`ffmpeg-n7.1-latest-win64-gpl-7.1.zip`)
/// مع السقوط التلقائي إلى `ffmpeg-master-latest-win64-gpl.zip`
pub fn pick_ffmpeg_asset(assets: &[GhAsset]) -> Option<&GhAsset> {
    let re = Regex::new(r"^ffmpeg-n([0-9.]+)-latest-win64-gpl-[0-9.]+\.zip$").ok()?;
    let parse_ver = |v: &str| -> Vec<u64> {
        v.split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    };
    assets
        .iter()
        .filter_map(|a| re.captures(&a.name).map(|c| (parse_ver(&c[1]), a)))
        .max_by(|x, y| x.0.cmp(&y.0))
        .map(|(_, a)| a)
        .or_else(|| {
            assets
                .iter()
                .find(|a| a.name == "ffmpeg-master-latest-win64-gpl.zip")
        })
}

/// قائمة المجلدات المرشحة للبحث عن أو حفظ أدوات `yt-dlp` و`ffmpeg` بالترتيب التفضيلي
pub fn tool_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("tools"));
            dirs.push(parent.join("bin"));
            dirs.push(parent.to_path_buf());
        }
    }
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let base = PathBuf::from(&local_app_data).join("RustIDM");
        dirs.push(base.join("tools"));
        dirs.push(base.join("bin"));
    }
    if let Ok(app_data) = std::env::var("APPDATA") {
        dirs.push(PathBuf::from(&app_data).join("RustIDM").join("tools"));
    }
    dirs.push(crate::core::storage::default_download_directory().join("tools"));
    dirs
}

/// البحث عن الأداة في مجلدات الأدوات المدمجة بالبرنامج أولاً
pub fn bundled_tool(name: &str) -> Option<PathBuf> {
    let exe_name = tool_exe_name(name);
    tool_dirs()
        .into_iter()
        .map(|d| d.join(&exe_name))
        .find(|p| p.is_file())
}

/// البحث الشامل عن أداة (`ffmpeg` أو `yt-dlp`) في مجلدات البرنامج، ثم روابط WinGet، ثم مسار النظام `PATH`
pub fn find_tool(name: &str) -> Option<PathBuf> {
    if let Some(b) = bundled_tool(name) {
        return Some(b);
    }

    let exe_name = tool_exe_name(name);

    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let winget_link = PathBuf::from(&local_app_data)
            .join("Microsoft")
            .join("WinGet")
            .join("Links")
            .join(&exe_name);
        if winget_link.is_file() {
            return Some(winget_link);
        }
    }

    if let Ok(user_profile) = std::env::var("USERPROFILE") {
        let winget_user = PathBuf::from(&user_profile)
            .join("AppData")
            .join("Local")
            .join("Microsoft")
            .join("WinGet")
            .join("Links")
            .join(&exe_name);
        if winget_user.is_file() {
            return Some(winget_user);
        }
    }

    let mut probe = Command::new(&exe_name);
    if name == "ffmpeg" {
        probe.arg("-version");
    } else {
        probe.arg("--version");
    }
    hide_console_window(&mut probe);
    if let Ok(out) = probe.output() {
        if out.status.success() {
            return Some(PathBuf::from(exe_name));
        }
    }

    None
}

fn can_write_to_dir(dir: &Path) -> bool {
    let probe = dir.join(".rustidm-write-test");
    let ok = fs::write(&probe, b"ok").is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

/// تحديد المجلد القابل للكتابة لتثبيت أو تحديث الأداة
pub fn writable_tool_dir(name: &str) -> Option<PathBuf> {
    if let Some(existing) = bundled_tool(name) {
        if let Some(parent) = existing.parent() {
            if can_write_to_dir(parent) {
                return Some(parent.to_path_buf());
            }
        }
    }
    tool_dirs()
        .into_iter()
        .find(|d| fs::create_dir_all(d).is_ok() && can_write_to_dir(d))
}

fn is_update_due(stamp: &Path, interval: Duration) -> bool {
    fs::metadata(stamp)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|age| age > interval)
        .unwrap_or(true)
}

/// استبدال ملف تنفيذي بأمان حتى لو كان قيد التشغيل حالياً (ويندوز يسمح بإعادة تسمية ملف يعمل)
pub fn replace_executable_safely(new_file: &Path, target: &Path) -> Result<()> {
    if target.exists() {
        let old_backup = target.with_extension("old");
        let _ = fs::remove_file(&old_backup);
        if fs::remove_file(target).is_err() {
            fs::rename(target, &old_backup)?;
        }
    }
    fs::rename(new_file, target)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(target) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(target, perms);
        }
    }
    Ok(())
}

/// البحث العودي عن ملف بالاسم داخل مجلد مستخرج
pub fn find_file_recursive(dir: &Path, target_name: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file_recursive(&path, target_name) {
                return Some(found);
            }
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case(target_name))
            .unwrap_or(false)
        {
            return Some(path);
        }
    }
    None
}

/// الحد الأقصى لحجم ملف البصمات (SHA2-256SUMS / checksums.sha256) المقبول عند الجلب.
const MAX_CHECKSUM_FILE_BYTES: usize = 2 * 1024 * 1024;

/// استخراج بصمة SHA-256 المتوقعة لملف محدد من نص بصيغة `sha256sum` القياسية:
/// `<64 hex>␣␣<name>` أو `<64 hex>␣*<name>` (وضع ثنائي) أو مع بادئة `./`.
/// يُرجع البصمة بأحرف صغيرة، أو `None` إن لم يوجد الملف أو كانت البصمة غير صالحة.
pub fn parse_sha256_sums(text: &str, asset_name: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        let mut parts = line.split_whitespace();
        let (Some(hash), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        let name = name.trim_start_matches('*').trim_start_matches("./");
        if name == asset_name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

/// مقارنة بصمتين سداسيتين دون حساسية لحالة الأحرف.
pub fn sha256_hex_matches(actual: &str, expected: &str) -> bool {
    actual.len() == 64 && actual.eq_ignore_ascii_case(expected)
}

/// جلب ملف البصمات الرسمي للإصدار واستخراج بصمة الأصل المطلوب. الفشل هنا يمنع التثبيت
/// (سياسة الفشل المغلق): لا تُشغَّل أداة تنفيذية لم يُتحقق من بصمتها.
async fn fetch_expected_sha256(client: &Client, sums_url: &str, asset_name: &str) -> Result<String> {
    if !sums_url.starts_with("https://github.com/") {
        bail!("رابط ملف البصمات غير موثوق: {sums_url}");
    }
    let bytes = client
        .get(sums_url)
        .header(reqwest::header::USER_AGENT, DEFAULT_TOOL_USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    if bytes.len() > MAX_CHECKSUM_FILE_BYTES {
        bail!("ملف البصمات أكبر من الحد المسموح ({} بايت)", bytes.len());
    }
    let text = String::from_utf8_lossy(&bytes);
    parse_sha256_sums(&text, asset_name)
        .ok_or_else(|| anyhow!("لم تُوجد بصمة SHA-256 صالحة للملف {asset_name} في {sums_url}"))
}

/// تنزيل أداة تنفيذية مع التحقق من بصمة SHA-256 أثناء التدفق؛ عند عدم التطابق يُحذف الملف
/// المؤقت ولا يُستبدل الملف الهدف.
async fn download_tool_binary(
    client: &Client,
    url: &str,
    dest: &Path,
    expected_sha256: &str,
) -> Result<()> {
    let mut resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, DEFAULT_TOOL_USER_AGENT)
        .send()
        .await?
        .error_for_status()?;

    let tmp_path = dest.with_extension("download");
    let mut file = fs::File::create(&tmp_path)?;
    let mut hasher = Sha256::new();
    let mut total_bytes = 0usize;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk)?;
        hasher.update(&chunk);
        total_bytes += chunk.len();
    }
    file.sync_all()?;
    drop(file);

    if total_bytes < 1024 {
        let _ = fs::remove_file(&tmp_path);
        bail!("حجم الملف المنزل من {url} صغير جداً وغير صالح ({total_bytes} bytes)");
    }

    let actual = hex::encode(hasher.finalize());
    if !sha256_hex_matches(&actual, expected_sha256) {
        let _ = fs::remove_file(&tmp_path);
        bail!("فشل التحقق من بصمة SHA-256 للملف المنزل من {url}: المتوقع {expected_sha256} والفعلي {actual}");
    }

    replace_executable_safely(&tmp_path, dest)
}

/// التحقق من وجود `yt-dlp` وتنزيله تلقائياً إذا كان مفقوداً، أو تحديثه دورياً عبر `yt-dlp -U`
pub async fn ensure_or_update_ytdlp(
    client: &Client,
    data_dir: &Path,
    force_check: bool,
) -> Result<Option<PathBuf>> {
    let _ = fs::create_dir_all(data_dir);
    if find_tool("yt-dlp").is_none() {
        if let Some(dir) = writable_tool_dir("yt-dlp") {
            let dest = dir.join(tool_exe_name("yt-dlp"));
            let url = if cfg!(windows) {
                YTDLP_LATEST_URL
            } else {
                "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp"
            };
            let asset_name = if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" };
            let expected = fetch_expected_sha256(client, YTDLP_SHA256_SUMS_URL, asset_name).await?;
            download_tool_binary(client, url, &dest, &expected).await?;
            let _ = fs::write(data_dir.join("yt-dlp-updated"), b"installed");
            return Ok(Some(dest));
        }
        return Ok(None);
    }

    let stamp = data_dir.join("yt-dlp-updated");
    if !force_check && !is_update_due(&stamp, Duration::from_secs(20 * 3600)) {
        return Ok(find_tool("yt-dlp"));
    }

    if let Some(tool_path) = bundled_tool("yt-dlp").or_else(|| find_tool("yt-dlp")) {
        let tool_clone = tool_path.clone();
        let updated = tokio::task::spawn_blocking(move || {
            let mut cmd = Command::new(&tool_clone);
            cmd.arg("-U");
            hide_console_window(&mut cmd);
            cmd.output().map(|o| o.status.success()).unwrap_or(false)
        })
        .await
        .unwrap_or(false);

        if updated {
            let _ = fs::write(&stamp, b"updated");
        }
        return Ok(Some(tool_path));
    }

    Ok(None)
}

/// التحقق من وجود `ffmpeg` وتنزيل أحدث إصدار مستقر من GitHub Releases تلقائياً إذا كان مفقوداً أو قديماً
pub async fn ensure_or_update_ffmpeg(
    client: &Client,
    data_dir: &Path,
    force_check: bool,
) -> Result<Option<PathBuf>> {
    let _ = fs::create_dir_all(data_dir);
    let existing = find_tool("ffmpeg");
    let stamp = data_dir.join("ffmpeg-checked");

    if existing.is_some() && !force_check && !is_update_due(&stamp, Duration::from_secs(20 * 3600)) {
        return Ok(existing);
    }

    if !cfg!(windows) {
        return Ok(existing);
    }

    let text = client
        .get(FFMPEG_RELEASE_API)
        .header(reqwest::header::USER_AGENT, DEFAULT_TOOL_USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let release: GhRelease = serde_json::from_str(&text)?;
    let Some(asset) = pick_ffmpeg_asset(&release.assets) else {
        bail!("لم يتم العثور على حزمة FFmpeg صالحة لنظام ويندوز في الإصدار الأخير");
    };

    let latest_tag = format!("{} {}", asset.name, asset.updated_at);
    let zip_url = asset.browser_download_url.clone();
    if !zip_url.starts_with("https://github.com/") {
        bail!("رابط تنزيل FFmpeg غير موثوق: {zip_url}");
    }

    let Some(sums_asset) = release.assets.iter().find(|a| a.name == "checksums.sha256") else {
        bail!("لا يحتوي إصدار FFmpeg على ملف البصمات checksums.sha256؛ أُلغي التثبيت لعدم إمكان التحقق");
    };
    let expected_zip_sha256 =
        fetch_expected_sha256(client, &sums_asset.browser_download_url, &asset.name).await?;

    let Some(dir) = writable_tool_dir("ffmpeg") else {
        return Ok(existing);
    };
    let version_file = dir.join("ffmpeg-version.txt");
    let current_tag = fs::read_to_string(&version_file).unwrap_or_default();

    if existing.is_some() && current_tag.trim() == latest_tag.trim() {
        let _ = fs::write(&stamp, b"up-to-date");
        return Ok(existing);
    }

    let work_dir = std::env::temp_dir().join(format!("rustidm-ffmpeg-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work_dir);
    fs::create_dir_all(&work_dir)?;
    let zip_path = work_dir.join("ffmpeg.zip");

    download_tool_binary(client, &zip_url, &zip_path, &expected_zip_sha256).await?;

    let (zip_clone, work_clone) = (zip_path.clone(), work_dir.clone());
    let unpacked = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("tar");
        cmd.arg("-xf").arg(&zip_clone).arg("-C").arg(&work_clone);
        hide_console_window(&mut cmd);
        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    })
    .await
    .unwrap_or(false);

    let exe_name = tool_exe_name("ffmpeg");
    let found_exe = unpacked.then(|| find_file_recursive(&work_dir, &exe_name)).flatten();

    let result = match found_exe {
        Some(extracted_ffmpeg) => {
            let staged = dir.join("ffmpeg.new");
            fs::copy(&extracted_ffmpeg, &staged)?;
            let final_dest = dir.join(&exe_name);
            replace_executable_safely(&staged, &final_dest)?;
            let _ = fs::write(&version_file, &latest_tag);
            let _ = fs::write(&stamp, b"updated");
            Ok(Some(final_dest))
        }
        None => Err(anyhow!("تعذر استخراج ffmpeg.exe من الأرشيف المنزل")),
    };

    let _ = fs::remove_dir_all(&work_dir);
    result
}

/// فحص وتحديث كلتا الأداتين (`yt-dlp` و`ffmpeg`) وتنظيف النسخ القديمة `.old`
pub async fn update_all_tools(client: Client, data_dir: PathBuf, force_check: bool) -> ToolsStatusReport {
    let _ = ensure_or_update_ytdlp(&client, &data_dir, force_check).await;
    let _ = ensure_or_update_ffmpeg(&client, &data_dir, force_check).await;

    for dir in tool_dirs() {
        for name in ["yt-dlp", "ffmpeg"] {
            let _ = fs::remove_file(dir.join(tool_exe_name(name)).with_extension("old"));
        }
    }

    get_tools_status(&data_dir)
}

/// قراءة حالة تثبيت الأدوات الحالية ومساراتها الفعلية
pub fn get_tools_status(data_dir: &Path) -> ToolsStatusReport {
    let ytdlp = find_tool("yt-dlp");
    let ffmpeg = find_tool("ffmpeg");
    let t_dir = writable_tool_dir("ffmpeg")
        .or_else(|| writable_tool_dir("yt-dlp"))
        .unwrap_or_else(|| data_dir.join("tools"));
    let ver_tag = t_dir
        .join("ffmpeg-version.txt")
        .exists()
        .then(|| fs::read_to_string(t_dir.join("ffmpeg-version.txt")).ok())
        .flatten()
        .map(|s| s.trim().to_string());

    ToolsStatusReport {
        ytdlp_installed: ytdlp.is_some(),
        ytdlp_path: ytdlp.map(|p| p.to_string_lossy().to_string()),
        ffmpeg_installed: ffmpeg.is_some(),
        ffmpeg_path: ffmpeg.map(|p| p.to_string_lossy().to_string()),
        ffmpeg_version_tag: ver_tag,
        tools_directory: t_dir.to_string_lossy().to_string(),
    }
}

/// تشغيل المحدّث التلقائي الصامت في الخلفية عند إقلاع المحرك وكل 6 ساعات
pub fn spawn_background_tools_updater(client: Client, data_dir: PathBuf) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(12)).await;
        loop {
            let _ = update_all_tools(client.clone(), data_dir.clone(), false).await;
            tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
        }
    });
}

/// تحديد اسم حاوية الدمج (`muxer`) لـ FFmpeg بناءً على امتداد الملف النهائي
pub fn muxer_format_for_filename(filename: &str) -> &'static str {
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4")
        .to_ascii_lowercase();
    match ext.as_str() {
        "webm" => "webm",
        "mkv" => "matroska",
        "m4a" | "mp4" | "m4v" | "mov" => "mp4",
        _ => "matroska",
    }
}

/// فحص مخرجات `ffmpeg -i` للتأكد من احتواء الملف المدمج على مسار فيديو ومسار صوت صالحين
pub fn verify_probed_streams_have_video_and_audio(ffmpeg_stderr: &str) -> bool {
    let has_video = ffmpeg_stderr.contains("Video:");
    let has_audio = ffmpeg_stderr.contains("Audio:");
    has_video && has_audio
}

/// التحقق الصارم من صلاحية الملف النهائي المدمج (وجود الملف، الحجم الأدنى المتوقع، ووجود مساري الفيديو والصوت)
pub fn verify_merged_media_file(
    ffmpeg_bin: &Path,
    merged_path: &Path,
    video_input_size: u64,
    audio_input_size: u64,
) -> Result<()> {
    let meta = fs::metadata(merged_path)
        .context("الملف المدمج غير موجود على القرص بعد انتهاء FFmpeg")?;
    let merged_len = meta.len();

    let combined_inputs = video_input_size.saturating_add(audio_input_size);
    let min_expected = if combined_inputs > 4096 {
        (combined_inputs * 70) / 100
    } else {
        512
    };

    if merged_len < min_expected {
        bail!(
            "حجم الملف المدمج ({merged_len} بايت) أقل من الحد الأدنى المتوقع ({min_expected} بايت)؛ تم الإبقاء على الملفات الأصلية لمنع فقدان البيانات"
        );
    }

    let mut probe = Command::new(ffmpeg_bin);
    probe.arg("-hide_banner").arg("-i").arg(merged_path);
    hide_console_window(&mut probe);
    if let Ok(out) = probe.output() {
        let stderr_text = String::from_utf8_lossy(&out.stderr);
        if !verify_probed_streams_have_video_and_audio(&stderr_text) {
            bail!(
                "الملف المدمج يفتقر إلى أحد مساري الفيديو أو الصوت في فحص الحاوية؛ تم رفض الملف المدمج"
            );
        }
    }

    Ok(())
}

/// دمج ملف فيديو منفصل وملف صوت منفصل في ملف واحد قابل للتشغيل فوراً عبر `ffmpeg -c copy`
/// مع الكتابة إلى ملف مؤقت (`.muxing.tmp`) والتحقق الصارم من وجود مساري الفيديو والصوت وحجم الملف قبل اعتماده!
pub fn merge_video_and_audio_streams(
    video_path: &Path,
    audio_path: &Path,
    output_path: &Path,
    final_filename: &str,
) -> Result<()> {
    let Some(ffmpeg) = find_tool("ffmpeg") else {
        bail!(
            "أداة ffmpeg غير متوفرة حالياً لدمج الصوت والفيديو. يقوم RustIDM بتنزيلها تلقائياً في الخلفية، ثم اضغط استئناف."
        );
    };

    let v_size = fs::metadata(video_path)
        .context("ملف الفيديو المؤقت غير موجود للدمج")?
        .len();
    let a_size = fs::metadata(audio_path)
        .context("ملف الصوت المؤقت غير موجود للدمج")?
        .len();
    if v_size == 0 || a_size == 0 {
        bail!(
            "أحد مساري الإدخال فارغ (الفيديو: {v_size} بايت، الصوت: {a_size} بايت)؛ لا يمكن الدمج"
        );
    }

    let format = muxer_format_for_filename(final_filename);
    let staged_output = output_path.with_extension("muxing.tmp");
    let _ = fs::remove_file(&staged_output);

    let mut cmd = Command::new(&ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(video_path)
        .arg("-i")
        .arg(audio_path)
        .args(["-map", "0:v:0", "-map", "1:a:0", "-c", "copy"]);

    if format == "mp4" {
        cmd.args(["-movflags", "+faststart"]);
    }
    cmd.args(["-f", format]).arg(&staged_output);
    hide_console_window(&mut cmd);

    let out = cmd.output().context("تعذر تشغيل عملية ffmpeg لدمج المسارات")?;
    let mut mux_succeeded = out.status.success();

    if !mux_succeeded && format == "mp4" {
        // محاولة احتياطية بترميز الصوت إلى AAC إذا كان وعاء MP4 يرفض صوت Opus
        let _ = fs::remove_file(&staged_output);
        let mut retry_cmd = Command::new(&ffmpeg);
        retry_cmd
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(video_path)
            .arg("-i")
            .arg(audio_path)
            .args([
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-c:v",
                "copy",
                "-c:a",
                "aac",
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
            ])
            .arg(&staged_output);
        hide_console_window(&mut retry_cmd);
        if let Ok(retry_out) = retry_cmd.output() {
            mux_succeeded = retry_out.status.success();
        }
    }

    if !mux_succeeded {
        let _ = fs::remove_file(&staged_output);
        let err = String::from_utf8_lossy(&out.stderr);
        let last_line = err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("خطأ غير معروف أثناء الدمج");
        bail!("فشل ffmpeg في دمج الفيديو والصوت: {last_line}");
    }

    // التحقق من صلاحية وحجم ومسارات الملف المدمج قبل استبدال الملف النهائي
    if let Err(verify_err) = verify_merged_media_file(&ffmpeg, &staged_output, v_size, a_size) {
        let _ = fs::remove_file(&staged_output);
        return Err(verify_err);
    }

    if output_path.exists() {
        let _ = fs::remove_file(output_path);
    }
    fs::rename(&staged_output, output_path)
        .context("تعذر نقل الملف المدمج النهائي إلى وجهته")?;

    Ok(())
}

/// التحقق الأمني من أمر مضاد الفيروسات أو الأوامر المخصصة لمنع حقن محارف الصدفة (`Shell Injection`)
pub fn validate_safe_executable_path(cmd_path: &str) -> Result<()> {
    let trimmed = cmd_path.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let forbidden_chars = ['&', '|', ';', '`', '$', '>', '<', '\n', '\r'];
    if trimmed.chars().any(|c| forbidden_chars.contains(&c)) {
        bail!("يرفض المحرك أوامر النظام التي تحتوي على محارف ربط أو حقن أوامر الصدفة (&, |, ;, $, `)");
    }
    Ok(())
}

/// التحقق من أن قيمة قادمة من العميل (إضافة المتصفح أو واجهة محلية) رابط ويب حقيقي قبل تمريرها
/// كوسيط موضعي إلى `yt-dlp`. بدون هذا الفحص يمكن لقيمة مثل `--exec=calc.exe` أن تُفسَّر كخيار
/// وتؤدي إلى تنفيذ أوامر على الجهاز (Argument Injection).
/// يُرجع الرابط بعد تطبيعه (يبدأ دائماً بـ `http://` أو `https://`).
pub fn validate_web_url_for_tools(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("الرابط فارغ");
    }
    if trimmed.starts_with('-') {
        bail!("الرابط لا يمكن أن يبدأ بشرطة (يُفسَّر كخيار في الأداة الخارجية)");
    }
    if trimmed.chars().any(|c| c.is_control()) {
        bail!("الرابط يحتوي على أحرف تحكم غير مسموحة");
    }
    let parsed = url::Url::parse(trimmed).map_err(|e| anyhow!("صيغة الرابط غير صالحة: {e}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        bail!("بروتوكول الرابط غير مسموح: {}", parsed.scheme());
    }
    if parsed.host_str().map(str::is_empty).unwrap_or(true) {
        bail!("الرابط بلا مضيف");
    }
    Ok(parsed.to_string())
}

// ============================================================================
// إدارة الطاقة ومنع السكون وفحص نظام الملفات على ويندوز (System Power & Disk Checks)
// ============================================================================

/// منع نظام ويندوز من الدخول في وضع السكون (Sleep) أثناء وجود تحميلات نشطة،
/// وإعادة السماح بالسكون الطبيعي فور انتهاء أو إيقاف كافة التحميلات.
pub fn set_system_awake_for_downloads(has_active_downloads: bool) {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Power::{
            SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
        };
        let flags = if has_active_downloads {
            ES_CONTINUOUS | ES_SYSTEM_REQUIRED
        } else {
            ES_CONTINUOUS
        };
        SetThreadExecutionState(flags);
    }
    #[cfg(not(windows))]
    let _ = has_active_downloads;
}

/// فحص المساحة الحرة على القرص ونوع نظام الملفات (منع حفظ ملفات > 4GB على أقراص FAT32)
/// هامش الأمان الإلزامي على قرص النظام (1 جيجابايت) لمنع امتلاء قرص النظام أثناء تخزين المقاطع المؤقتة (مستوحى من dlman)
pub const SYSTEM_DISK_HEADROOM_BYTES: u64 = 1024 * 1024 * 1024;

/// اختيار مجلد التخزين المؤقت الأمثل للمقاطع (`auto`, `destination`, `appdata`) مع التحقق من وجود 1 GiB هامش أمان على قرص النظام
pub fn resolve_optimal_scratch_dir(
    mode: &str,
    destination: &Path,
    app_data_dir: &Path,
    expected_size: Option<u64>,
) -> PathBuf {
    let appdata_scratch = || app_data_dir.join("temp");
    let destination_scratch = || destination.join(".stormget-cache");

    match mode {
        "destination" => destination_scratch(),
        "appdata" => appdata_scratch(),
        _ => {
            if let Some(size) = expected_size {
                if check_disk_room_and_filesystem(
                    app_data_dir,
                    size.saturating_add(SYSTEM_DISK_HEADROOM_BYTES),
                    Some(size),
                )
                .is_err()
                {
                    return destination_scratch();
                }
            }
            appdata_scratch()
        }
    }
}

pub fn check_disk_room_and_filesystem(
    dir: &Path,
    needed_bytes: u64,
    total_size: Option<u64>,
) -> Result<()> {
    #[cfg(windows)]
    {
        let existing_dir = find_existing_parent(dir);
        if let Some(free) = win_disk_free_space(&existing_dir) {
            if needed_bytes > 0 && free < needed_bytes.saturating_add(16 * 1024 * 1024) {
                bail!(
                    "المساحة الحرة على القرص غير كافية: يحتاج الملف إلى {} بايت إضافية، بينما المتاح {} بايت فقط",
                    needed_bytes,
                    free
                );
            }
        }
        if let Some(total) = total_size {
            if total >= 4 * 1024 * 1024 * 1024 {
                if let Some(fs_name) = win_file_system_name(&existing_dir) {
                    if fs_name.eq_ignore_ascii_case("FAT32") || fs_name.eq_ignore_ascii_case("FAT")
                    {
                        bail!(
                            "القرص المحدد يستخدم نظام الملفات {fs_name} الذي لا يدعم ملفات أكبر من 4 جيجابايت. يرجى اختيار قرص بنظام NTFS أو exFAT."
                        );
                    }
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (dir, needed_bytes, total_size);
    }
    Ok(())
}

#[cfg(windows)]
fn find_existing_parent(dir: &Path) -> PathBuf {
    let mut cur = dir.to_path_buf();
    while !cur.exists() {
        match cur.parent() {
            Some(p) if p != cur => cur = p.to_path_buf(),
            _ => break,
        }
    }
    cur
}

#[cfg(windows)]
fn win_disk_free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free: u64 = 0;
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(windows)]
fn win_file_system_name(dir: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetVolumeInformationW, GetVolumePathNameW};
    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut root = [0u16; 300];
    let mut name = [0u16; 64];
    // SAFETY: `wide`, `root`, and `name` are valid stack-allocated UTF-16 buffers with explicit lengths.
    let ok = unsafe {
        if GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), root.len() as u32) == 0 {
            0
        } else {
            GetVolumeInformationW(
                root.as_ptr(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                name.as_mut_ptr(),
                name.len() as u32,
            )
        }
    };
    if ok == 0 {
        return None;
    }
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Some(String::from_utf16_lossy(&name[..len]))
}


// ============================================================================
// تكامل نظام ويندوز المتقدم: وسم الأمان (Mark of the Web :Zone.Identifier)،
// حفظ تاريخ التعديل الأصلي للملف من السيرفر، الفحص الآمن بالانتيفيروس، وتحويل MP3
// (مستلهم من ezoo-download/crates/engine/src/system.rs & mux.rs)
// ============================================================================

/// تنقية الرابط من اسم المستخدم وكلمة المرور والـ `#fragment` قبل كتابته في `:Zone.Identifier`
pub fn sanitize_zone_url(raw_url: &str) -> Option<String> {
    let mut parsed = url::Url::parse(raw_url.trim()).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    parsed.set_fragment(None);
    Some(parsed.to_string())
}

/// بناء محتوى دفق `:Zone.Identifier` القياسي لنظام ويندوز (Mark of the Web)
pub fn build_zone_identifier_content(host_url: &str, referrer: Option<&str>) -> String {
    let mut out = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
    if let Some(r) = referrer.and_then(sanitize_zone_url) {
        out.push_str(&format!("ReferrerUrl={r}\r\n"));
    }
    if let Some(h) = sanitize_zone_url(host_url) {
        out.push_str(&format!("HostUrl={h}\r\n"));
    }
    out
}

/// وسم الملف المحمّل بأنه قادم من الإنترنت (`<file>:Zone.Identifier`) على أقراص NTFS
/// لكي يقوم Windows SmartScreen و Office بفحصه وتنبيه المستخدم قبل تشغيله
pub fn mark_file_from_internet(file: &Path, host_url: &str, referrer: Option<&str>) {
    #[cfg(windows)]
    {
        let mut stream = file.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let _ = std::fs::write(
            PathBuf::from(stream),
            build_zone_identifier_content(host_url, referrer),
        );
    }
    #[cfg(not(windows))]
    {
        let _ = (file, host_url, referrer);
    }
}

/// تطبيق طابع الوقت الأصلي للملف على الخادم (`Last-Modified`) على الملف النهائي بعد اكتمال التحميل
pub fn apply_server_last_modified_time(path: &Path, last_modified_header: Option<&str>) {
    let Some(raw) = last_modified_header.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    let parsed_dt = chrono::DateTime::parse_from_rfc2822(raw)
        .or_else(|_| chrono::DateTime::parse_from_rfc3339(raw))
        .ok();
    if let Some(dt) = parsed_dt {
        let secs = dt.timestamp();
        if secs > 0 {
            let sys_time = SystemTime::UNIX_EPOCH + Duration::from_secs(secs as u64);
            if let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) {
                let _ = f.set_modified(sys_time);
            }
        }
    }
}

/// تقسيم سطر الأوامر إلى معاملات مع الحفاظ على الأجزاء المحاطة بعلامات اقتباس `"..."`
pub fn split_command_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// فحص الملف المكتمل ببرنامج الحماية من الفيروسات مع استبدال العنصر النائب `[File]`
pub fn run_virus_scanner_on_file(program: &str, args: &str, file: &Path) {
    let program = program.trim().trim_matches('"');
    if program.is_empty() || validate_safe_executable_path(program).is_err() {
        return;
    }
    let file_str = file.display().to_string();
    let effective_args = if args.contains("[File]") {
        args.replace("[File]", &format!("\"{file_str}\""))
    } else if args.trim().is_empty() {
        format!("\"{file_str}\"")
    } else {
        format!("{args} \"{file_str}\"")
    };
    let mut cmd = Command::new(program);
    cmd.args(split_command_args(&effective_args));
    hide_console_window(&mut cmd);
    let _ = cmd.spawn();
}

/// تحويل ملف صوتي أو فيديو محمّل إلى ملف `MP3` قياسي (`libmp3lame -b:a 192k -id3v2_version 3`)
#[allow(dead_code)]
pub fn convert_media_to_mp3(
    input_path: &Path,
    output_path: &Path,
    kbps: u64,
    title: Option<&str>,
) -> Result<()> {
    let Some(ffmpeg) = find_tool("ffmpeg") else {
        bail!("أداة ffmpeg غير متوفرة لتحويل الملف إلى MP3");
    };
    let mut cmd = Command::new(&ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input_path)
        .args([
            "-vn",
            "-map",
            "0:a:0",
            "-map_metadata",
            "-1",
            "-c:a",
            "libmp3lame",
            "-b:a",
        ])
        .arg(format!("{}k", kbps.max(64)))
        .args(["-id3v2_version", "3"]);

    if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
        let clean_title: String = t.chars().take(200).collect();
        cmd.arg("-metadata").arg(format!("title={clean_title}"));
    }
    cmd.args(["-f", "mp3"]).arg(output_path);
    hide_console_window(&mut cmd);

    let out = cmd.output().context("تعذر تشغيل ffmpeg لتحويل MP3")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("خطأ غير معروف");
        bail!("فشل تحويل الملف إلى MP3: {last}");
    }
    Ok(())
}

#[cfg(test)]
mod web_url_guard_tests {
    use super::validate_web_url_for_tools;

    #[test]
    fn accepts_normal_web_urls() {
        assert_eq!(
            validate_web_url_for_tools("https://www.youtube.com/watch?v=abc").expect("verified in test"),
            "https://www.youtube.com/watch?v=abc"
        );
        assert!(validate_web_url_for_tools("  http://example.com/a.mp4  ").is_ok());
    }

    #[test]
    fn output_is_always_a_normalized_web_url() {
        // مكتبة url تطبّع الشرطات الزائدة؛ المهم أن الناتج يبدأ دائماً بـ http(s)
        let out = validate_web_url_for_tools("https:///example.com/x").expect("verified in test");
        assert_eq!(out, "https://example.com/x");
        assert!(out.starts_with("https://") || out.starts_with("http://"));
    }

    #[test]
    fn rejects_option_injection() {
        for bad in [
            "--exec=calc.exe",
            "-o /tmp/x",
            "--config-locations=//evil/share/yt.conf",
            " --exec=calc youtube.com/",
            "-J",
        ] {
            assert!(validate_web_url_for_tools(bad).is_err(), "must reject: {bad}");
        }
    }

    #[test]
    fn rejects_non_web_schemes_and_junk() {
        for bad in [
            "file:///etc/passwd",
            "ftp://example.com/a",
            "javascript:alert(1)",
            "http://",
            "",
            "   ",
            "https://exa\nmple.com/",
            "not a url",
        ] {
            assert!(validate_web_url_for_tools(bad).is_err(), "must reject: {bad:?}");
        }
    }
}

#[cfg(test)]
mod tool_checksum_tests {
    use super::{parse_sha256_sums, sha256_hex_matches};

    const H1: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const H2: &str = "FEDCBA9876543210FEDCBA9876543210FEDCBA9876543210FEDCBA9876543210";

    #[test]
    fn parses_standard_sha256sum_formats() {
        let text = format!("{H1}  yt-dlp.exe\n{H2} *yt-dlp\n{H1}  ./other.zip\n");
        assert_eq!(parse_sha256_sums(&text, "yt-dlp.exe").expect("verified in test"), H1);
        assert_eq!(parse_sha256_sums(&text, "yt-dlp").expect("verified in test"), H2.to_ascii_lowercase());
        assert_eq!(parse_sha256_sums(&text, "other.zip").expect("verified in test"), H1);
    }

    #[test]
    fn rejects_missing_or_malformed_entries() {
        let text = format!("{H1}  yt-dlp.exe\nnothex0123  broken\n{}  short\n", &H1[..63]);
        assert!(parse_sha256_sums(&text, "missing.exe").is_none());
        assert!(parse_sha256_sums(&text, "broken").is_none());
        assert!(parse_sha256_sums(&text, "short").is_none());
        assert!(parse_sha256_sums("", "yt-dlp.exe").is_none());
    }

    #[test]
    fn name_must_match_exactly() {
        let text = format!("{H1}  yt-dlp.exe.sig\n");
        assert!(parse_sha256_sums(&text, "yt-dlp.exe").is_none());
    }

    #[test]
    fn hash_comparison_is_case_insensitive_and_length_checked() {
        assert!(sha256_hex_matches(H1, &H1.to_ascii_uppercase()));
        assert!(!sha256_hex_matches(H1, H2));
        assert!(!sha256_hex_matches(&H1[..63], &H1[..63]));
    }
}
