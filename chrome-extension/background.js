/**
 * StormGet (RustIDM v2.3.0) Integration Module — Background Service Worker (`background.js`)
 * يطبق:
 * 1. التقاط جلسات يوتيوب الحديثة `SABR` (`sabr=1` + `requestBody` Protobuf Seed + `ytInitialPlayerResponse`)
 *    بالإضافة إلى روابط `googlevideo.com/videoplayback` المباشرة عبر `yt-core.js` (مستلهم من `awesome-downloader`).
 * 2. إرسال الترويسة الأمنية الإلزامية (`X-RustIDM: 1`) والتوكن المحلي في اتصالات المنفذ المحلي 16888.
 * 3. اعتراض التحميلات بآلية التسليم الآمن (Pause -> Handoff -> Cancel or Resume on Failure كما في IDM و Awesome Downloader).
 * 4. السقوط التلقائي الآمن (Automatic Fallback) إلى بروتوكول Chrome Native Messaging Host (`com.rustidm.native_host`)
 *    الذي يقوم بتشغيل محرك سطح المكتب تلقائياً عند الحاجة.
 */

try {
  importScripts("yt-core.js");
} catch (_) {
  // في بيئة Firefox أو الاختبار حيث يتم تحميل yt-core.js بشكل منفصل
}

const RUSTIDM_API_BASE = "http://127.0.0.1:16888/api/v1";
const NATIVE_HOST_NAME = "com.rustidm.native_host";
const IPC_HEADERS = {
  "Content-Type": "application/json",
  "X-RustIDM": "1"
};

// جلب رمز الجلسة المشفر (256-bit bridge.json token) من مضيف المراسلة الأصلي (Native Messaging Host)
// وإرفاقه تلقائياً في ترويسة Authorization: Bearer لجميع طلبات المنفذ المحلي 16888
function refreshBridgeTokenFromNativeHost() {
  try {
    chrome.runtime.sendNativeMessage(
      NATIVE_HOST_NAME,
      { action: "PING" },
      (resp) => {
        if (!chrome.runtime.lastError && resp && typeof resp.task_id === "string" && resp.task_id.length >= 32) {
          IPC_HEADERS["Authorization"] = `Bearer ${resp.task_id}`;
        }
      }
    );
  } catch (_) {}
}
refreshBridgeTokenFromNativeHost();

// تخزين روابط البث العام المكتشفة لكل تبويب: tabId -> Map<url, StreamEntry>
const tabStreams = new Map();

// تخزين مسارات videoplayback المباشرة المفكوكة من مشغل يوتيوب لكل تبويب: tabId -> { videoId, streams: { [itag]: parsedStream }, observedUrls: string[] }
const tabYoutubeStreams = new Map();

// تخزين بيانات استجابة مشغل يوتيوب المطهرة (ytInitialPlayerResponse / /youtubei/v1/player) لكل تبويب: tabId -> cleanPlayer
const youtubePlayers = new Map();

// تخزين بذور وجلسات يوتيوب SABR الملتقطة من webRequest.onBeforeRequest (requestBody Protobuf):
// requestId -> { tabId, url, requestBody } و tabId -> { url, requestBody }
const youtubeSabrSeed = new Map();
const youtubeSabrObserved = new Map();

function clearSabrCaptureForTab(tabId) {
  youtubeSabrObserved.delete(tabId);
  for (const [reqId, seed] of youtubeSabrSeed.entries()) {
    if (seed.tabId === tabId) {
      youtubeSabrSeed.delete(reqId);
    }
  }
}

function bytesToBase64(bytes) {
  let binary = "";
  const view = new Uint8Array(bytes);
  for (let i = 0; i < view.length; i += 8192) {
    binary += String.fromCharCode(...view.subarray(i, i + 8192));
  }
  return btoa(binary);
}

