/**
 * StormGet (RustIDM v2.3.0) — YouTube Direct Stream & SABR Capture Engine (`yt-core.js`)
 * يجمع بين:
 * 1. التقاط روابط `*.googlevideo.com/videoplayback` المفكوكة مباشرة مع فحص `sparams` قبل حذف `range` (تجنباً لكسر التوقيع 403).
 * 2. دعم كامل لبروتوكول يوتيوب الحديث SABR (`sabr=1` + `ytInitialPlayerResponse` + `/youtubei/v1/player` + `poToken` seed)
 *    المستلهم من `awesome-downloader` (`youtube-media.js` و `core/sabr.mjs`).
 * 3. استبعاد المسارات الصوتية المدبلجة أو المضغوطة (`!isDrc && !isVb && audioIsDefault`) ودمج الصوت والفيديو عبر `direct_mux` أو `sabr`.
 */

const MAX_URL_LEN = 16384;
const MAX_FORMATS_COUNT = 150;

const YT_ITAGS = {
  // مسارات مدمجة (فيديو + صوت في ملف واحد)
  18: { kind: "av", h: 360, ext: "mp4" },
  22: { kind: "av", h: 720, ext: "mp4" },
  // H.264 (MP4)
  160: { kind: "v", h: 144, ext: "mp4" },
  133: { kind: "v", h: 240, ext: "mp4" },
  134: { kind: "v", h: 360, ext: "mp4" },
  135: { kind: "v", h: 480, ext: "mp4" },
  136: { kind: "v", h: 720, ext: "mp4" },
  137: { kind: "v", h: 1080, ext: "mp4" },
  298: { kind: "v", h: 720, fps: 60, ext: "mp4" },
  299: { kind: "v", h: 1080, fps: 60, ext: "mp4" },
  264: { kind: "v", h: 1440, ext: "mp4" },
  266: { kind: "v", h: 2160, ext: "mp4" },
  // AV1 (MP4)
  394: { kind: "v", h: 144, ext: "mp4", codec: "AV1" },
  395: { kind: "v", h: 240, ext: "mp4", codec: "AV1" },
  396: { kind: "v", h: 360, ext: "mp4", codec: "AV1" },
  397: { kind: "v", h: 480, ext: "mp4", codec: "AV1" },
  398: { kind: "v", h: 720, ext: "mp4", codec: "AV1" },
  399: { kind: "v", h: 1080, ext: "mp4", codec: "AV1" },
  400: { kind: "v", h: 1440, ext: "mp4", codec: "AV1" },
  401: { kind: "v", h: 2160, ext: "mp4", codec: "AV1" },
  // VP9 (WebM)
  278: { kind: "v", h: 144, ext: "webm" },
  242: { kind: "v", h: 240, ext: "webm" },
  243: { kind: "v", h: 360, ext: "webm" },
  244: { kind: "v", h: 480, ext: "webm" },
  247: { kind: "v", h: 720, ext: "webm" },
  248: { kind: "v", h: 1080, ext: "webm" },
  271: { kind: "v", h: 1440, ext: "webm" },
  313: { kind: "v", h: 2160, ext: "webm" },
  302: { kind: "v", h: 720, fps: 60, ext: "webm" },
  303: { kind: "v", h: 1080, fps: 60, ext: "webm" },
  308: { kind: "v", h: 1440, fps: 60, ext: "webm" },
  315: { kind: "v", h: 2160, fps: 60, ext: "webm" },
  // مسارات صوتية فقط (Audio Only)
  139: { kind: "a", abr: 48, ext: "m4a" },
  140: { kind: "a", abr: 128, ext: "m4a" },
  141: { kind: "a", abr: 256, ext: "m4a" },
  249: { kind: "a", abr: 50, ext: "webm" },
  250: { kind: "a", abr: 70, ext: "webm" },
  251: { kind: "a", abr: 160, ext: "webm" }
};

// المعلمات الخاصة بالقطعة الصغيرة التي يطلبها المشغل؛ نحذفها للحصول على الملف الكامل
const PIECE_PARAMS = ["range", "rn", "rbuf", "ump", "srfvp", "alr"];

const boundedText = (value, max = 200) => String(value || "").slice(0, max);
const boundedInt = (value) => {
  const n = Number(value);
  return Number.isSafeInteger(n) && n >= 0 ? n : 0;
};
const boundedDecimal = (value, max = 32) =>
  /^\d+$/.test(String(value || "")) ? String(value).slice(0, max) : "";

