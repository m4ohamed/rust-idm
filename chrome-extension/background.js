/**
 * RustIDM Integration Module - Background Service Worker (v2.0.0)
 * يطبق:
 * 1. اعتراض التحميلات العادية وتمرير الكوكيز المقيدة بالنطاق والمرجع إلى محرك RustIDM المحلي.
 * 2. السقوط التلقائي الآمن (Automatic Fallback) إلى بروتوكول Chrome Native Messaging Host (`com.rustidm.native_host`) إذا كان المنفذ 16888 غير متاح.
 * 3. مراقبة رؤوس الشبكة (HLS .m3u8 / MPEG-DASH .mpd / MP4 / WebM / MKV) وإخطار شريط الفيديو العائم لحظياً.
 * 4. قوائم النقر بزر الفأرة الأيمن (Context Menus):
 *    - Download with RustIDM
 *    - Download all links on page with RustIDM (Batch Grabber)
 *    - Refresh expired download link in RustIDM
 */

const RUSTIDM_API_BASE = "http://127.0.0.1:16888/api/v1";
const NATIVE_HOST_NAME = "com.rustidm.native_host";

// تخزين روابط البث المكتشفة لكل تبويب: tabId -> Map<url, StreamEntry>
const tabStreams = new Map();

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

function hasInterceptableExtension(url) {
  let pathname;
  try {
    pathname = new URL(url).pathname.toLowerCase();
  } catch {
    return false;
  }

  const lastDot = pathname.lastIndexOf(".");
  if (lastDot === -1) return false;
  const extension = pathname.slice(lastDot + 1);
  return MEDIA_EXTENSION_SET.has(extension) || /^r[01]\d*$/.test(extension);
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
  if (/seg-\d+|segment-\d+|chunk-\d+|frag-\d+/i.test(urlLower) && !urlLower.includes(".m3u8") && !urlLower.includes(".mpd")) {
    return true;
  }
  return false;
}

/**
 * جلب الكوكيز الخاصة بالرابط الهدف حصراً لتمريرها بأمان إلى محرك RustIDM
 */
async function getCookiesHeaderForUrl(url) {
  try {
    if (!url || !url.startsWith("http")) return "";
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch (_) {
    return "";
  }
}

/**
 * إرسال أمر إلى مضيف المراسلة الأصلي (Chrome Native Messaging Host) عند عدم استجابة المنفذ المحلي
 */
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

/**
 * إرسال طلب اعتراض تحميل إلى محرك RustIDM عبر HTTP IPC أولاً، ثم عبر Native Messaging كبديل تلقائي
 */
async function dispatchInterceptToRustIdm({ url, referer, filename, userAgent, queueName, description }) {
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
      headers: { "Content-Type": "application/json" },
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

/**
 * تسجيل قوائم النقر بالزر الأيمن في المتصفح (Context Menus) عند تثبيت الإضافة
 */
function setupContextMenus() {
  if (!chrome.contextMenus) return;
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: "rustidm_download_link",
      title: "Download with RustIDM",
      contexts: ["link", "video", "audio"]
    });
    chrome.contextMenus.create({
      id: "rustidm_download_all_links",
      title: "Download all links on page with RustIDM (Batch)...",
      contexts: ["page", "frame"]
    });
    chrome.contextMenus.create({
      id: "rustidm_refresh_expired_link",
      title: "Use this link to refresh expired download in RustIDM...",
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
});

/**
 * اعتراض التحميلات العادية في المتصفح وتحويلها إلى RustIDM (مع دعم Native Messaging كاحتياطي)
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

  const filename = downloadItem.filename
    ? downloadItem.filename.split(/[\\/]/).pop()
    : null;

  const outcome = await dispatchInterceptToRustIdm({
    url,
    referer: downloadItem.referrer || null,
    filename,
    userAgent: navigator.userAgent,
    queueName: "main",
    description: "Browser Intercepted Download"
  });

  if (outcome.ok) {
    chrome.downloads.cancel(downloadItem.id, () => {
      void chrome.runtime.lastError;
    });
    chrome.downloads.erase({ id: downloadItem.id }, () => {
      void chrome.runtime.lastError;
    });
  }
});

/**
 * معالجة رسائل content.js لاستكشاف الجودات، بدء التحميل، السحب الدفعي، وتجديد الروابط المنتهية
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
        // محاولة الإرسال الاحتياطي عبر Native Messaging إذا كان الرابط مباشراً
        try {
          const option = message.qualityOption || {};
          const cookies = await getCookiesHeaderForUrl(option.download_url || message.pageUrl);
          const nativeResp = await sendViaNativeMessagingHost({
            action: "VIDEO_STREAM",
            url: option.download_url || message.pageUrl,
            filename: `${message.title || "video"}.${option.container || "mp4"}`,
            referer: message.pageUrl || null,
            cookies: cookies || null,
            user_agent: navigator.userAgent
          });
          sendResponse({ ok: true, result: nativeResp });
        } catch (_) {
          sendResponse({
            ok: false,
            error: "فشل إرسال طلب التحميل إلى محرك RustIDM."
          });
        }
      }
    })();
    return true;
  }

  if (message.type === "RUSTIDM_LIST_EXPIRED_TASKS") {
    (async () => {
      try {
        const resp = await fetch(`${RUSTIDM_API_BASE}/downloads`);
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
          headers: { "Content-Type": "application/json" },
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
          headers: { "Content-Type": "application/json" },
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