// التقاط جسم طلب SABR الأولي (Protobuf VideoPlaybackAbrRequest الذي يحوي poToken و clientInfo)
if (
  typeof chrome !== "undefined" &&
  chrome.webRequest &&
  chrome.webRequest.onBeforeRequest
) {
  try {
    chrome.webRequest.onBeforeRequest.addListener(
      (details) => {
        if (details.tabId < 0) return;
        const validSabr =
          typeof sabrUrl === "function" ? sabrUrl(details.url) : null;
        const rawParts = details.requestBody && details.requestBody.raw;
        if (!validSabr || !Array.isArray(rawParts) || rawParts.length === 0) {
          return;
        }
        let totalSize = 0;
        for (const part of rawParts) {
          if (!part.bytes) return;
          totalSize += part.bytes.byteLength;
        }
        if (totalSize === 0 || totalSize > 262144) return;
        const combined = new Uint8Array(totalSize);
        let offset = 0;
        for (const part of rawParts) {
          combined.set(new Uint8Array(part.bytes), offset);
          offset += part.bytes.byteLength;
        }
        youtubeSabrSeed.set(details.requestId, {
          tabId: details.tabId,
          url: validSabr,
          requestBody: bytesToBase64(combined)
        });
        if (youtubeSabrSeed.size > 100) {
          youtubeSabrSeed.delete(youtubeSabrSeed.keys().next().value);
        }
      },
      {
        urls: [
          "https://*.googlevideo.com/videoplayback*",
          "https://googlevideo.com/videoplayback*"
        ]
      },
      ["requestBody"]
    );
  } catch (_) {}
}

const MEDIA_EXTENSIONS = [
  "3gp", "7z", "aac", "ace", "aif", "apk", "arj", "asf", "avi", "bin",
  "bz2", "deb", "dmg", "doc", "docx", "epub", "exe", "flac", "flv", "gz",
  "gzip", "img", "iso", "lzh", "m4a", "m4v", "mkv", "mov", "mp3", "mp4",
  "mpa", "mpe", "mpeg", "mpg", "msi", "msu", "ogg", "ogv", "pdf", "plj",
  "pps", "ppt", "pptx", "qt", "ra", "rar", "rm", "rmvb", "rpm", "sea",
  "sit", "sitx", "tar", "tif", "tiff", "wav", "webm", "wma", "wmv", "xls",
  "xlsx", "xz", "z", "zip", "m3u8", "mpd"
];

const MEDIA_EXTENSION_SET = new Set(MEDIA_EXTENSIONS);

const ARCHIVE_AND_BINARY_MIME_TYPES = new Set([
  "application/zip",
  "application/x-zip-compressed",
  "application/x-zip",
  "application/x-7z-compressed",
  "application/x-rar-compressed",
  "application/x-rar",
  "application/x-tar",
  "application/x-bzip2",
  "application/x-bzip",
  "application/gzip",
  "application/x-gzip",
  "application/x-msdownload",
  "application/x-executable",
  "application/vnd.android.package-archive"
]);

