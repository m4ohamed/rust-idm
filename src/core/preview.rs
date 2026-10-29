//! المعاينة الحية أثناء التحميل ("Play while downloading")
//! مستلهم من `ezoo-download/crates/app/src/preview.rs` و `crates/engine/src/mux.rs`.
//!
//! 1. للملفات العادية قيد التحميل (`.part`): يستعلم عن المشغل الافتراضي للامتداد الحقيقي عبر `AssocQueryStringW`
//!    أو ينشئ رابطاً صلباً (`std::fs::hard_link`) بالامتداد الحقيقي (`<stem> (preview).<ext>`) ينمو مع التحميل دون نسخ البيانات!
//! 2. لتحميلات يوتيوب المقسمة (فيديو + صوت منفصلان): يولّد لقطة قابلة للتشغيل الفوري عبر بروتوكول `subfile` في `ffmpeg` بدون إعادة ترميز.

use crate::core::tools::{find_tool, hide_console_window};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static PREVIEW_LINKS: Mutex<Vec<(PathBuf, Instant)>> = Mutex::new(Vec::new());

/// بناء مسار ملف المعاينة بالامتداد الحقيقي للملف النهائي
pub fn preview_link_name(part_path: &Path, final_name: &str, ext_override: Option<&str>) -> PathBuf {
    let stem = Path::new(final_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("video");
    let ext = ext_override
        .or_else(|| Path::new(final_name).extension().and_then(|e| e.to_str()))
        .filter(|e| !e.is_empty())
        .unwrap_or("mp4");
    part_path.with_file_name(format!("{stem} (preview).{ext}"))
}

fn find_free_preview_path(first: PathBuf) -> std::io::Result<PathBuf> {
    let mut candidate = first;
    let mut n = 2u32;
    while candidate.exists() {
        let stem = candidate
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let ext = candidate
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let base = stem
            .trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ')
            .to_string();
        candidate = candidate.with_file_name(format!("{base} {n}.{ext}"));
        n += 1;
        if n > 50 {
            return Err(std::io::Error::other("تجاوز العدد الأقصى لملفات المعاينة المؤقتة"));
        }
    }
    Ok(candidate)
}

/// تشغيل الجزء المحمّل حتى الآن من ملف `.part` في مشغل الوسائط الافتراضي للمستخدم
pub fn play_partial_download(part_path: &Path, final_name: &str) -> Result<PathBuf> {
    if !part_path.exists() {
        bail!("ملف التحميل الجزئي غير موجود على القرص بعد");
    }
    let ext = Path::new(final_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4");

    #[cfg(windows)]
    if let Some(player_exe) = query_windows_player_for_ext(ext) {
        Command::new(&player_exe)
            .arg(part_path)
            .spawn()
            .context("تعذر تشغيل مشغل الوسائط")?;
        return Ok(part_path.to_path_buf());
    }

    let companion_audio = part_path.with_extension("audio.part");
    if companion_audio.exists() {
        if let Ok(v_meta) = std::fs::metadata(part_path) {
            let a_len = std::fs::metadata(&companion_audio).map(|m| m.len()).unwrap_or(0);
            let snap_mkv = find_free_preview_path(preview_link_name(part_path, final_name, Some("mkv")))?;
            if create_dual_stream_preview_snapshot(
                part_path,
                v_meta.len(),
                (a_len > 0).then_some((companion_audio.as_path(), a_len)),
                &snap_mkv,
            )
            .is_ok()
            {
                PREVIEW_LINKS
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push((snap_mkv.clone(), Instant::now()));
                open_path_detached(&snap_mkv)?;
                return Ok(snap_mkv);
            }
        }
    }
    let link_path = find_free_preview_path(preview_link_name(part_path, final_name, Some(ext)))?;
    std::fs::hard_link(part_path, &link_path)
        .context("تعذر إنشاء الرابط الصلب (Hard Link) لمعاينة الملف")?;
    PREVIEW_LINKS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push((link_path.clone(), Instant::now()));
    open_path_detached(&link_path)?;
    Ok(link_path)
}

/// إنشاء لقطة معاينة سريعة (`Matroska .mkv`) لملفي فيديو وصوت قيد التحميل عبر `ffmpeg subfile` بدون إعادة ترميز
pub fn create_dual_stream_preview_snapshot(
    video_part: &Path,
    video_ready_bytes: u64,
    audio_part: Option<(&Path, u64)>,
    output_mkv: &Path,
) -> Result<()> {
    let Some(ffmpeg) = find_tool("ffmpeg") else {
        bail!("أداة ffmpeg غير متوفرة لإعداد لقطة المعاينة");
    };
    let subfile_arg = |p: &Path, n: u64| {
        let mut s = std::ffi::OsString::from(format!("subfile,,start,0,end,{n},,:"));
        s.push(p.as_os_str());
        s
    };

    let mut cmd = Command::new(&ffmpeg);
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-protocol_whitelist",
        "file,subfile",
        "-i",
    ])
    .arg(subfile_arg(video_part, video_ready_bytes));

    if let Some((a_path, a_bytes)) = audio_part {
        cmd.arg("-i")
            .arg(subfile_arg(a_path, a_bytes))
            .args(["-map", "0:v:0", "-map", "1:a:0"]);
    } else {
        cmd.args(["-map", "0:v:0"]);
    }
    cmd.args(["-c", "copy", "-f", "matroska"]).arg(output_mkv);
    hide_console_window(&mut cmd);

    let out = cmd.output().context("تعذر تشغيل ffmpeg لإعداد المعاينة")?;
    let created_ok = std::fs::metadata(output_mkv)
        .map(|m| m.len() > 1024)
        .unwrap_or(false);
    if !created_ok {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("خطأ غير معروف");
        bail!("تعذر تجهيز لقطة المعاينة عبر ffmpeg: {last}");
    }
    Ok(())
}