/**
 * فحص وتطبيع روابط googlevideo.com/videoplayback (سواء المباشرة أو SABR)
 * مع التحقق من عدم وجود `range` ضمن `sparams` الموقعة قبل حذف `range`
 */
function googleVideoUrl(value, requireSabr = false) {
  try {
    const u = new URL(value);
    if (
      u.protocol !== "https:" ||
      !(u.hostname === "googlevideo.com" || u.hostname.endsWith(".googlevideo.com")) ||
      u.pathname !== "/videoplayback" ||
      u.username ||
      u.password ||
      u.href.length > MAX_URL_LEN
    ) {
      return null;
    }
    if (requireSabr) {
      return u.searchParams.get("sabr") === "1" ? u.href : null;
    }
    if (
      u.searchParams.has("sq") ||
      u.searchParams.get("source") === "yt_live_broadcast" ||
      u.searchParams.get("sabr") === "1"
    ) {
      return null;
    }
    if (u.searchParams.has("range")) {
      const signedParams = (u.searchParams.get("sparams") || "").split(",");
      if (signedParams.includes("range")) return null;
      u.searchParams.delete("range");
    }
    for (const k of ["rn", "rbuf", "ump", "srfvp", "alr"]) {
      const signedParams = (u.searchParams.get("sparams") || "").split(",");
      if (!signedParams.includes(k)) {
        u.searchParams.delete(k);
      }
    }
    return u.href;
  } catch {
    return null;
  }
}

const mediaUrl = (value) => googleVideoUrl(value, false);
const sabrUrl = (value) => googleVideoUrl(value, true);

/**
 * تحليل رابط videoplayback ملتقط وتحويله إلى وصف مسار كامل، أو null إذا لم يكن صالحاً
 */
function parseStream(rawUrl) {
  let u;
  try {
    u = new URL(rawUrl);
  } catch {
    return null;
  }
  if (!/(^|\.)googlevideo\.com$/i.test(u.hostname) || u.pathname !== "/videoplayback") {
    return null;
  }
  const p = u.searchParams;
  if (p.get("sabr") === "1") {
    const validSabr = sabrUrl(rawUrl);
    return validSabr ? { sabr: true, url: validSabr } : null;
  }
  const cleanHref = mediaUrl(rawUrl);
  if (!cleanHref) return null;
  const cleanUrlObj = new URL(cleanHref);
  const cp = cleanUrlObj.searchParams;

  const itag = Number(cp.get("itag"));
  if (!itag) return null;

  const mime = (cp.get("mime") || "").toLowerCase();
  let info = YT_ITAGS[itag];
  if (!info) {
    if (mime.startsWith("audio/")) {
      info = { kind: "a", abr: 0, ext: mime.includes("webm") ? "webm" : "m4a" };
    } else if (mime.startsWith("video/")) {
      info = { kind: "v", h: 0, ext: mime.includes("webm") ? "webm" : "mp4" };
    } else {
      return null;
    }
  }

  return {
    itag,
    url: cleanUrlObj.toString(),
    mime,
    size: Number(cp.get("clen")) || 0,
    dur: Math.round(Number(cp.get("dur")) || 0),
    ...info
  };
}

/**
 * تطهير عنصر تنسيق واحد من استجابة مشغل يوتيوب (streamingData.formats / adaptiveFormats)
 */