function extractExtensionFromText(text) {
  if (!text) return "";
  const clean = text.split(/[?#]/)[0].trim().toLowerCase();
  const lastDot = clean.lastIndexOf(".");
  if (lastDot === -1) return "";
  return clean.slice(lastDot + 1);
}

function hasInterceptableExtension(url, filename = null, mime = null) {
  if (filename) {
    const fnExt = extractExtensionFromText(filename);
    if (MEDIA_EXTENSION_SET.has(fnExt) || /^r[01]\d*$/.test(fnExt)) {
      return true;
    }
  }

  if (url) {
    try {
      const urlObj = new URL(url);
      const pathExt = extractExtensionFromText(urlObj.pathname);
      if (MEDIA_EXTENSION_SET.has(pathExt) || /^r[01]\d*$/.test(pathExt)) {
        return true;
      }
      for (const [_, val] of urlObj.searchParams.entries()) {
        const queryExt = extractExtensionFromText(val);
        if (MEDIA_EXTENSION_SET.has(queryExt) || /^r[01]\d*$/.test(queryExt)) {
          return true;
        }
      }
    } catch {
      const fallbackExt = extractExtensionFromText(url);
      if (MEDIA_EXTENSION_SET.has(fallbackExt)) return true;
    }
  }

  if (mime) {
    const mimeLower = mime.toLowerCase();
    if (ARCHIVE_AND_BINARY_MIME_TYPES.has(mimeLower)) {
      return true;
    }
    if (
      (mimeLower.startsWith("video/") || mimeLower.startsWith("audio/")) &&
      !mimeLower.includes("html") &&
      !mimeLower.includes("javascript")
    ) {
      return true;
    }
  }

  return false;
}

const VIDEO_CONTENT_TYPES = [
  "application/vnd.apple.mpegurl",
  "application/x-mpegurl",
  "application/dash+xml",
  "video/mp4",
  "video/webm",
  "video/x-matroska",
  "video/x-flv",
  "video/quicktime",
  "video/MP2T"
];

function isSegmentFragmentUrl(urlLower) {
  if (urlLower.endsWith(".ts") || urlLower.endsWith(".m4s") || urlLower.includes(".m4s?")) {
    return true;
  }
  if (
    /seg-\d+|segment-\d+|chunk-\d+|frag-\d+/i.test(urlLower) &&
    !urlLower.includes(".m3u8") &&
    !urlLower.includes(".mpd")
  ) {
    return true;
  }
  return false;
}

async function getCookiesHeaderForUrl(url) {
  try {
    if (!url || !url.startsWith("http")) return "";
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch (_) {
    return "";
  }
}

function sendViaNativeMessagingHost(payload) {
  return new Promise((resolve, reject) => {
    try {
      chrome.runtime.sendNativeMessage(NATIVE_HOST_NAME, payload, (response) => {
        if (chrome.runtime.lastError) {
          reject(new Error(chrome.runtime.lastError.message));
          return;
        }
        if (response && (response.status === "accepted" || response.status === "online")) {
          resolve(response);
        } else {
          reject(new Error((response && response.message) || "Native host rejected request"));
        }
      });
    } catch (err) {
      reject(err);
    }
  });
}

async function dispatchInterceptToRustIdm({
  url,
  referer,
  filename,
  userAgent,
  queueName,
  description
}) {
  const cookies = await getCookiesHeaderForUrl(url);
  const bodyPayload = {
    url,
    source_page_url: referer || null,
    filename: filename || null,
    cookies: cookies || null,
    referer: referer || null,
    user_agent: userAgent || navigator.userAgent,
    queue_name: queueName || "main",
    expected_sha256: null,
    description: description || null
  };

  try {
    const resp = await fetch(`${RUSTIDM_API_BASE}/intercept`, {
      method: "POST",
      headers: IPC_HEADERS,
      body: JSON.stringify(bodyPayload)
    });
    if (resp.ok) {
      const data = await resp.json();
      return { ok: true, channel: "http_ipc", data };
    }
  } catch (_) {
    // محرك HTTP غير متاح حالياً؛ ننتقل تلقائياً إلى Chrome Native Messaging Host
  }

  try {
    const nativeResp = await sendViaNativeMessagingHost({
      action: "INTERCEPT_DOWNLOAD",
      url,
      filename: filename || null,
      referer: referer || null,
      cookies: cookies || null,
      user_agent: userAgent || navigator.userAgent
    });
    return { ok: true, channel: "native_messaging", data: nativeResp };
  } catch (nativeErr) {
    return { ok: false, error: nativeErr.message };
  }
}

function setupContextMenus() {
  if (!chrome.contextMenus) return;
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: "rustidm_download_link",
      title: "Download with StormGet (RustIDM)",
      contexts: ["link", "video", "audio"]
    });
    chrome.contextMenus.create({
      id: "rustidm_download_all_links",
      title: "Download all links on page with StormGet (Batch)...",
      contexts: ["page", "frame"]
    });
    chrome.contextMenus.create({
      id: "rustidm_refresh_expired_link",
      title: "Use this link to refresh expired download in StormGet...",
      contexts: ["link"]
    });
  });
}

chrome.runtime.onInstalled.addListener(() => {
  setupContextMenus();
});

