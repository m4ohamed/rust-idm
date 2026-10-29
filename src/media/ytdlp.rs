//! جسر التكامل غير المتزامن مع `yt-dlp` ومحلل جلسات يوتيوب `SABR` (`Server ABR Streaming` + `poToken` Protobuf Seed)،
//! ذاكرة التخزين المؤقت لفحص يوتيوب وحارس تقييد الطلبات (`YoutubeProbeCache` & `429 Cooldown` المستلهم من `awesome-downloader`)،
//! الدمج المباشر لمسارات الصوت والفيديو (`ffmpeg -c copy +faststart`)،
//! ومحول ترجمات الفيديو المعياري (`WebVTT -> SubRip .srt`)

use crate::{
    core::tools::{find_tool, hide_tokio_console_window, merge_video_and_audio_streams},
    media::hls::MediaStreamEngine,
};
use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    time::{Duration, Instant},
};
use tokio::{
    io::AsyncReadExt,
    process::Command as TokioCommand,
    sync::Mutex,
    time::sleep,
};
use url::Url;

// ============================================================================
// 1. دعم بروتوكول يوتيوب الحديث SABR واستخراج poToken من Protobuf (مستلهم من Awesome Downloader)
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SabrAudioTrackInfo {
    #[serde(default)]
    pub id: String,
    #[serde(default, alias = "audioIsDefault")]
    pub audio_is_default: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SabrFormatItem {
    pub itag: u32,
    #[serde(default, alias = "mimeType")]
    pub mime_type: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub fps: Option<u32>,
    #[serde(default)]
    pub bitrate: Option<u64>,
    #[serde(default, alias = "contentLength")]
    pub content_length: Option<String>,
    #[serde(default, alias = "approxDurationMs")]
    pub approx_duration_ms: Option<String>,
    #[serde(default, alias = "qualityLabel")]
    pub quality_label: Option<String>,
    #[serde(default, alias = "audioQuality")]
    pub audio_quality: Option<String>,
    #[serde(default, alias = "audioTrack")]
    pub audio_track: Option<SabrAudioTrackInfo>,
    #[serde(default, alias = "isDrc")]
    pub is_drc: bool,
    #[serde(default, alias = "isVb")]
    pub is_vb: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SabrClientInfo {
    #[serde(default, alias = "clientName")]
    pub client_name: Option<u32>,
    #[serde(default, alias = "clientVersion")]
    pub client_version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SabrSessionPayload {
    #[serde(alias = "videoId")]
    pub video_id: String,
    #[serde(alias = "serverAbrStreamingUrl")]
    pub server_abr_streaming_url: String,
    #[serde(alias = "videoPlaybackUstreamerConfig")]
    pub video_playback_ustreamer_config: String,
    #[serde(default, alias = "clientInfo")]
    pub client_info: Option<SabrClientInfo>,
    #[serde(default, alias = "videoItag")]
    pub video_itag: Option<u32>,
    #[serde(default, alias = "audioItag")]
    pub audio_itag: u32,
    #[serde(default, alias = "requestBody")]
    pub request_body: Option<String>,
    #[serde(default)]
    pub formats: Vec<SabrFormatItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedSabrSession {
    pub video_id: String,
    pub server_abr_streaming_url: String,
    pub video_playback_ustreamer_config: String,
    pub client_name: u32,
    pub client_version: String,
    pub video_itag: Option<u32>,
    pub audio_itag: u32,
    pub po_token: Option<String>,
    pub estimated_total_bytes: Option<u64>,
    pub approx_duration_secs: Option<u64>,
}

/// فك ترميز Base64 القياسي في Rust الصرفة (دون اعتماديات خارجية)
pub fn decode_base64_standard(input: &str) -> Option<Vec<u8>> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > 350_000 || trimmed.len() % 4 != 0 {
        return None;
    }
    let val_of = |b: u8| -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    };

    let bytes = trimmed.as_bytes();
    let mut out = Vec::with_capacity((bytes.len() / 4) * 3);
    for chunk in bytes.chunks_exact(4) {
        let a = val_of(chunk[0])?;
        let b = val_of(chunk[1])?;
        out.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            let c = val_of(chunk[2])?;
            out.push((b << 4) | (c >> 2));
            if chunk[3] != b'=' {
                let d = val_of(chunk[3])?;
                out.push((c << 6) | d);
            }
        } else if chunk[3] != b'=' {
            return None;
        }
    }
    Some(out)
}

/// ترميز Base64 القياسي في Rust الصرفة
pub fn encode_base64_standard(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(((bytes.len() + 2) / 3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn read_protobuf_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut result = 0u64;
    let mut shift = 0u32;
    while *pos < buf.len() {
        let byte = buf[*pos];
        *pos += 1;
        result |= ((byte & 0x7F) as u64) << shift;
        if (byte & 0x80) == 0 {
            return Some(result);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    None
}

fn skip_protobuf_field(buf: &[u8], pos: &mut usize, wire_type: u8) -> Option<()> {
    match wire_type {
        0 => {
            read_protobuf_varint(buf, pos)?;
            Some(())
        }
        1 => {
            if *pos + 8 > buf.len() {
                return None;
            }
            *pos += 8;
            Some(())
        }
        2 => {
            let len = read_protobuf_varint(buf, pos)? as usize;
            if *pos + len > buf.len() {
                return None;
            }
            *pos += len;
            Some(())
        }
        5 => {
            if *pos + 4 > buf.len() {
                return None;
            }
            *pos += 4;
            Some(())
        }
        _ => None,
    }
}

/// فك حزمة Protobuf الخاصة بـ `VideoPlaybackAbrRequest` لاستخراج `poToken` ومعلومات العميل (`clientName`, `clientVersion`)
/// وفق مخطط `googlevideo`:
/// - الحقل 1 في `VideoPlaybackAbrRequest` هو `streamer_context` (wire_type = 2)
///   - الحقل 1 داخل `StreamerContext` هو `client_info` (wire_type = 2):
///     - الحقل 16 هو `client_name` (varint)، والحقل 17 هو `client_version` (string)
///   - الحقل 2 داخل `StreamerContext` هو `po_token` (bytes, wire_type = 2)
pub fn extract_sabr_context_from_protobuf(
    buf: &[u8],
) -> Option<(Option<String>, Option<u32>, Option<String>)> {
    if buf.is_empty() || buf.len() > 262_144 {
        return None;
    }
    let mut pos = 0usize;
    let mut po_token_b64: Option<String> = None;
    let mut client_name: Option<u32> = None;
    let mut client_version: Option<String> = None;

    while pos < buf.len() {
        let tag = read_protobuf_varint(buf, &mut pos)?;
        let field_num = (tag >> 3) as u32;
        let wire_type = (tag & 0x07) as u8;

        if field_num == 1 && wire_type == 2 {
            let sc_len = read_protobuf_varint(buf, &mut pos)? as usize;
            if pos + sc_len > buf.len() {
                return None;
            }
            let sc_slice = &buf[pos..pos + sc_len];
            pos += sc_len;

            let mut sc_pos = 0usize;
            while sc_pos < sc_slice.len() {
                let sc_tag = read_protobuf_varint(sc_slice, &mut sc_pos)?;
                let sc_field = (sc_tag >> 3) as u32;
                let sc_wire = (sc_tag & 0x07) as u8;

                if sc_field == 1 && sc_wire == 2 {
                    let ci_len = read_protobuf_varint(sc_slice, &mut sc_pos)? as usize;
                    if sc_pos + ci_len > sc_slice.len() {
                        break;
                    }
                    let ci_slice = &sc_slice[sc_pos..sc_pos + ci_len];
                    sc_pos += ci_len;

                    let mut ci_pos = 0usize;
                    while ci_pos < ci_slice.len() {
                        let Some(ci_tag) = read_protobuf_varint(ci_slice, &mut ci_pos) else {
                            break;
                        };
                        let ci_field = (ci_tag >> 3) as u32;
                        let ci_wire = (ci_tag & 0x07) as u8;
                        if (ci_field == 16 || ci_field == 1) && ci_wire == 0 {
                            if let Some(v) = read_protobuf_varint(ci_slice, &mut ci_pos) {
                                client_name = Some(v as u32);
                            }
                        } else if (ci_field == 17 || ci_field == 2) && ci_wire == 2 {
                            let Some(v_len) = read_protobuf_varint(ci_slice, &mut ci_pos) else {
                                break;
                            };
                            let v_len = v_len as usize;
                            if ci_pos + v_len <= ci_slice.len() {
                                if let Ok(s) = std::str::from_utf8(&ci_slice[ci_pos..ci_pos + v_len])
                                {
                                    client_version = Some(s.to_string());
                                }
                                ci_pos += v_len;
                            } else {
                                break;
                            }
                        } else if skip_protobuf_field(ci_slice, &mut ci_pos, ci_wire).is_none() {
                            break;
                        }
                    }
                } else if sc_field == 2 && sc_wire == 2 {
                    let pt_len = read_protobuf_varint(sc_slice, &mut sc_pos)? as usize;
                    if sc_pos + pt_len > sc_slice.len() {
                        break;
                    }
                    let pt_bytes = &sc_slice[sc_pos..sc_pos + pt_len];
                    sc_pos += pt_len;
                    if !pt_bytes.is_empty() {
                        po_token_b64 = Some(encode_base64_standard(pt_bytes));
                    }
                } else {
                    skip_protobuf_field(sc_slice, &mut sc_pos, sc_wire)?;
                }
            }
        } else {
            skip_protobuf_field(buf, &mut pos, wire_type)?;
        }
    }

    Some((po_token_b64, client_name, client_version))
}

/// التحقق الصارم من رابط تدفق يوتيوب SABR (`https://*.googlevideo.com/videoplayback?...sabr=1`)
pub fn validate_google_video_sabr_url(raw_url: &str) -> Result<String> {
    let parsed = Url::parse(raw_url.trim()).context("صيغة رابط يوتيوب SABR غير صالحة")?;
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("رابط يوتيوب SABR يجب أن يستخدم بروتوكول HTTPS بدون بيانات اعتماد مضمنة");
    }
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    if !(host == "googlevideo.com" || host.ends_with(".googlevideo.com"))
        || parsed.path() != "/videoplayback"
    {
        bail!("نطاق أو مسار رابط SABR غير صالح: {host}{}", parsed.path());
    }
    let is_sabr = parsed
        .query_pairs()
        .any(|(k, v)| k == "sabr" && v == "1");
    if !is_sabr {
        bail!("رابط videoplayback لا يحمل معلمة sabr=1");
    }
    Ok(parsed.to_string())
}

/// تطبيع والتحقق من جلسة يوتيوب SABR (مستلهم من `awesome-downloader/core/sabr.mjs:normalizeSabrSession`)
pub fn normalize_sabr_session(raw: &SabrSessionPayload) -> Result<NormalizedSabrSession> {
    let vid = raw.video_id.trim();
    if vid.len() != 11
        || !vid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("معرف فيديو يوتيوب (videoId) في جلسة SABR غير صالح");
    }

    let server_abr_streaming_url = validate_google_video_sabr_url(&raw.server_abr_streaming_url)?;

    let cfg = raw.video_playback_ustreamer_config.trim();
    if cfg.is_empty() || cfg.len() > 131_072 {
        bail!("إعدادات تدفق يوتيوب (videoPlaybackUstreamerConfig) مفقودة أو تتجاوز الحد المسموح");
    }

    if raw.formats.is_empty() || raw.formats.len() > 150 {
        bail!("قائمة تنسيقات يوتيوب SABR فارغة أو تتجاوز 150 تنسيقاً");
    }

    // اختيار المسار الصوتي الافتراضي مع استبعاد المسارات المدبلجة غير الافتراضية أو المضغوطة (DRC / Voice-Boost)
    let audio_candidates: Vec<&SabrFormatItem> = raw
        .formats
        .iter()
        .filter(|f| {
            f.itag == raw.audio_itag
                && f.mime_type.to_ascii_lowercase().starts_with("audio/")
                && !f.is_drc
                && !f.is_vb
        })
        .collect();

    let chosen_audio = audio_candidates
        .iter()
        .copied()
        .find(|f| f.audio_track.as_ref().map(|t| t.audio_is_default).unwrap_or(false))
        .or_else(|| {
            audio_candidates
                .iter()
                .copied()
                .find(|f| f.audio_track.is_none())
        })
        .or_else(|| audio_candidates.first().copied())
        .ok_or_else(|| anyhow!("المسار الصوتي المختار في جلسة SABR غير متاح"))?;

    let chosen_video = match raw.video_itag {
        Some(v_itag) => {
            let v = raw
                .formats
                .iter()
                .find(|f| {
                    f.itag == v_itag && f.mime_type.to_ascii_lowercase().starts_with("video/")
                })
                .ok_or_else(|| anyhow!("مسار الفيديو المختار في جلسة SABR غير متاح"))?;
            Some(v)
        }
        None => None,
    };

    let mut po_token: Option<String> = None;
    let mut proto_client_name: Option<u32> = None;
    let mut proto_client_ver: Option<String> = None;

    if let Some(ref b64_body) = raw.request_body {
        if !b64_body.trim().is_empty() {
            let decoded = decode_base64_standard(b64_body)
                .ok_or_else(|| anyhow!("سياق طلب SABR (requestBody base64) غير صالح"))?;
            if decoded.is_empty() || decoded.len() > 262_144 {
                bail!("حجم سياق طلب SABR غير صالح");
            }
            if let Some((pt, cn, cv)) = extract_sabr_context_from_protobuf(&decoded) {
                po_token = pt;
                proto_client_name = cn;
                proto_client_ver = cv;
            }
        }
    }

    let client_name = proto_client_name
        .or_else(|| raw.client_info.as_ref().and_then(|c| c.client_name))
        .unwrap_or(1);
    let client_version = proto_client_ver
        .or_else(|| {
            raw.client_info
                .as_ref()
                .and_then(|c| c.client_version.clone())
        })
        .unwrap_or_default();

    let parse_u64 = |opt: &Option<String>| -> Option<u64> {
        opt.as_ref()?.trim().parse::<u64>().ok().filter(|&n| n > 0)
    };

    let audio_len = parse_u64(&chosen_audio.content_length);
    let video_len = chosen_video.and_then(|v| parse_u64(&v.content_length));
    let estimated_total_bytes = match (video_len, audio_len) {
        (Some(v), Some(a)) => Some(v.saturating_add(a)),
        (None, Some(a)) if chosen_video.is_none() => Some(a),
        _ => None,
    };

    let dur_ms = chosen_audio
        .approx_duration_ms
        .as_ref()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
        .max(
            chosen_video
                .and_then(|v| v.approx_duration_ms.as_ref())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0),
        );

    Ok(NormalizedSabrSession {
        video_id: vid.to_string(),
        server_abr_streaming_url,
        video_playback_ustreamer_config: cfg.to_string(),
        client_name,
        client_version,
        video_itag: chosen_video.map(|v| v.itag),
        audio_itag: chosen_audio.itag,
        po_token,
        estimated_total_bytes,
        approx_duration_secs: (dur_ms > 0).then_some(dur_ms / 1000),
    })
}

// ============================================================================
// 2. ذاكرة التخزين المؤقت لفحص يوتيوب وحارس تقييد الطلبات 429 (مستلهم من Awesome Downloader)
// ============================================================================

#[derive(Debug, Clone)]
pub struct CachedYoutubeProbeEntry {
    pub expires_at: Instant,
    pub result: Result<serde_json::Value, String>,
}

#[derive(Default)]
pub struct YoutubeProbeCache {
    entries: HashMap<String, CachedYoutubeProbeEntry>,
    rate_limit_until: Option<Instant>,
}

static YT_PROBE_CACHE: OnceLock<Mutex<YoutubeProbeCache>> = OnceLock::new();

pub fn global_youtube_probe_cache() -> &'static Mutex<YoutubeProbeCache> {
    YT_PROBE_CACHE.get_or_init(|| Mutex::new(YoutubeProbeCache::default()))
}

/// تصنيف أخطاء يوتيوب (`429 Too Many Requests` أو `Bot/Sign-in Verification`) برسائل واضحة
pub fn classify_youtube_error(raw_stderr: &str) -> (String, u16) {
    let lower = raw_stderr.to_ascii_lowercase();
    if lower.contains("http error 429") || lower.contains("too many requests") {
        return (
            "يرفض يوتيوب الطلبات مؤقتاً بسبب كثرة المحاولات (HTTP 429 Too Many Requests). يرجى الانتظار دقيقتين قبل المحاولة مجدداً.".to_string(),
            429,
        );
    }
    if lower.contains("not a bot")
        || lower.contains("sign in to confirm")
        || lower.contains("login required")
        || lower.contains("cookies-from-browser")
    {
        return (
            "يتطلب يوتيوب التحقق من المتصفح أو تسجيل الدخول لهذا الفيديو. يرجى فتح الفيديو في المتصفح مع تفعيل إضافة StormGet لتمرير الجلسة والكوكيز تلقائياً.".to_string(),
            422,
        );
    }
    let clean = raw_stderr
        .lines()
        .rev()
        .find(|l| l.contains("ERROR:") || l.contains("error:"))
        .unwrap_or(raw_stderr)
        .trim();
    let truncated: String = clean.chars().take(500).collect();
    (
        if truncated.is_empty() {
            "فشل استخراج أو تحميل فيديو يوتيوب.".to_string()
        } else {
            truncated
        },
        400,
    )
}

impl YoutubeProbeCache {
    pub fn get_valid(&mut self, url: &str) -> Option<Result<serde_json::Value, String>> {
        let now = Instant::now();
        if let Some(until) = self.rate_limit_until {
            if now < until {
                let (msg, _) = classify_youtube_error("HTTP Error 429");
                return Some(Err(msg));
            } else {
                self.rate_limit_until = None;
            }
        }
        if let Some(entry) = self.entries.get(url) {
            if entry.expires_at > now {
                return Some(entry.result.clone());
            }
        }
        self.entries.remove(url);
        None
    }

    pub fn insert_result(&mut self, url: String, result: Result<serde_json::Value, String>) {
        let ttl = Duration::from_secs(120);
        if let Err(ref e) = result {
            if e.contains("429") || e.contains("Too Many Requests") {
                self.rate_limit_until = Some(Instant::now() + ttl);
            }
        }
        if self.entries.len() >= 20 {
            if let Some(first_key) = self.entries.keys().next().cloned() {
                self.entries.remove(&first_key);
            }
        }
        self.entries.insert(
            url,
            CachedYoutubeProbeEntry {
                expires_at: Instant::now() + ttl,
                result,
            },
        );
    }
}

/// حارس RAII لحذف ملف الكوكيز المؤقت تلقائياً عند انتهاء الاستدعاء أو الإلغاء
pub struct TempCookieGuard(pub std::path::PathBuf);

impl Drop for TempCookieGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// إنشاء ملف كوكيز مؤقت بنسق Netscape القياسي لتقديمه لـ yt-dlp بدلاً من --add-header
pub fn create_temp_netscape_cookie_file(
    target_url: &str,
    raw_cookies: &str,
) -> std::io::Result<TempCookieGuard> {
    use std::io::Write;
    let host = url::Url::parse(target_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_else(|| "youtube.com".to_string());

    let domain = if host.starts_with('.') {
        host
    } else {
        format!(".{host}")
    };

    let temp_path =
        std::env::temp_dir().join(format!("rustidm_cookie_{}.txt", uuid::Uuid::new_v4()));
    let mut file = std::fs::File::create(&temp_path)?;
    writeln!(file, "# Netscape HTTP Cookie File")?;
    writeln!(file, "# Generated by StormGet v2.3 for yt-dlp")?;

    for item in raw_cookies.split(';') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if let Some((k, v)) = item.split_once('=') {
            let k = k.trim();
            let v = v.trim();
            if !k.is_empty() {
                writeln!(file, "{domain}\tTRUE\t/\tTRUE\t2147483647\t{k}\t{v}")?;
            }
        }
    }

    Ok(TempCookieGuard(temp_path))
}

/// البحث التلقائي عن مسار أداة FFmpeg لدمج مسارات الصوت والفيديو (مجلد tools المدمج، PATH، WinGet)
pub fn find_ffmpeg() -> Option<PathBuf> {
    find_tool("ffmpeg")
}

/// البحث التلقائي عن مسار أداة yt-dlp (مجلد tools المدمج، PATH، WinGet)
pub fn find_ytdlp() -> Option<PathBuf> {
    find_tool("yt-dlp")
}

impl MediaStreamEngine {
    /// جسر تكامل غير متزامن مع yt-dlp وجلسات يوتيوب SABR يدعم:
    /// 1. تطبيع جلسات `SABR` وتمرير `poToken` المستخرج من `VideoPlaybackAbrRequest` لتجاوز حجب 403.
    /// 2. استخدام النسخة المثبتة تلقائياً في `tools/yt-dlp.exe` أو المثبتة في مسار النظام.
    /// 3. الإلغاء الفوري (kill_on_drop) لمنع العمليات اليتيمة وإخفاء نافذة الكونسول السوداء.
    /// 4. الإيقاف المؤقت والاستئناف الحقيقي عبر تفعيل `--continue` (`-c`).
    /// 5. الكشف التلقائي عن FFmpeg وتمريره لـ yt-dlp لدمج مسارات الصوت والفيديو (Muxing).
    #[allow(clippy::too_many_arguments)]
    pub async fn download_via_ytdlp_with_format(
        url: &str,
        output_file: &Path,
        format_selector: Option<&str>,
        cookies_header: Option<&str>,
        referer: Option<&str>,
        user_agent: Option<&str>,
        pause_flag: Arc<AtomicBool>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<()> {
        Self::download_via_ytdlp_or_sabr(
            url,
            output_file,
            format_selector,
            None,
            cookies_header,
            referer,
            user_agent,
            0,
            pause_flag,
            cancel_flag,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn download_via_ytdlp_or_sabr(
        url: &str,
        output_file: &Path,
        format_selector: Option<&str>,
        sabr_session: Option<&SabrSessionPayload>,
        cookies_header: Option<&str>,
        referer: Option<&str>,
        user_agent: Option<&str>,
        speed_limit_kbps: u64,
        pause_flag: Arc<AtomicBool>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<()> {
        let normalized_sabr = match sabr_session {
            Some(sabr) => Some(normalize_sabr_session(sabr)?),
            None => None,
        };

        // إذا كانت المهمة جلسة SABR، فإن رابط صفحة الفيديو الأصلي على يوتيوب هو الهدف الفعلي لـ yt-dlp مع الـ itags المحددة
        let effective_target_url = if let Some(ref sabr) = normalized_sabr {
            format!("https://www.youtube.com/watch?v={}", sabr.video_id)
        } else {
            url.to_string()
        };

        // دفاع إضافي: لا يُمرَّر لـ yt-dlp إلا رابط ويب صالح (يمنع حقن الخيارات مثل --exec)
        let safe_url = crate::core::tools::validate_web_url_for_tools(&effective_target_url)?;
        let ffmpeg_bin = find_ffmpeg();
        let ytdlp_bin = find_ytdlp().unwrap_or_else(|| PathBuf::from("yt-dlp"));

        let sabr_fmt_string = normalized_sabr.as_ref().map(|sabr| {
            if let Some(v_itag) = sabr.video_itag {
                format!("{}+{}/bestvideo+bestaudio/best", v_itag, sabr.audio_itag)
            } else {
                format!("{}/bestaudio/best", sabr.audio_itag)
            }
        });

        let raw_fmt = format_selector
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.strip_prefix("audio:").unwrap_or(s))
            .or(sabr_fmt_string.as_deref())
            .unwrap_or_else(|| {
                if ffmpeg_bin.is_some() {
                    "bestvideo+bestaudio/best"
                } else {
                    "best[ext=mp4]/best/b"
                }
            });

        let is_audio_only = format_selector
            .map(|s| s.starts_with("audio:") || s == "bestaudio/best")
            .unwrap_or(false)
            || normalized_sabr
                .as_ref()
                .map(|s| s.video_itag.is_none())
                .unwrap_or(false);

        let effective_fmt = if ffmpeg_bin.is_none() && raw_fmt.contains('+') {
            "best[ext=mp4]/best/b"
        } else {
            raw_fmt
        };

        loop {
            while pause_flag.load(Ordering::Relaxed) {
                if cancel_flag.load(Ordering::Relaxed) {
                    return Err(anyhow!("تم إلغاء عملية yt-dlp"));
                }
                sleep(Duration::from_millis(200)).await;
            }

            if cancel_flag.load(Ordering::Relaxed) {
                return Err(anyhow!("تم إلغاء عملية yt-dlp"));
            }

            let mut cmd = TokioCommand::new(&ytdlp_bin);
            hide_tokio_console_window(&mut cmd);
            if let Some(work_dir) = crate::core::tools::writable_tool_dir("yt-dlp") {
                cmd.env("TEMP", &work_dir).env("TMP", &work_dir);
            }
            cmd.kill_on_drop(true)
                .arg("--no-playlist")
                .arg("--no-warnings")
                .arg("-c")
                .arg("-f")
                .arg(effective_fmt)
                .arg("--remote-components")
                .arg("ejs:github")
                .arg("--js-runtimes")
                .arg("node");

            if !is_audio_only {
                cmd.arg("--merge-output-format").arg("mp4");
            }

            if speed_limit_kbps > 0 {
                cmd.arg("--limit-rate")
                    .arg((speed_limit_kbps * 1024).to_string());
            }

            // تمرير poToken المستخرج من جلسة يوتيوب SABR عند توفره لمنع خطأ 403 / Bot Check
            if let Some(ref sabr) = normalized_sabr {
                if let Some(ref token) = sabr.po_token {
                    cmd.arg("--extractor-args")
                        .arg(format!("youtube:po_token=web+{token}"));
                }
            }

            if let Some(ref p) = ffmpeg_bin {
                cmd.arg("--ffmpeg-location").arg(p);
            }

            cmd.arg("-o").arg(output_file).stderr(Stdio::piped());

            let _cookie_guard = if let Some(cookie) =
                cookies_header.filter(|value| !value.trim().is_empty())
            {
                if let Ok(guard) = create_temp_netscape_cookie_file(&safe_url, cookie) {
                    cmd.arg("--cookies").arg(&guard.0);
                    Some(guard)
                } else {
                    cmd.arg("--add-header").arg(format!("Cookie: {cookie}"));
                    None
                }
            } else {
                None
            };

            if let Some(ref_url) = referer.filter(|value| !value.is_empty()) {
                cmd.arg("--referer").arg(ref_url);
            }
            if let Some(ua) = user_agent.filter(|value| !value.is_empty()) {
                cmd.arg("--user-agent").arg(ua);
            }

            let mut child = cmd.arg("--").arg(&safe_url).spawn().context(
                "تعذر تشغيل yt-dlp؛ سيقوم StormGet بتنزيل yt-dlp و ffmpeg تلقائياً في مجلد tools",
            )?;

            let mut stderr_pipe = child.stderr.take();
            let stderr_buf = Arc::new(Mutex::new(String::new()));
            let stderr_buf_clone = Arc::clone(&stderr_buf);

            let stderr_drain = tokio::spawn(async move {
                if let Some(mut reader) = stderr_pipe.take() {
                    let mut temp = vec![0u8; 2048];
                    let mut accum = String::new();
                    while let Ok(n) = reader.read(&mut temp).await {
                        if n == 0 {
                            break;
                        }
                        accum.push_str(&String::from_utf8_lossy(&temp[..n]));
                        if accum.len() > 8192 {
                            let start_idx = accum.len() - 4096;
                            accum = accum[start_idx..].to_string();
                        }
                    }
                    let mut g = stderr_buf_clone.lock().await;
                    *g = accum;
                }
            });

            loop {
                if cancel_flag.load(Ordering::Relaxed) {
                    let _ = child.kill().await;
                    stderr_drain.abort();
                    return Err(anyhow!(
                        "تم إلغاء عملية yt-dlp وإنهاء العملية الفرعية بأمان"
                    ));
                }
                if pause_flag.load(Ordering::Relaxed) {
                    let _ = child.kill().await;
                    stderr_drain.abort();
                    break;
                }
                if let Some(status) = child.try_wait()? {
                    let _ = stderr_drain.await;
                    if status.success() {
                        if output_file.exists() {
                            return Ok(());
                        }
                        if let Some(parent) = output_file.parent() {
                            if let Some(stem) = output_file.file_stem().and_then(|s| s.to_str()) {
                                if let Ok(entries) = std::fs::read_dir(parent) {
                                    let mut v_part: Option<PathBuf> = None;
                                    let mut a_part: Option<PathBuf> = None;
                                    for entry in entries.flatten() {
                                        let p = entry.path();
                                        if let Some(fname) = p.file_name().and_then(|n| n.to_str())
                                        {
                                            if fname.starts_with(stem) && p != output_file {
                                                if (fname.contains(".f")
                                                    || fname.contains(".video"))
                                                    && (fname.ends_with(".mp4")
                                                        || fname.ends_with(".webm")
                                                        || fname.ends_with(".mkv"))
                                                {
                                                    v_part = Some(p.clone());
                                                } else if (fname.contains(".f")
                                                    || fname.contains(".audio"))
                                                    && (fname.ends_with(".webm")
                                                        || fname.ends_with(".m4a")
                                                        || fname.ends_with(".opus"))
                                                {
                                                    a_part = Some(p.clone());
                                                }
                                            }
                                        }
                                    }
                                    if let (Some(v), Some(a)) = (v_part, a_part) {
                                        let out_name = output_file
                                            .file_name()
                                            .and_then(|n| n.to_str())
                                            .unwrap_or("video.mp4");
                                        if merge_video_and_audio_streams(&v, &a, output_file, out_name)
                                            .is_ok()
                                            && output_file.exists()
                                        {
                                            let _ = std::fs::remove_file(v);
                                            let _ = std::fs::remove_file(a);
                                            return Ok(());
                                        }
                                    }
                                }
                            }
                        }
                        return Ok(());
                    }
                    let captured_err = stderr_buf.lock().await.clone();
                    let (friendly_err, _) = classify_youtube_error(&captured_err);
                    return Err(anyhow!("فشلت عملية yt-dlp: {friendly_err}"));
                }
                sleep(Duration::from_millis(200)).await;
            }
        }
    }

    /// جسر تكامل غير متزامن مع yt-dlp يدعم الإلغاء الفوري (kill_on_drop) ومنع بقاء عمليات يتيمة
    pub async fn download_via_ytdlp_cancellable(
        url: &str,
        output_dir: &Path,
        cookies_header: Option<&str>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<()> {
        let output_template = output_dir.join("%(title)s.%(ext)s");
        Self::download_via_ytdlp_with_format(
            url,
            &output_template,
            None,
            cookies_header,
            None,
            None,
            Arc::new(AtomicBool::new(false)),
            cancel_flag,
        )
        .await
    }
}

// ============================================================================
// 3. محول ملفات الترجمة المدمج (WebVTT to SubRip .srt Converter)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleCue {
    pub index: usize,
    pub start_time_ms: u64,
    pub end_time_ms: u64,
    pub text: String,
}

pub struct SubtitleConverter;

impl SubtitleConverter {
    /// تحويل ملفات الترجمة بصيغة WebVTT (`.vtt`) المصاحبة لبث HLS/DASH إلى صيغة SubRip القياسية (`.srt`)
    pub fn convert_webvtt_to_srt(vtt_text: &str) -> String {
        let mut srt_out = String::new();
        let mut cue_idx = 1usize;
        let mut lines = vtt_text.lines().peekable();
        let tag_re = Regex::new(r"</?[^>]+>").ok();

        while let Some(raw_line) = lines.next() {
            let line = raw_line.trim();
            if line.is_empty()
                || line.starts_with("WEBVTT")
                || line.starts_with("NOTE")
                || line.starts_with("STYLE")
                || line.starts_with("REGION")
                || line.starts_with("Kind:")
                || line.starts_with("Language:")
            {
                continue;
            }

            if line.contains("-->") {
                let timing_parts: Vec<&str> = line.split("-->").collect();
                if timing_parts.len() >= 2 {
                    let start_raw = timing_parts[0].trim();
                    let end_raw = timing_parts[1]
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .trim();

                    let start_srt = Self::normalize_vtt_timestamp(start_raw);
                    let end_srt = Self::normalize_vtt_timestamp(end_raw);
                    let _cue_meta = SubtitleCue {
                        index: cue_idx,
                        start_time_ms: 0,
                        end_time_ms: 0,
                        text: format!("{start_srt} --> {end_srt}"),
                    };

                    let mut text_lines = Vec::new();
                    while let Some(&next_line) = lines.peek() {
                        let trimmed_next = next_line.trim();
                        if trimmed_next.is_empty() {
                            lines.next();
                            break;
                        }
                        let Some(raw_consumed) = lines.next() else { break; };
                        let consumed = raw_consumed.trim();
                        let cleaned = if let Some(ref re) = tag_re {
                            let without_voice =
                                re.replace_all(consumed, |caps: &regex::Captures| {
                                    let tag = &caps[0];
                                    if tag.starts_with("<i")
                                        || tag == "</i>"
                                        || tag.starts_with("<b")
                                        || tag == "</b>"
                                    {
                                        tag.to_string()
                                    } else {
                                        String::new()
                                    }
                                });
                            without_voice.to_string()
                        } else {
                            consumed.to_string()
                        };

                        if !cleaned.is_empty() {
                            text_lines.push(cleaned);
                        }
                    }

                    if !text_lines.is_empty() {
                        srt_out.push_str(&format!(
                            "{}\n{} --> {}\n{}\n\n",
                            cue_idx,
                            start_srt,
                            end_srt,
                            text_lines.join("\n")
                        ));
                        cue_idx += 1;
                    }
                }
            }
        }

        srt_out
    }

    /// تطبيع الطابع الزمني لـ WebVTT إلى نسق SRT القياسي: `HH:MM:SS,mmm`
    pub fn normalize_vtt_timestamp(ts: &str) -> String {
        let replaced = ts.replace('.', ",");
        let parts: Vec<&str> = replaced.split(',').collect();
        let time_part = parts[0];
        let ms_part = if parts.len() > 1 { parts[1] } else { "000" };

        let formatted_ms = match ms_part.len() {
            0 => "000".to_string(),
            1 => format!("{}00", ms_part),
            2 => format!("{}0", ms_part),
            3 => ms_part.to_string(),
            _ => ms_part[..3].to_string(),
        };

        let sub_parts: Vec<&str> = time_part.split(':').collect();
        let formatted_time = match sub_parts.len() {
            1 => format!("00:00:{:0>2}", sub_parts[0]),
            2 => format!("00:{:0>2}:{:0>2}", sub_parts[0], sub_parts[1]),
            _ => format!(
                "{:0>2}:{:0>2}:{:0>2}",
                sub_parts[0], sub_parts[1], sub_parts[2]
            ),
        };

        format!("{},{}", formatted_time, formatted_ms)
    }
}


// ============================================================================
// 4. تحسينات يوتيوب المتقدمة المستلهمة من `Ezoo Download`:
//    - ترتيب المسارات الصوتية (`audio_track_rank`) لتفضيل اللغة الأصلية وتجنب الدبلجة الآلية.
//    - فصل خيار `AV1` (`MP4 AV1`) عن `H.264` لتوفير حجم ملف أصغر بنفس الجودة.
//    - خزان الكوكيز المؤقت في الذاكرة (`SITE_COOKIE_JAR`) والمحاولة بدون كوكيز أولاً (`needs_login_or_cookies`).
//    - دعم استخراج قوائم التشغيل الكاملة (`PlaylistInfo` / `fetch_playlist_flat`).
// ============================================================================

/// حساب رتبة المسار الصوتي لتفضيل الصوت الأصلي (`original` / `default`) واستبعاد المسارات المدبلجة آلياً (`dubbed` / `descriptive`)
pub fn audio_track_rank(fmt: &serde_json::Value) -> i64 {
    let note = format!(
        "{} {}",
        fmt.get("format_note").and_then(|v| v.as_str()).unwrap_or(""),
        fmt.get("format_id").and_then(|v| v.as_str()).unwrap_or("")
    )
    .to_ascii_lowercase();
    let mut rank = fmt
        .get("language_preference")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if note.contains("original") {
        rank += 100;
    } else if note.contains("default") {
        rank += 50;
    }
    if note.contains("dubbed") || note.contains("descriptive") {
        rank -= 100;
    }
    rank
}

/// فحص ما إذا كان ترميز الفيديو هو `AV1` (`av01.*`)
pub fn is_av1_vcodec(vcodec: &str) -> bool {
    let lower = vcodec.trim().to_ascii_lowercase();
    lower.starts_with("av01") || lower == "av1"
}

/// فحص ما إذا كانت رسالة خطأ `yt-dlp` تتطلب إعادة المحاولة باستخدام كوكيز المتصفح
pub fn needs_login_or_cookies(err_msg: &str) -> bool {
    let m = err_msg.to_ascii_lowercase();
    [
        "sign in",
        "log in",
        "login",
        "cookies",
        "age",
        "bot",
        "private",
        "registered users",
        "rate-limit",
        "empty media",
    ]
    .iter()
    .any(|k| m.contains(k))
}

/// استخراج النطاق الأساسي للموقع لمشاركة الكوكيز في الذاكرة (`youtu.be` -> `youtube.com`)
pub fn canonical_cookie_site(url_str: &str) -> String {
    let host = Url::parse(url_str)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default();
    if host == "youtu.be" || host.ends_with(".youtu.be") {
        return "youtube.com".to_string();
    }
    let parts: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    let keep = if parts.len() >= 3
        && parts[parts.len() - 2].len() <= 3
        && parts[parts.len() - 1].len() == 2
    {
        3
    } else {
        2
    };
    parts[parts.len().saturating_sub(keep)..].join(".")
}

static SITE_COOKIE_JAR: std::sync::Mutex<Vec<(String, String)>> =
    std::sync::Mutex::new(Vec::new());

/// حفظ كوكيز الموقع في الذاكرة فقط خلال الجلسة لاستخدامها عند الحاجة دون كتابتها للقرص بشكل دائم
pub fn remember_site_cookies(page_url: &str, raw_cookies: &str) {
    let trimmed = raw_cookies.trim();
    if trimmed.is_empty() {
        return;
    }
    let site = canonical_cookie_site(page_url);
    if site.is_empty() {
        return;
    }
    let mut jar = SITE_COOKIE_JAR.lock().unwrap_or_else(|p| p.into_inner());
    jar.retain(|(s, _)| *s != site);
    jar.push((site, trimmed.to_string()));
    if jar.len() > 25 {
        jar.remove(0);
    }
}

pub fn remembered_site_cookies(page_url: &str) -> Option<String> {
    let site = canonical_cookie_site(page_url);
    let jar = SITE_COOKIE_JAR.lock().unwrap_or_else(|p| p.into_inner());
    jar.iter()
        .find(|(s, _)| *s == site)
        .map(|(_, c)| c.clone())
}

/// عنصر فيديو واحد داخل قائمة تشغيل (Playlist)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistEntry {
    pub title: String,
    pub url: String,
    pub duration_secs: u64,
}

/// معلومات قائمة التشغيل المستخرجة عبر `--flat-playlist`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistInfo {
    pub title: String,
    pub entries: Vec<PlaylistEntry>,
}

/// تحليل مخرجات JSON الخاصة بقائمة التشغيل من `yt-dlp -J --flat-playlist` مع استبعاد الفيديوهات المحذوفة أو الخاصة
pub fn parse_playlist_json(json: &serde_json::Value) -> PlaylistInfo {
    let title = json
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Playlist")
        .to_string();
    let mut entries = Vec::new();
    let list = json
        .get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if list.is_empty() && json.get("_type").and_then(|v| v.as_str()) != Some("playlist") {
        if let Some(u) = json
            .get("webpage_url")
            .and_then(|v| v.as_str())
            .or_else(|| json.get("original_url").and_then(|v| v.as_str()))
        {
            let dur = json
                .get("duration")
                .and_then(|d| d.as_u64().or_else(|| d.as_f64().map(|f| f as u64)))
                .unwrap_or(0);
            entries.push(PlaylistEntry {
                title: title.clone(),
                url: u.to_string(),
                duration_secs: dur,
            });
        }
    }

    for e in list {
        let url = e
            .get("url")
            .and_then(|u| u.as_str())
            .filter(|u| u.starts_with("http"))
            .map(String::from)
            .or_else(|| e.get("webpage_url").and_then(|u| u.as_str()).map(String::from))
            .or_else(|| {
                e.get("id")
                    .and_then(|id| id.as_str())
                    .map(|id| format!("https://www.youtube.com/watch?v={id}"))
            });
        let Some(url) = url else { continue };
        let t = e.get("title").and_then(|v| v.as_str()).unwrap_or("video");
        if t == "[Deleted video]" || t == "[Private video]" {
            continue;
        }
        let duration_secs = e
            .get("duration")
            .and_then(|d| d.as_u64().or_else(|| d.as_f64().map(|f| f as u64)))
            .unwrap_or(0);
        entries.push(PlaylistEntry {
            title: t.to_string(),
            url,
            duration_secs,
        });
    }

    PlaylistInfo { title, entries }
}

/// استخراج عناصر قائمة التشغيل بسرعة عبر `yt-dlp --flat-playlist --yes-playlist` (يجرّب بدون كوكيز أولاً ثم بالكوكيز عند الحاجة)
pub async fn fetch_playlist_flat(url: &str, cookies: Option<&str>) -> Result<PlaylistInfo> {
    let safe_url = crate::core::tools::validate_web_url_for_tools(url)?;
    if let Some(c) = cookies.filter(|s| !s.trim().is_empty()) {
        remember_site_cookies(&safe_url, c);
    }
    let ytdlp_bin = find_ytdlp().unwrap_or_else(|| PathBuf::from("yt-dlp"));

    let run_once = |cookie_str: Option<String>| {
        let bin = ytdlp_bin.clone();
        let target = safe_url.clone();
        async move {
            let mut cmd = TokioCommand::new(&bin);
            hide_tokio_console_window(&mut cmd);
            if let Some(work_dir) = crate::core::tools::writable_tool_dir("yt-dlp") {
                cmd.env("TEMP", &work_dir).env("TMP", &work_dir);
            }
            cmd.kill_on_drop(true).args([
                "--ignore-config",
                "-J",
                "--flat-playlist",
                "--yes-playlist",
                "--no-warnings",
                "--no-cache-dir",
                "--socket-timeout",
                "20",
            ]);
            let _guard = if let Some(ref c) = cookie_str {
                if let Ok(g) = create_temp_netscape_cookie_file(&target, c) {
                    cmd.arg("--cookies").arg(&g.0);
                    Some(g)
                } else {
                    None
                }
            } else {
                None
            };
            cmd.arg("--").arg(&target);
            let out = tokio::time::timeout(Duration::from_secs(25), cmd.output())
                .await
                .context("انتهت مهلة استخراج قائمة التشغيل")?
                .context("تعذر تشغيل yt-dlp لاستخراج قائمة التشغيل")?;
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr).to_string();
                bail!("{err}");
            }
            let val: serde_json::Value =
                serde_json::from_slice(&out.stdout).context("مخرجات JSON غير صالحة لقائمة التشغيل")?;
            Ok(parse_playlist_json(&val))
        }
    };

    match run_once(None).await {
        Ok(info) => Ok(info),
        Err(e) if needs_login_or_cookies(&e.to_string()) => {
            let fallback_cookies = cookies
                .map(String::from)
                .or_else(|| remembered_site_cookies(&safe_url))
                .filter(|s| !s.trim().is_empty());
            if let Some(c) = fallback_cookies {
                run_once(Some(c)).await
            } else {
                Err(e)
            }
        }
        Err(e) => Err(e),
    }
}
