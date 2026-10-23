/**
 * RustIDM Integration Module - Background Service Worker (v1.2.0)
 * يلتقط تدفقات الفيديو (HLS m3u8 / DASH mpd / MP4 / WebM) وصفحات الفيديو،
 * ويتواصل مع محرك RustIDM لاستخراج الجودات الحقيقية وتنزيل الجودة المختارة.
 */

const RUSTIDM_API_BASE = "http://127.0.0.1:16888/api/v1";

// تخزين روابط البث المكتشفة لكل تبويب: tabId -> Map<url, StreamEntry>
const tabStreams = new Map();

const MEDIA_EXTENSIONS = [
  "3gp", "7z", "aac", "ace", "aif", "apk", "arj", "asf", "avi", "bin",
  "bz2", "exe", "gz", "gzip", "img", "iso", "lzh", "m4a", "m4v", "mkv",
  "mov", "mp3", "mp4", "mpa", "mpe", "mpeg", "mpg", "msi", "msu", "ogg",
  "ogv", "pdf", "plj", "pps", "ppt", "qt", "ra", "rar", "rm", "rmvb",
  "sea", "sit", "sitx", "tar", "tif", "tiff", "wav", "wma", "wmv", "z",
  "zip", "flv", "flac", "m3u8", "mpd"
];

const MEDIA_EXTENSION_SET = new Set(MEDIA_EXTENSIONS);

function hasInterceptableExtension(url) {
  let pathname;
  try {
    pathname = new URL(url).pathname.toLowerCase();
  } catch {
    return false;
  }

  const extension = pathname.slice(pathname.lastIndexOf(".") + 1);
  return MEDIA_EXTENSION_SET.has(extension) || /^r[01].*$/.test(extension);
}

const VIDEO_CONTENT_TYPES = [
  "application/vnd.apple.mpegurl",
  "application/x-mpegurl",
  "application/dash+xml",
  "video/mp4",
  "video/webm",
  "video/x-matroska",
  "video/x-flv",
  "video/quicktime"
];

/**
 * تجاهل مقاطع البث الجزئية الصغيرة (Segments) للتركيز على ملفات القوائم الرئيسية أو الفيديو الكامل
 */
function isSegmentFragmentUrl(urlLower) {
  if (urlLower.endsWith(".ts") || urlLower.endsWith(".m4s") || urlLower.includes(".m4s?")) {
    return true;
  }
  if (/seg-\d+|segment-\d+|chunk-\d+|frag-\d+/i.test(urlLower) && !urlLower.includes(".m3u8")) {
    return true;
  }
  return false;
}

/**
 * تسجيل بث مكتشف في التبويب وإشعار content.js فوراً
 */
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
 * مراقبة رؤوس الاستجابة لاكتشاف ملفات m3u8 و mpd وملفات الفيديو المباشرة
 */
chrome.webRequest.onHeadersReceived.addListener(
  (details) => {
    if (details.tabId < 0) return;
    const url = details.url;
    const urlLower = url.toLowerCase();

    if (url.startsWith(RUSTIDM_API_BASE) || isSegmentFragmentUrl(urlLower)) {
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

    const isHls =
      urlLower.includes(".m3u8") ||
      contentType.includes("mpegurl");
    const isDash =
      urlLower.includes(".mpd") ||
      contentType.includes("dash+xml");
    const isDirectVideo =
      VIDEO_CONTENT_TYPES.some((t) => contentType.includes(t)) ||
      /\.(mp4|webm|mkv|flv|mov)(\?|$)/i.test(urlLower);

    // تجاهل مقاطع الفيديو الإعلانية أو الصغيرة جداً أقل من 300 كيلوبايت (إلا إذا كانت قائمة m3u8/mpd)
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

/**
 * تنظيف بيانات التبويب عند إغلاقه أو الانتقال لصفحة جديدة
 */
chrome.tabs.onRemoved.addListener((tabId) => {
  tabStreams.delete(tabId);
});

/**
 * جلب الكوكيز الخاصة بالرابط لتمريرها بأمان إلى محرك RustIDM
 */
async function getCookiesHeaderForUrl(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch (_) {
    return "";
  }
}

/**
 * اعتراض التحميلات العادية في المتصفح وتحويلها إلى RustIDM
 */
chrome.downloads.onCreated.addListener(async (downloadItem) => {
  const url = downloadItem.finalUrl || downloadItem.url;
  if (!url || !url.startsWith("http") || url.startsWith(RUSTIDM_API_BASE)) {
    return;
  }

  const urlLower = url.toLowerCase();
  if (!hasInterceptableExtension(urlLower)) {
    return;
  }

  try {
    const cookies = await getCookiesHeaderForUrl(url);
    const resp = await fetch(`${RUSTIDM_API_BASE}/intercept`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        url,
        source_page_url: downloadItem.referrer || null,
        filename: downloadItem.filename ? downloadItem.filename.split(/[\\/]/).pop() : null,
        cookies: cookies || null,
        referer: downloadItem.referrer || null,
        user_agent: navigator.userAgent
      })
    });

    if (resp.ok) {
      chrome.downloads.cancel(downloadItem.id, () => {
        void chrome.runtime.lastError;
      });
      chrome.downloads.erase({ id: downloadItem.id }, () => {
        void chrome.runtime.lastError;
      });
    }
  } catch (_) {
    // في حال كان محرك سطح المكتب مغلقاً، يترك المتصفح يكمل التحميل العادي
  }
});

/**
 * معالجة رسائل content.js لاستكشاف الجودات الفعلية وبدء التحميل
 */
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  const tabId = sender.tab ? sender.tab.id : -1;

  if (message.type === "RUSTIDM_GET_TAB_STREAMS") {
    const streams = tabStreams.has(tabId)
      ? Array.from(tabStreams.get(tabId).values())
      : [];
    sendResponse({ ok: true, streams });
    return true;
  }

  if (message.type === "RUSTIDM_PROBE_QUALITIES") {
    (async () => {
      try {
        const targetUrl = message.streamUrl || message.pageUrl;
        const cookies = await getCookiesHeaderForUrl(targetUrl);

        const resp = await fetch(`${RUSTIDM_API_BASE}/video/probe`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
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
      } catch (err) {
        sendResponse({
          ok: false,
          error: "تعذر الاتصال بمحرك RustIDM على المنفذ 16888. تأكد من تشغيل البرنامج."
        });
      }
    })();
    return true;
  }

  if (message.type === "RUSTIDM_DOWNLOAD_VIDEO_QUALITY") {
    (async () => {
      try {
        const option = message.qualityOption;
        const cookies = await getCookiesHeaderForUrl(option.download_url || message.pageUrl);

        const resp = await fetch(`${RUSTIDM_API_BASE}/video/download`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            url: option.download_url,
            page_url: message.pageUrl || null,
            title: message.title || message.pageTitle || "video",
            quality_label: option.label || null,
            container: option.container || "mp4",
            protocol: option.protocol || "direct",
            format_id: option.format_id || null,
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
      } catch (err) {
        sendResponse({
          ok: false,
          error: "فشل إرسال طلب التحميل إلى محرك RustIDM."
        });
      }
    })();
    return true;
  }

  return false;
});