if (chrome.contextMenus && chrome.contextMenus.onClicked) {
  chrome.contextMenus.onClicked.addListener(async (info, tab) => {
    const pageUrl = (tab && tab.url) || info.pageUrl || "";
    if (info.menuItemId === "rustidm_download_link") {
      const targetUrl = info.linkUrl || info.srcUrl;
      if (targetUrl && targetUrl.startsWith("http")) {
        await dispatchInterceptToRustIdm({
          url: targetUrl,
          referer: pageUrl,
          filename: null,
          userAgent: navigator.userAgent,
          queueName: "main",
          description: "Context Menu Download"
        });
      }
    } else if (info.menuItemId === "rustidm_download_all_links" && tab && tab.id >= 0) {
      chrome.tabs.sendMessage(tab.id, { type: "RUSTIDM_OPEN_BATCH_GRABBER_MODAL" }, () => {
        void chrome.runtime.lastError;
      });
    } else if (info.menuItemId === "rustidm_refresh_expired_link" && tab && tab.id >= 0) {
      const freshUrl = info.linkUrl;
      if (freshUrl && freshUrl.startsWith("http")) {
        chrome.tabs.sendMessage(
          tab.id,
          { type: "RUSTIDM_OPEN_REFRESH_MODAL", candidateUrl: freshUrl },
          () => {
            void chrome.runtime.lastError;
          }
        );
      }
    }
  });
}

function registerTabStream(tabId, streamInfo) {
  if (tabId < 0 || !streamInfo.url) return;
  if (!tabStreams.has(tabId)) {
    tabStreams.set(tabId, new Map());
  }
  const map = tabStreams.get(tabId);
  map.set(streamInfo.url, {
    ...streamInfo,
    discoveredAt: Date.now()
  });

  chrome.tabs.sendMessage(
    tabId,
    {
      type: "RUSTIDM_STREAMS_UPDATED",
      streams: Array.from(map.values())
    },
    () => {
      void chrome.runtime.lastError;
    }
  );
}

/**
 * التقاط مسارات videoplayback الخاصة بيوتيوب (سواء المباشرة أو SABR) عبر yt-core.js
 */
function captureYoutubePlaybackStream(details) {
  const tabId = details.tabId;
  const url = details.url;
  if (tabId < 0 || typeof parseStream !== "function") return false;

  const parsed = parseStream(url);
  if (!parsed) return false;

  if (parsed.sabr) {
    if ([200, 206].includes(details.statusCode)) {
      const seed = youtubeSabrSeed.get(details.requestId);
      if (seed && seed.tabId === tabId && seed.url === parsed.url) {
        youtubeSabrObserved.set(tabId, seed);
      } else if (!youtubeSabrObserved.has(tabId)) {
        youtubeSabrObserved.set(tabId, {
          tabId,
          url: parsed.url,
          requestBody: null
        });
      }
      youtubeSabrSeed.delete(details.requestId);
      if (youtubeSabrObserved.size > 50) {
        youtubeSabrObserved.delete(youtubeSabrObserved.keys().next().value);
      }
    }
    return true;
  }

  chrome.tabs.get(tabId, (tab) => {
    if (chrome.runtime.lastError || !tab) return;
    const vid = typeof videoIdOf === "function" ? videoIdOf(tab.url || "") : null;
    let state = tabYoutubeStreams.get(tabId);
    if (!state || (vid && state.videoId && state.videoId !== vid)) {
      state = { videoId: vid, streams: {}, observedUrls: [] };
      tabYoutubeStreams.set(tabId, state);
    } else if (vid && !state.videoId) {
      state.videoId = vid;
    }
    state.streams[parsed.itag] = parsed;
    if (!state.observedUrls.includes(parsed.url)) {
      state.observedUrls.unshift(parsed.url);
      if (state.observedUrls.length > 100) {
        state.observedUrls.length = 100;
      }
    }

    registerTabStream(tabId, {
      url: parsed.url,
      protocol: "yt_direct",
      contentType: parsed.mime || "video/mp4",
      contentLength: parsed.size || null
    });
  });

  return true;
}

/**
 * مراقبة رؤوس الاستجابة لاكتشاف:
 * 1. تدفقات يوتيوب المباشرة وتدفقات SABR (googlevideo.com/videoplayback) عبر yt-core.js
 * 2. قوائم بث HLS (.m3u8) و MPEG-DASH (.mpd) وملفات الفيديو المباشرة
 */