function cleanFormat(f) {
  if (
    !f ||
    typeof f !== "object" ||
    Array.isArray(f) ||
    (Array.isArray(f.drmFamilies) && f.drmFamilies.length > 0) ||
    f.drmTrackType
  ) {
    return null;
  }
  const cipher = new URLSearchParams(
    typeof f.signatureCipher === "string" ? f.signatureCipher : ""
  );
  const mimeType = boundedText(f.mimeType, 200);
  const audioTrack =
    f.audioTrack && typeof f.audioTrack === "object"
      ? {
          id: boundedText(f.audioTrack.id, 100),
          audioIsDefault: Boolean(f.audioTrack.audioIsDefault)
        }
      : null;
  const colorInfo =
    f.colorInfo && typeof f.colorInfo === "object"
      ? {
          transferCharacteristics: boundedText(f.colorInfo.transferCharacteristics, 80)
        }
      : undefined;

  return {
    itag: boundedInt(f.itag),
    url: mediaUrl(f.url),
    sourceUrl: mediaUrl(f.url || cipher.get("url")),
    mimeType,
    width: boundedInt(f.width),
    height: boundedInt(f.height),
    fps: boundedInt(f.fps),
    bitrate: boundedInt(f.bitrate),
    averageBitrate: boundedInt(f.averageBitrate),
    audioQuality: boundedText(f.audioQuality, 80),
    audioTrack,
    lastModified: boundedDecimal(f.lastModified),
    xtags: boundedText(f.xtags, 2048),
    contentLength: boundedDecimal(f.contentLength),
    approxDurationMs: boundedDecimal(f.approxDurationMs),
    quality: boundedText(f.quality, 80),
    qualityLabel: boundedText(f.qualityLabel, 80),
    audioChannels: boundedInt(f.audioChannels),
    targetDurationSec: Number.isFinite(Number(f.targetDurationSec))
      ? Number(f.targetDurationSec)
      : undefined,
    isDrc: Boolean(f.isDrc),
    isVb: Boolean(f.isVb),
    colorInfo
  };
}

/**
 * تطهير والتحقق من بيانات مشغل يوتيوب الملتقطة من الصفحة
 */
function sanitizePlayer(value) {
  if (
    !value ||
    !/^[\w-]{11}$/.test(value.videoId || "") ||
    !Array.isArray(value.formats) ||
    value.formats.length > MAX_FORMATS_COUNT
  ) {
    return null;
  }
  const formats = value.formats
    .map(cleanFormat)
    .filter((f) => f && f.itag && f.mimeType);
  const url = sabrUrl(value.sabrUrl);
  const config = /^[A-Za-z0-9_+\/=.-]{1,131072}$/.test(String(value.ustreamerConfig || ""))
    ? String(value.ustreamerConfig)
    : "";
  const clientName = boundedInt(value.clientName) || 1;
  const clientVersion = /^[\w.-]{1,80}$/.test(String(value.clientVersion || ""))
    ? String(value.clientVersion)
    : "";
  return {
    videoId: value.videoId,
    title: boundedText(value.title || "Video", 180),
    sabr: Boolean(value.sabr) || Boolean(url),
    sabrUrl: url,
    ustreamerConfig: config,
    clientName,
    clientVersion,
    formats
  };
}

/**
 * تحويل كائن ytInitialPlayerResponse أو استجابة /youtubei/v1/player الخام إلى بنية مطهرة
 */
function playerData(value, ytcfgObj = null) {
  if (value && value.videoId && Array.isArray(value.formats)) {
    return sanitizePlayer(value);
  }
  if (
    !value ||
    !/^[\w-]{11}$/.test(value.videoDetails?.videoId || "") ||
    value.videoDetails?.isLiveContent
  ) {
    return null;
  }
  const formats = [
    ...(Array.isArray(value.streamingData?.formats) ? value.streamingData.formats : []),
    ...(Array.isArray(value.streamingData?.adaptiveFormats)
      ? value.streamingData.adaptiveFormats
      : [])
  ];
  const ytcfg = ytcfgObj || (typeof globalThis !== "undefined" ? globalThis.ytcfg : null);
  return sanitizePlayer({
    videoId: value.videoDetails.videoId,
    title: value.videoDetails.title,
    sabr: Boolean(value.streamingData?.serverAbrStreamingUrl),
    sabrUrl: value.streamingData?.serverAbrStreamingUrl,
    ustreamerConfig:
      value.playerConfig?.mediaCommonConfig?.mediaUstreamerRequestConfig
        ?.videoPlaybackUstreamerConfig,
    clientName:
      ytcfg?.get?.("INNERTUBE_CONTEXT_CLIENT_NAME") ||
      ytcfg?.data_?.INNERTUBE_CONTEXT_CLIENT_NAME ||
      1,
    clientVersion:
      ytcfg?.get?.("INNERTUBE_CONTEXT_CLIENT_VERSION") ||
      ytcfg?.data_?.INNERTUBE_CONTEXT_CLIENT_VERSION ||
      "",
    formats
  });
}

/**
 * استخراج المعرف الفريد لفيديو يوتيوب من رابط الصفحة (watch?v=, /shorts/, /embed/, youtu.be)
 */