/// تنظيف روابط المعاينة المؤقتة التي لم يعد أي مشغل وسائط يمسكها (مع مهلة سماح 30 ثانية للمشغلات بطيئة الإقلاع)
pub fn cleanup_finished_previews() {
    cleanup_previews_with_grace(Duration::from_secs(30));
}

pub fn cleanup_all_previews_now() {
    cleanup_previews_with_grace(Duration::ZERO);
}

fn cleanup_previews_with_grace(grace: Duration) {
    let mut links = PREVIEW_LINKS.lock().unwrap_or_else(|p| p.into_inner());
    links.retain(|(p, made)| {
        p.exists() && (made.elapsed() < grace || std::fs::remove_file(p).is_err())
    });
}

fn open_path_detached(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", &path.display().to_string()]);
        hide_console_window(&mut cmd);
        cmd.spawn()?;
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("xdg-open").arg(path).spawn();
    }
    Ok(())
}

#[cfg(windows)]
fn query_windows_player_for_ext(ext: &str) -> Option<PathBuf> {
    if ext.is_empty() {
        return None;
    }
    const ASSOCF_NOTRUNCATE: u32 = 0x00000020;
    const ASSOCF_INIT_IGNOREUNKNOWN: u32 = 0x00000400;
    const ASSOCSTR_EXECUTABLE: i32 = 2;

    #[link(name = "shlwapi")]
    extern "system" {
        fn AssocQueryStringW(
            flags: u32,
            str_type: i32,
            psz_assoc: *const u16,
            psz_extra: *const u16,
            psz_out: *mut u16,
            pcch_out: *mut u32,
        ) -> i32;
    }

    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (dot_ext, verb) = (wide(&format!(".{ext}")), wide("open"));
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    let hr = unsafe {
        AssocQueryStringW(
            ASSOCF_INIT_IGNOREUNKNOWN | ASSOCF_NOTRUNCATE,
            ASSOCSTR_EXECUTABLE,
            dot_ext.as_ptr(),
            verb.as_ptr(),
            buf.as_mut_ptr(),
            &mut len,
        )
    };
    if hr != 0 {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let exe = PathBuf::from(String::from_utf16_lossy(&buf[..end]));
    let lower = exe.to_string_lossy().to_ascii_lowercase();
    let is_stand_in = [
        "rundll32",
        "explorer.exe",
        "openwith",
        "\\windowsapps\\",
        "launchwinapp",
    ]
    .iter()
    .any(|k| lower.contains(k));
    (!is_stand_in && lower.ends_with(".exe") && exe.is_file()).then_some(exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_preview_link_name_with_real_extension() {
        let part = Path::new("/tmp/x/Clip [1080p].mp4.part");
        assert_eq!(
            preview_link_name(part, "Clip [1080p].mp4", None),
            Path::new("/tmp/x/Clip [1080p] (preview).mp4")
        );
        assert_eq!(
            preview_link_name(part, "song", Some("mkv")),
            Path::new("/tmp/x/song (preview).mkv")
        );
    }

    #[test]
    fn hard_link_preview_tracks_live_writes_and_cleans_up() {
        let dir = std::env::temp_dir().join(format!("stormget-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("verified in test");
        let part = dir.join("video.mp4.part");
        std::fs::write(&part, b"initial_chunk").expect("verified in test");
        let link = preview_link_name(&part, "video.mp4", None);
        std::fs::hard_link(&part, &link).expect("verified in test");

        std::fs::write(&part, b"initial_chunk_plus_more").expect("verified in test");
        assert_eq!(std::fs::read(&link).expect("verified in test"), b"initial_chunk_plus_more");

        PREVIEW_LINKS
            .lock()
            .expect("verified in test")
            .push((link.clone(), Instant::now()));
        cleanup_finished_previews();
        assert!(link.exists(), "Grace period keeps recent preview link alive");
        cleanup_all_previews_now();
        assert!(!link.exists() && part.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