chrome.webRequest.onHeadersReceived.addListener(
  (details) => {
    if (details.tabId < 0) return;
    const url = details.url;
    const urlLower = url.toLowerCase();

    if (url.startsWith(RUSTIDM_API_BASE)) {
      return;
    }

    // 1. فحص روابط videoplayback الخاصة بيوتيوب أولاً عبر yt-core.js
    if (urlLower.includes("googlevideo.com/videoplayback")) {
      captureYoutubePlaybackStream(details);
      return;
    }

    if (isSegmentFragmentUrl(urlLower)) {
      return;
    }

    let contentType = "";
    let contentLength = 0;

    for (const h of details.responseHeaders || []) {
      const name = h.name.toLowerCase();
      if (name === "content-type" && h.value) {
        contentType = h.value.toLowerCase();
      } else if (name === "content-length" && h.value) {
        contentLength = parseInt(h.value, 10) || 0;
      }
    }

    const isHls = urlLower.includes(".m3u8") || contentType.includes("mpegurl");
    const isDash = urlLower.includes(".mpd") || contentType.includes("dash+xml");
    const isDirectVideo =
      VIDEO_CONTENT_TYPES.some((t) => contentType.includes(t.toLowerCase())) ||
      /\.(mp4|webm|mkv|flv|mov)(\?|$)/i.test(urlLower);

    if (isDirectVideo && !isHls && !isDash && contentLength > 0 && contentLength < 300 * 1024) {
      return;
    }

    if (isHls || isDash || isDirectVideo) {
      const protocol = isHls ? "hls" : isDash ? "dash" : "direct";
      registerTabStream(details.tabId, {
        url,
        protocol,
        contentType,
        contentLength: contentLength > 0 ? contentLength : null
      });
    }
  },
  { urls: ["<all_urls>"] },
  ["responseHeaders"]
);

chrome.tabs.onRemoved.addListener((tabId) => {
  tabStreams.delete(tabId);
  tabYoutubeStreams.delete(tabId);
  youtubePlayers.delete(tabId);
  clearSabrCaptureForTab(tabId);
});

if (chrome.tabs.onUpdated) {
  chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
    if (changeInfo.status === "loading") {
      clearSabrCaptureForTab(tabId);
    }
  });
}

const interceptedDownloadIds = new Set();

/**
 * نموذج ملكية التحميل المستلهم من IDM و Awesome Downloader:
 * إيقاف تحميل المتصفح مؤقتاً أثناء التسليم للمحرك، وإلغاؤه عند النجاح أو استئنافه في المتصفح إذا كان المحرك مغلقاً.
 */
async function handleDownloadInterception(downloadItem) {
  if (!downloadItem || !downloadItem.id) return;
  const id = downloadItem.id;
  if (interceptedDownloadIds.has(id)) {
    return;
  }

  const url = downloadItem.finalUrl || downloadItem.url;
  if (!url || !url.startsWith("http") || url.startsWith(RUSTIDM_API_BASE)) {
    return;
  }

  const filename = downloadItem.filename
    ? downloadItem.filename.split(/[\\/]/).pop()
    : null;
  const mime = downloadItem.mime || null;

  if (!hasInterceptableExtension(url, filename, mime)) {
    return;
  }

  interceptedDownloadIds.add(id);

  // إيقاف التحميل مؤقتاً في المتصفح ريثما يتم تسليمه لمحرك StormGet
  let pausedInBrowser = false;
  try {
    chrome.downloads.pause(id, () => void chrome.runtime.lastError);
    pausedInBrowser = true;
  } catch (_) {}

  const outcome = await dispatchInterceptToRustIdm({
    url,
    referer: downloadItem.referrer || null,
    filename,
    userAgent: navigator.userAgent,
    queueName: "main",
    description: "Browser Intercepted Download"
  });

  if (outcome.ok) {
    chrome.downloads.cancel(id, () => {
      void chrome.runtime.lastError;
      chrome.downloads.erase({ id }, () => {
        void chrome.runtime.lastError;
      });
    });
  } else {
    // إذا فشل التسليم، نستأنف التحميل داخل المتصفح حتى لا يفقد المستخدم ملفه
    interceptedDownloadIds.delete(id);
    if (pausedInBrowser) {
      chrome.downloads.resume(id, () => void chrome.runtime.lastError);
    }
    console.warn("[StormGet] Dispatch failed, browser download resumed:", outcome.error);
  }
}