function videoIdOf(pageUrl) {
  try {
    const u = new URL(pageUrl);
    if (u.hostname === "youtu.be") return u.pathname.slice(1) || null;
    if (u.searchParams.get("v")) return u.searchParams.get("v");
    const m = /^\/(shorts|embed|live)\/([\w-]{6,})/.exec(u.pathname);
    return m ? m[2] : null;
  } catch {
    return null;
  }
}

function fmtSize(n) {
  if (!n) return "";
  const u = ["B", "KB", "MB", "GB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(i ? 1 : 0)} ${u[i]}`;
}

/**
 * بناء خيارات التحميل من كافة المسارات الملتقطة للتبويب،
 * مع استبعاد الإعلانات القصيرة تلقائياً عبر اختيار المجموعة صاحبة أطول مدة زمنية (dur).
 */
function buildOptions(streams) {
  const list = Object.values(streams || {});
  if (!list.length) return [];

  const groups = new Map();
  for (const s of list) {
    const key = s.dur || 0;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(s);
  }
  const mainDur = [...groups.keys()].sort(
    (a, b) => b - a || groups.get(b).length - groups.get(a).length
  )[0];
  const main = groups.get(mainDur);

  const audios = main
    .filter((s) => s.kind === "a")
    .sort((a, b) => (b.abr || 0) - (a.abr || 0) || b.size - a.size);
  const bestAudio = (ext) =>
    audios.find((a) => (ext === "webm" ? a.ext === "webm" : a.ext === "m4a"));

  const options = [];
  const seen = new Set();
  const videos = main
    .filter((s) => s.kind === "v" || s.kind === "av")
    .sort(
      (a, b) =>
        (b.h || 0) - (a.h || 0) ||
        (b.fps || 30) - (a.fps || 30) ||
        b.size - a.size
    );

  for (const v of videos) {
    const q = `${v.h ? v.h + "p" : "video"}${v.fps && v.fps > 30 ? v.fps : ""}`;
    const fmt = v.codec ? `${v.ext.toUpperCase()} ${v.codec}` : v.ext.toUpperCase();
    const key = `${q}|${fmt}`;
    if (seen.has(key)) continue;
    seen.add(key);

    if (v.kind === "av") {
      options.push({
        label: `${q} · ${fmt} (فوري مباشر)`,
        sub: `${fmtSize(v.size)} · video with sound`,
        quality: q,
        videoUrl: v.url,
        audioUrl: null,
        ext: v.ext,
        size: v.size,
        protocol: "direct"
      });
      continue;
    }

    let audio = bestAudio(v.ext);
    let ext = v.ext;
    if (!audio && audios.length) {
      audio = audios[0];
      ext = "mkv";
    }
    if (!audio) continue;

    options.push({
      label: `${q} · ${ext === "mkv" ? "MKV" : fmt} (فوري متعدد الاتصالات)`,
      sub: fmtSize(v.size + audio.size),
      quality: q,
      videoUrl: v.url,
      audioUrl: audio.url,
      ext,
      size: v.size + audio.size,
      protocol: "direct_mux"
    });
  }

  const audioOnly = bestAudio("m4a") || audios[0];
  if (audioOnly) {
    options.push({
      label: `Audio only · ${audioOnly.ext.toUpperCase()}${
        audioOnly.abr ? " " + audioOnly.abr + "k" : ""
      } (صوت مباشر)`,
      sub: fmtSize(audioOnly.size),
      quality: "audio",
      videoUrl: audioOnly.url,
      audioUrl: null,
      ext: audioOnly.ext,
      size: audioOnly.size,
      protocol: "direct"
    });
  }

  return options;
}

/**
 * بناء خيارات يوتيوب الموحدة (المسارات المباشرة + جلسات SABR الحديثة)
 * مستلهم من `awesome-downloader/extension/youtube-media.js:choices()`
 */
function buildSabrAndDirectChoices(player, observedUrls = [], sabrCapture = null) {
  if (!player || !Array.isArray(player.formats)) return [];

  const effectiveSabrUrl = sabrUrl(sabrCapture?.url || player.sabrUrl);
  const requestBody = sabrCapture?.requestBody || undefined;

  // ربط المسارات الملتقطة فعلياً بالـ itag و id، واستبعاد الروابط المنتهية أو المحجوبة بـ SABR
  const hydratedFormats = player.formats
    .map((f) => {
      let sourceUrlObj = null;
      try {
        if (f.sourceUrl) sourceUrlObj = new URL(f.sourceUrl);
      } catch {}
      const match =
        sourceUrlObj &&
        observedUrls.find((raw) => {
          try {
            const u = new URL(raw);
            return (
              u.searchParams.get("itag") === String(f.itag) &&
              Boolean(sourceUrlObj.searchParams.get("id")) &&
              u.searchParams.get("id") === sourceUrlObj.searchParams.get("id")
            );
          } catch {
            return false;
          }
        });
      // مع تفعيل SABR، الروابط غير الملتقطة مباشرة من الشبكة تعيد 403؛ لذا نفضل SABR ما لم يُلتقط الرابط بنجاح
      return {
        ...f,
        url: match || (effectiveSabrUrl && player.ustreamerConfig ? null : f.url)
      };
    })
    .filter((f) => {
      if (!f.url) return false;
      try {
        const u = new URL(f.url);
        return (
          !u.searchParams.has("expire") ||
          Number(u.searchParams.get("expire")) * 1000 > Date.now() + 30000
        );
      } catch {
        return false;
      }
    });

  const options = [];
  const seen = new Set();

  // 1. المسارات المباشرة المؤكدة (Direct / Direct Mux)
  const directAudios = hydratedFormats
    .filter(
      (f) =>
        /^audio\/mp4/i.test(f.mimeType) &&
        (!f.audioTrack || f.audioTrack.audioIsDefault) &&
        !f.isDrc &&
        !f.isVb
    )
    .sort((a, b) => (b.bitrate || 0) - (a.bitrate || 0));
  const bestDirectAudio = directAudios[0];

  const directVideos = hydratedFormats
    .filter((f) => /^video\/mp4/i.test(f.mimeType) && f.height)
    .sort(
      (a, b) =>
        b.height - a.height || (b.fps || 30) - (a.fps || 30) || b.bitrate - a.bitrate
    );

  for (const f of directVideos) {
    const muxed = Boolean(f.audioQuality) || /mp4a/i.test(f.mimeType);
    if (!muxed && !bestDirectAudio) continue;
    const key = `${f.height}:${f.fps || 30}`;
    if (seen.has(key)) continue;
    seen.add(key);

    const vSize = Number(f.contentLength) || 0;
    const aSize = muxed ? 0 : Number(bestDirectAudio.contentLength) || 0;
    const totalSize = vSize + aSize;
    const qLabel = `${f.height}p${f.fps > 30 ? f.fps : ""}`;

    options.push({
      label: `MP4 ${qLabel} (${muxed ? "فوري مباشر" : "فوري متعدد الاتصالات"})`,
      sub: fmtSize(totalSize),
      quality: qLabel,
      videoUrl: f.url,
      audioUrl: muxed ? null : bestDirectAudio.url,
      ext: "mp4",
      size: totalSize,
      bitrate: f.bitrate || null,
      protocol: muxed ? "direct" : "direct_mux",
      formatId: muxed ? String(f.itag) : `${f.itag}+${bestDirectAudio.itag}`,
      sabrSession: null
    });
  }

  if (bestDirectAudio) {
    const aSize = Number(bestDirectAudio.contentLength) || 0;
    options.push({
      label: "Audio Only · M4A/MP3 (صوت مباشر فوري)",
      sub: fmtSize(aSize),
      quality: "audio",
      videoUrl: bestDirectAudio.url,
      audioUrl: null,
      ext: "m4a",
      size: aSize,
      bitrate: bestDirectAudio.bitrate || null,
      protocol: "direct",
      formatId: String(bestDirectAudio.itag),
      sabrSession: null
    });
  }

  // 2. مسارات بروتوكول يوتيوب الحديث SABR (Server ABR Streaming)
  if (!effectiveSabrUrl || !player.ustreamerConfig) {
    return options;
  }

  const sabrAudios = player.formats
    .filter(
      (f) =>
        /^audio\/(mp4|webm)/i.test(f.mimeType) &&
        (!f.audioTrack || f.audioTrack.audioIsDefault) &&
        !f.isDrc &&
        !f.isVb
    )
    .sort((a, b) => {
      const aMp4 = /^audio\/mp4/i.test(a.mimeType) ? 1 : 0;
      const bMp4 = /^audio\/mp4/i.test(b.mimeType) ? 1 : 0;
      return bMp4 - aMp4 || (b.bitrate || 0) - (a.bitrate || 0);
    });
  const sabrAudio = sabrAudios[0];
  if (!sabrAudio) return options;

  const sabrVideos = player.formats
    .filter(
      (f) =>
        /^video\/(mp4|webm)/i.test(f.mimeType) &&
        f.height &&
        !f.audioQuality &&
        !/mp4a/i.test(f.mimeType)
    )
    .sort((a, b) => {
      const aMp4 = /^video\/mp4/i.test(a.mimeType) ? 1 : 0;
      const bMp4 = /^video\/mp4/i.test(b.mimeType) ? 1 : 0;
      return (
        b.height - a.height ||
        (b.fps || 30) - (a.fps || 30) ||
        bMp4 - aMp4 ||
        (b.bitrate || 0) - (a.bitrate || 0)
      );
    });

  for (const f of sabrVideos) {
    const key = `${f.height}:${f.fps || 30}`;
    if (seen.has(key)) continue;
    seen.add(key);

    const vSize = Number(f.contentLength) || 0;
    const aSize = Number(sabrAudio.contentLength) || 0;
    const totalSize = vSize > 0 && aSize > 0 ? vSize + aSize : 0;
    const qLabel = `${f.height}p${f.fps > 30 ? f.fps : ""}`;

    options.push({
      label: `MP4 ${qLabel} (SABR دفق يوتيوب المحصّن)`,
      sub: fmtSize(totalSize),
      quality: qLabel,
      videoUrl: effectiveSabrUrl,
      audioUrl: null,
      ext: "mp4",
      size: totalSize,
      bitrate: f.bitrate || null,
      protocol: "sabr",
      formatId: `${f.itag}+${sabrAudio.itag}`,
      sabrSession: {
        videoId: player.videoId,
        serverAbrStreamingUrl: effectiveSabrUrl,
        videoPlaybackUstreamerConfig: player.ustreamerConfig,
        clientInfo: {
          clientName: player.clientName || 1,
          clientVersion: player.clientVersion || ""
        },
        videoItag: f.itag,
        audioItag: sabrAudio.itag,
        requestBody: requestBody || null,
        formats: [f, sabrAudio]
      }
    });
  }

  if (!options.some((o) => o.quality === "audio")) {
    const aSize = Number(sabrAudio.contentLength) || 0;
    options.push({
      label: "Audio Only · MP3/M4A (SABR صوت يوتيوب)",
      sub: fmtSize(aSize),
      quality: "audio",
      videoUrl: effectiveSabrUrl,
      audioUrl: null,
      ext: "m4a",
      size: aSize,
      bitrate: sabrAudio.bitrate || null,
      protocol: "sabr",
      formatId: `audio:${sabrAudio.itag}`,
      sabrSession: {
        videoId: player.videoId,
        serverAbrStreamingUrl: effectiveSabrUrl,
        videoPlaybackUstreamerConfig: player.ustreamerConfig,
        clientInfo: {
          clientName: player.clientName || 1,
          clientVersion: player.clientVersion || ""
        },
        videoItag: null,
        audioItag: sabrAudio.itag,
        requestBody: requestBody || null,
        formats: [sabrAudio]
      }
    });
  }

  return options;
}

/**
 * تنظيف عنوان صفحة يوتيوب ليصلح كاسم ملف آمن
 */
function cleanYoutubeTitle(title) {
  return (
    (title || "video")
      .replace(/^\(\d+\)\s*/, "")
      .replace(/\s*-\s*YouTube\s*$/i, "")
      .replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_")
      .trim()
      .slice(0, 120) || "video"
  );
}

if (typeof globalThis !== "undefined") {
  globalThis.stormgetYtCore = {
    YT_ITAGS,
    googleVideoUrl,
    mediaUrl,
    sabrUrl,
    parseStream,
    cleanFormat,
    sanitizePlayer,
    playerData,
    videoIdOf,
    buildOptions,
    buildSabrAndDirectChoices,
    cleanYoutubeTitle,
    fmtSize
  };
}

if (typeof module !== "undefined") {
  module.exports = {
    YT_ITAGS,
    googleVideoUrl,
    mediaUrl,
    sabrUrl,
    parseStream,
    cleanFormat,
    sanitizePlayer,
    playerData,
    videoIdOf,
    buildOptions,
    buildSabrAndDirectChoices,
    cleanYoutubeTitle,
    fmtSize
  };
}