chrome.downloads.onCreated.addListener((downloadItem) => {
  handleDownloadInterception(downloadItem);
});

if (chrome.downloads.onDeterminingFilename) {
  chrome.downloads.onDeterminingFilename.addListener((downloadItem) => {
    handleDownloadInterception(downloadItem);
  });
}

if (chrome.downloads.onErased) {
  chrome.downloads.onErased.addListener((id) => {
    interceptedDownloadIds.delete(id);
  });
}

/**
 * بناء خيارات يوتيوب الفورية (تجمع بين المسارات المباشرة `yt-core` وجلسات `SABR` الحديثة)
 */
function buildInstantYoutubeProbe(tabId, pageUrl, pageTitle) {
  const currentVid = typeof videoIdOf === "function" ? videoIdOf(pageUrl || "") : null;
  if (!currentVid) return null;

  const ytState = tabYoutubeStreams.get(tabId);
  const player = youtubePlayers.get(tabId);
  const sabrCapture = youtubeSabrObserved.get(tabId);

  const cleanTitle =
    typeof cleanYoutubeTitle === "function"
      ? cleanYoutubeTitle((player && player.title) || pageTitle)
      : pageTitle || "youtube_video";

  const qualities = [];
  const seenResolutions = new Set();

  // 1. خيارات playerResponse + SABR (إن توفرت لنفس الفيديو)
  if (
    player &&
    player.videoId === currentVid &&
    typeof buildSabrAndDirectChoices === "function"
  ) {
    const observedUrls =
      ytState && ytState.videoId === currentVid ? ytState.observedUrls || [] : [];
    const sabrChoices = buildSabrAndDirectChoices(player, observedUrls, sabrCapture);
    sabrChoices.forEach((opt, idx) => {
      seenResolutions.add(opt.quality);
      qualities.push({
        id: `ytsabr_${idx}_${opt.quality}`,
        label: opt.label,
        resolution: opt.quality,
        container: opt.ext || "mp4",
        bandwidth_bps: opt.bitrate || null,
        estimated_size_bytes: opt.size > 0 ? opt.size : null,
        download_url: opt.videoUrl,
        audio_url: opt.audioUrl || null,
        protocol: opt.protocol || (opt.audioUrl ? "direct_mux" : "direct"),
        format_id: opt.formatId || null,
        sabr_session: opt.sabrSession || null
      });
    });
  }

  // 2. دمج أي مسارات videoplayback مباشرة إضافية التقطها yt-core.js
  if (
    ytState &&
    (!ytState.videoId || ytState.videoId === currentVid) &&
    ytState.streams &&
    typeof buildOptions === "function"
  ) {
    const directOptions = buildOptions(ytState.streams);
    directOptions.forEach((opt, idx) => {
      if (seenResolutions.has(opt.quality)) return;
      seenResolutions.add(opt.quality);
      qualities.push({
        id: `ytcore_${idx}_${opt.quality}`,
        label: opt.label,
        resolution: opt.quality,
        container: opt.ext || "mp4",
        bandwidth_bps: null,
        estimated_size_bytes: opt.size > 0 ? opt.size : null,
        download_url: opt.videoUrl,
        audio_url: opt.audioUrl || null,
        protocol: opt.audioUrl ? "direct_mux" : "direct",
        format_id: null,
        sabr_session: null
      });
    });
  }

  if (qualities.length === 0) return null;

  return {
    title: cleanTitle,
    drm_protected: false,
    drm_reason: null,
    qualities
  };
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  const tabId = sender.tab ? sender.tab.id : -1;

  if (message.type === "RUSTIDM_YOUTUBE_PLAYER_RESPONSE") {
    if (tabId >= 0 && message.data && typeof sanitizePlayer === "function") {
      const clean = sanitizePlayer(message.data);
      if (clean) {
        const previous = youtubePlayers.get(tabId);
        if (previous && previous.videoId !== clean.videoId) {
          clearSabrCaptureForTab(tabId);
          tabYoutubeStreams.delete(tabId);
        }
        youtubePlayers.set(tabId, clean);
        if (youtubePlayers.size > 50) {
          youtubePlayers.delete(youtubePlayers.keys().next().value);
        }
      }
    }
    sendResponse({ ok: true });
    return true;
  }

  if (message.type === "RUSTIDM_INTERCEPT_LINK") {
    (async () => {
      const outcome = await dispatchInterceptToRustIdm({
        url: message.url,
        referer: message.pageUrl || null,
        filename: message.filename || null,
        userAgent: navigator.userAgent,
        queueName: "main",
        description: "Direct Page Link Click"
      });
      sendResponse(outcome);
    })();
    return true;
  }

  if (message.type === "RUSTIDM_REPORT_HOOK_STREAM") {
    if (tabId !== null && tabId >= 0 && message.url) {
      registerTabStream(tabId, {
        url: message.url,
        protocol: message.protocol || "direct",
        contentType: message.hookSource || "main-hook",
        detectedAt: Date.now()
      });
    }
    sendResponse({ ok: true });
    return true;
  }

  if (message.type === "RUSTIDM_GET_TAB_STREAMS") {
    const streams = tabStreams.has(tabId)
      ? Array.from(tabStreams.get(tabId).values())
      : [];
    sendResponse({ ok: true, streams });
    return true;
  }

  if (message.type === "RUSTIDM_PROBE_QUALITIES") {
    (async () => {
      // إذا كنا في يوتيوب، نطلب تحديث بيانات المشغل أولاً
      if (tabId >= 0 && (message.pageUrl || "").includes("youtube.com")) {
        try {
          await new Promise((resolve) => {
            chrome.tabs.sendMessage(
              tabId,
              { type: "RUSTIDM_REFRESH_YOUTUBE_PLAYER" },
              () => {
                void chrome.runtime.lastError;
                resolve();
              }
            );
          });
        } catch (_) {}
      }

      // 1. إذا كانت الصفحة يوتيوب ولدينا مسارات مباشر أو جلسات SABR نعرضها فوراً (0ms)!
      const instantYt = buildInstantYoutubeProbe(
        tabId,
        message.pageUrl,
        message.pageTitle
      );

      try {
        if (instantYt && instantYt.qualities.length > 0) {
          sendResponse({ ok: true, probe: instantYt });
          return;
        }

        const targetUrl = message.streamUrl || message.pageUrl;
        const cookies = await getCookiesHeaderForUrl(targetUrl);

        const resp = await fetch(`${RUSTIDM_API_BASE}/video/probe`, {
          method: "POST",
          headers: IPC_HEADERS,
          body: JSON.stringify({
            url: targetUrl,
            page_url: message.pageUrl || null,
            page_title: message.pageTitle || null,
            cookies: cookies || null,
            referer: message.pageUrl || null,
            user_agent: navigator.userAgent
          })
        });

        if (!resp.ok) {
          const errText = await resp.text();
          sendResponse({ ok: false, error: errText });
          return;
        }

        const data = await resp.json();
        sendResponse({ ok: true, probe: data });
      } catch (_) {
        if (instantYt) {
          sendResponse({ ok: true, probe: instantYt });
        } else {
          sendResponse({
            ok: false,
            error: "تعذر الاتصال بمحرك StormGet على المنفذ 16888. تأكد من تشغيل البرنامج."
          });
        }
      }
    })();
    return true;
  }

  if (message.type === "RUSTIDM_DOWNLOAD_VIDEO_QUALITY") {
    (async () => {
      try {
        const option = message.qualityOption;
        const cookies = await getCookiesHeaderForUrl(
          message.pageUrl || option.download_url
        );

        const resp = await fetch(`${RUSTIDM_API_BASE}/video/download`, {
          method: "POST",
          headers: IPC_HEADERS,
          body: JSON.stringify({
            url: option.download_url,
            audio_url: option.audio_url || option.audioUrl || null,
            page_url: message.pageUrl || null,
            title: message.title || message.pageTitle || "video",
            quality_label: option.label || null,
            container: option.container || "mp4",
            protocol:
              option.protocol || (option.audio_url ? "direct_mux" : "direct"),
            format_id: option.format_id || null,
            sabr_session: option.sabr_session || null,
            cookies: cookies || null,
            referer: message.pageUrl || null,
            user_agent: navigator.userAgent
          })
        });

        if (!resp.ok) {
          const errText = await resp.text();
          sendResponse({ ok: false, error: errText });
          return;
        }

        const result = await resp.json();
        sendResponse({ ok: true, result });
      } catch (_) {
        try {
          const option = message.qualityOption || {};
          const cookies = await getCookiesHeaderForUrl(
            message.pageUrl || option.download_url
          );
          const nativeResp = await sendViaNativeMessagingHost({
            action: "VIDEO_STREAM",
            url: option.download_url || message.pageUrl,
            audio_url: option.audio_url || option.audioUrl || null,
            protocol: option.protocol || "ytdlp",
            format_id: option.format_id || null,
            sabr_session: option.sabr_session || null,
            filename: `${message.title || "video"}.${option.container || "mp4"}`,
            referer: message.pageUrl || null,
            cookies: cookies || null,
            user_agent: navigator.userAgent
          });
          sendResponse({ ok: true, result: nativeResp });
        } catch (_) {
          sendResponse({
            ok: false,
            error: "فشل إرسال طلب التحميل إلى محرك StormGet."
          });
        }
      }
    })();
    return true;
  }

  if (message.type === "RUSTIDM_LIST_EXPIRED_TASKS") {
    (async () => {
      try {
        const resp = await fetch(`${RUSTIDM_API_BASE}/downloads`, {
          method: "GET",
          headers: IPC_HEADERS
        });
        if (!resp.ok) {
          sendResponse({ ok: false, error: "Engine unreachable" });
          return;
        }
        const all = await resp.json();
        const candidates = all.filter(
          (t) =>
            t.status === "NeedsUrlRefresh" ||
            t.status === "Paused" ||
            t.status === "Failed"
        );
        sendResponse({ ok: true, tasks: candidates });
      } catch (err) {
        sendResponse({ ok: false, error: err.message });
      }
    })();
    return true;
  }

  if (message.type === "RUSTIDM_SUBMIT_REFRESH_URL") {
    (async () => {
      try {
        const cookies = await getCookiesHeaderForUrl(message.newUrl);
        const resp = await fetch(`${RUSTIDM_API_BASE}/refresh-url`, {
          method: "POST",
          headers: IPC_HEADERS,
          body: JSON.stringify({
            task_id: message.taskId,
            new_url: message.newUrl,
            cookies: cookies || null
          })
        });
        if (!resp.ok) {
          const errText = await resp.text();
          sendResponse({ ok: false, error: errText });
          return;
        }
        const data = await resp.json();
        sendResponse({ ok: true, data });
      } catch (err) {
        sendResponse({ ok: false, error: err.message });
      }
    })();
    return true;
  }

  if (message.type === "RUSTIDM_BATCH_ENQUEUE_LINKS") {
    (async () => {
      try {
        const cookies = await getCookiesHeaderForUrl(message.pageUrl || "");
        const resp = await fetch(`${RUSTIDM_API_BASE}/batch`, {
          method: "POST",
          headers: IPC_HEADERS,
          body: JSON.stringify({
            pattern_or_urls: (message.urls || []).join("\n"),
            step: 1,
            queue_name: message.queueName || "main",
            cookies: cookies || null,
            referer: message.pageUrl || null,
            start_immediately: message.startImmediately !== false
          })
        });
        if (!resp.ok) {
          const errText = await resp.text();
          sendResponse({ ok: false, error: errText });
          return;
        }
        const data = await resp.json();
        sendResponse({ ok: true, data });
      } catch (err) {
        sendResponse({ ok: false, error: err.message });
      }
    })();
    return true;
  }

  return false;
});
