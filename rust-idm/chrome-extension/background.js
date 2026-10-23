/**
 * RustIDM Integration Module v1.1.0 - Chrome Extension Background Service Worker (Manifest V3)
 * يدعم:
 * 1. بروتوكول Chrome Native Messaging ("com.rustidm.native_host") لتشغيل التطبيق الأصلي تلقائياً حتى لو كان مغلقاً.
 * 2. التراجع الآمن إلى خادم IPC المحلي (http://127.0.0.1:16888/api/v1) أو تحميل المتصفح الافتراضي عند غياب التطبيق.
 * 3. تقييد صارم لملفات تعريف الارتباط (Domain-Scoped Cookies) ومنع تكرار نفس التحميل (Deduplication).
 * 4. رصد تدفقات الفيديو (MP4, WebM, HLS .m3u8, MPEG-DASH .mpd) وتجديد الروابط منتهية الصلاحية.
 */

const RUST_IDM_API_BASE = "http://127.0.0.1:16888/api/v1";
const NATIVE_HOST_NAME = "com.rustidm.native_host";
const MIN_VIDEO_SIZE_BYTES = 512 * 1024; // 512 KB

const tabDetectedStreams = new Map();
const recentDispatchedUrls = new Map(); // url -> timestamp لمنع تكرار نفس التنزيل

const INTERCEPT_EXTENSIONS = new Set([
  "zip", "rar", "7z", "tar", "gz", "bz2", "xz",
  "exe", "msi", "dmg", "iso", "apk", "deb", "rpm",
  "mp4", "mkv", "webm", "avi", "mov", "flv",
  "mp3", "flac", "wav", "m4a", "aac",
  "pdf", "docx", "xlsx", "pptx", "bin"
]);

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "rustidm_download_link",
    title: "⬇ التحميل بواسطة محرك RustIDM",
    contexts: ["link", "video", "audio"]
  });

  chrome.contextMenus.create({
    id: "rustidm_refresh_expired",
    title: "🔄 استخدام هذا الرابط لتجديد مهمة منتهية الصلاحية في RustIDM",
    contexts: ["link"]
  });

  chrome.contextMenus.create({
    id: "rustidm_download_all",
    title: "📦 تحميل جميع الروابط في هذه الصفحة عبر RustIDM",
    contexts: ["page"]
  });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId === "rustidm_download_link") {
    const targetUrl = info.linkUrl || info.srcUrl;
    if (targetUrl) {
      await dispatchDownloadToRustIDM(targetUrl, null, info.pageUrl);
    }
  } else if (info.menuItemId === "rustidm_refresh_expired") {
    const targetUrl = info.linkUrl;
    if (targetUrl && tab?.id) {
      chrome.tabs.sendMessage(tab.id, {
        action: "PROMPT_REFRESH_TASK_ID",
        new_url: targetUrl
      });
    }
  } else if (info.menuItemId === "rustidm_download_all" && tab?.id) {
    chrome.tabs.sendMessage(tab.id, { action: "COLLECT_ALL_PAGE_LINKS" });
  }
});

/**
 * اعتراض التحميلات في المتصفح مع فحص صلاحية البروتوكول ومنع التكرار
 */
chrome.downloads.onCreated.addListener(async (downloadItem) => {
  const targetUrl = downloadItem.finalUrl || downloadItem.url;
  if (!targetUrl || targetUrl.startsWith("blob:") || targetUrl.startsWith("data:")) {
    return;
  }

  let parsedUrl;
  try {
    parsedUrl = new URL(targetUrl);
    if (parsedUrl.protocol !== "http:" && parsedUrl.protocol !== "https:") {
      return;
    }
  } catch {
    return;
  }

  const ext = extractExtension(targetUrl);
  if (ext && !INTERCEPT_EXTENSIONS.has(ext) && downloadItem.fileSize > 0 && downloadItem.fileSize < 1024 * 1024) {
    return;
  }

  if (isDuplicateDispatch(targetUrl)) {
    return;
  }

  // محاولة التسليم عبر Native Messaging أولاً (لتشغيل التطبيق تلقائياً إذا كان مغلقاً) أو عبر HTTP IPC
  const delivered = await dispatchDownloadToRustIDM(
    targetUrl,
    downloadItem.filename ? downloadItem.filename.split(/[\\/]/).pop() : null,
    downloadItem.referrer
  );

  if (delivered) {
    try {
      await chrome.downloads.cancel(downloadItem.id);
      await chrome.downloads.erase({ id: downloadItem.id });
    } catch {
      // في حال انتهى التحميل الصغير قبل الإلغاء
    }
  }
});

/**
 * مراقبة ترويسات الشبكة لاكتشاف قوائم HLS (.m3u8) و MPEG-DASH (.mpd) والفيديوهات المباشرة
 */
chrome.webRequest.onHeadersReceived.addListener(
  (details) => {
    if (details.tabId < 0 || details.statusCode < 200 || details.statusCode >= 300) {
      return;
    }

    const headers = details.responseHeaders || [];
    let contentType = "";
    let contentLength = 0;

    for (const h of headers) {
      const name = h.name.toLowerCase();
      if (name === "content-type" && h.value) {
        contentType = h.value.toLowerCase();
      } else if (name === "content-length" && h.value) {
        contentLength = parseInt(h.value, 10) || 0;
      }
    }

    const urlLower = details.url.toLowerCase();
    const isHls =
      urlLower.includes(".m3u8") ||
      contentType.includes("application/vnd.apple.mpegurl") ||
      contentType.includes("application/x-mpegurl");
    const isDash =
      urlLower.includes(".mpd") || contentType.includes("application/dash+xml");
    const isDirectVideo =
      contentType.startsWith("video/") &&
      !contentType.includes("mp2t") &&
      (contentLength === 0 || contentLength >= MIN_VIDEO_SIZE_BYTES);

    if (isHls || isDash || isDirectVideo) {
      const streamType = isHls ? "HLS (.m3u8)" : isDash ? "MPEG-DASH (.mpd)" : "Direct Video";
      const streamInfo = {
        url: details.url,
        stream_type: streamType,
        mime_type: contentType || (isHls ? "application/x-mpegURL" : isDash ? "application/dash+xml" : "video/mp4"),
        content_length: contentLength,
        quality_label: inferQualityLabel(details.url, streamType, contentLength),
        tab_id: details.tabId
      };

      if (!tabDetectedStreams.has(details.tabId)) {
        tabDetectedStreams.set(details.tabId, []);
      }
      const list = tabDetectedStreams.get(details.tabId);
      if (!list.some((item) => item.url === streamInfo.url)) {
        list.push(streamInfo);
        chrome.tabs.sendMessage(details.tabId, {
          action: "STREAMS_UPDATED",
          streams: list
        }).catch(() => {});
      }
    }
  },
  { urls: ["<all_urls>"] },
  ["responseHeaders"]
);

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  const tabId = sender.tab?.id;

  if (message.action === "GET_TAB_STREAMS") {
    const streams = (tabId && tabDetectedStreams.get(tabId)) || [];
    sendResponse({ streams });
    return true;
  }

  if (message.action === "DOWNLOAD_VIDEO_STREAM") {
    handleVideoStreamDownload(message.payload, sender.tab).then((res) => {
      sendResponse(res);
    });
    return true;
  }

  if (message.action === "REFRESH_EXPIRED_TASK_URL") {
    handleRefreshExpiredUrl(message.payload).then((res) => {
      sendResponse(res);
    });
    return true;
  }

  if (message.action === "BATCH_DOWNLOAD_LINKS") {
    handleBatchDownloadLinks(message.payload, sender.tab).then((res) => {
      sendResponse(res);
    });
    return true;
  }
});

function isDuplicateDispatch(url) {
  const now = Date.now();
  for (const [k, ts] of recentDispatchedUrls.entries()) {
    if (now - ts > 5000) recentDispatchedUrls.delete(k);
  }
  if (recentDispatchedUrls.has(url)) return true;
  recentDispatchedUrls.set(url, now);
  return false;
}

/**
 * استخراج ملفات تعريف الارتباط المقيدة بنطاق الرابط الهدف فقط (Domain & Secure Scoped)
 * دون تسجيل أي قيم سرية في Console
 */
async function getScopedCookiesHeader(targetUrl) {
  try {
    const parsed = new URL(targetUrl);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return "";
    const cookies = await chrome.cookies.getAll({ url: targetUrl });
    return cookies
      .filter((c) => !c.secure || parsed.protocol === "https:")
      .map((c) => `${c.name}=${c.value}`)
      .join("; ");
  } catch {
    return "";
  }
}

/**
 * إرسال أمر التحميل عبر Chrome Native Messaging أولاً (لتشغيل التطبيق تلقائياً)،
 * وفي حال عدم تسجيل المضيف الأصلي يتم الإرسال إلى خادم HTTP IPC المحلي.
 */
async function dispatchDownloadToRustIDM(targetUrl, filename, referer) {
  const cookieHeader = await getScopedCookiesHeader(targetUrl);

  // 1. محاولة الإرسال عبر Chrome Native Messaging Host
  try {
    const nativeReply = await sendNativeMessageWithTimeout(
      {
        action: "INTERCEPT_DOWNLOAD",
        url: targetUrl,
        filename: filename,
        referer: referer || "",
        cookies: cookieHeader,
        user_agent: navigator.userAgent
      },
      1500
    );
    if (nativeReply && nativeReply.status === "accepted") {
      notifyUser("محرك RustIDM (Native Host)", `بدأ التحميل فائق السرعة: ${filename || targetUrl.split("/").pop()}`);
      return true;
    }
  } catch {
    // المضيف الأصلي غير مسجل بعد؛ ننتقل إلى فحص خادم IPC المحلي
  }

  // 2. التراجع الآمن إلى خادم HTTP IPC المحلي على المنفذ 16888
  try {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 1200);
    const response = await fetch(`${RUST_IDM_API_BASE}/intercept`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      signal: controller.signal,
      body: JSON.stringify({
        url: targetUrl,
        source_page_url: referer || "",
        filename: filename,
        cookies: cookieHeader,
        referer: referer || "",
        user_agent: navigator.userAgent,
        queue_name: "main"
      })
    });
    clearTimeout(timer);

    if (response.ok) {
      notifyUser("محرك RustIDM", `بدأ التحميل فائق السرعة: ${filename || targetUrl.split("/").pop()}`);
      return true;
    }
  } catch {
    // الخادم المحلي غير مشغل؛ نترك المتصفح يكمل التحميل العادي دون إفساد تجربة المستخدم
  }

  return false;
}

function sendNativeMessageWithTimeout(payload, timeoutMs) {
  return new Promise((resolve, reject) => {
    let settled = false;
    const timer = setTimeout(() => {
      if (!settled) {
        settled = true;
        reject(new Error("Native Messaging Timeout"));
      }
    }, timeoutMs);

    try {
      chrome.runtime.sendNativeMessage(NATIVE_HOST_NAME, payload, (response) => {
        clearTimeout(timer);
        if (settled) return;
        settled = true;
        if (chrome.runtime.lastError) {
          reject(new Error(chrome.runtime.lastError.message));
        } else {
          resolve(response);
        }
      });
    } catch (err) {
      clearTimeout(timer);
      if (!settled) {
        settled = true;
        reject(err);
      }
    }
  });
}

async function handleVideoStreamDownload(payload, tab) {
  const delivered = await dispatchDownloadToRustIDM(
    payload.url,
    payload.page_title ? `${payload.page_title}.mp4` : null,
    tab?.url || ""
  );
  return { status: delivered ? "video_download_started" : "engine_offline" };
}

async function handleRefreshExpiredUrl(payload) {
  try {
    const cookieHeader = await getScopedCookiesHeader(payload.new_url);
    const res = await fetch(`${RUST_IDM_API_BASE}/refresh-url`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        task_id: payload.task_id,
        new_url: payload.new_url,
        cookies: cookieHeader
      })
    });
    return await res.json();
  } catch (err) {
    return { status: "error", message: err.message };
  }
}

async function handleBatchDownloadLinks(payload, tab) {
  let count = 0;
  for (const link of payload.links || []) {
    const lower = link.toLowerCase();
    const matched =
      !payload.filter_extensions?.length ||
      payload.filter_extensions.some((ext) => lower.endsWith(`.${ext.toLowerCase()}`));
    if (matched) {
      const ok = await dispatchDownloadToRustIDM(link, null, tab?.url || payload.page_url);
      if (ok) count++;
    }
  }
  return { status: "batch_queued", queued_count: count };
}

function notifyUser(title, message) {
  chrome.notifications.create({
    type: "basic",
    iconUrl: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
    title,
    message
  });
}

function extractExtension(urlStr) {
  try {
    const pathname = new URL(urlStr).pathname;
    const lastSegment = pathname.split("/").pop() || "";
    if (lastSegment.includes(".")) {
      return lastSegment.split(".").pop().toLowerCase();
    }
  } catch {}
  return "";
}

function inferQualityLabel(url, streamType, sizeBytes) {
  const lower = url.toLowerCase();
  let res = streamType;
  if (lower.includes("2160") || lower.includes("4k")) res = `4K UHD (${streamType})`;
  else if (lower.includes("1080")) res = `1080p Full HD (${streamType})`;
  else if (lower.includes("720")) res = `720p HD (${streamType})`;
  else if (lower.includes("480")) res = `480p (${streamType})`;

  const sizeMb = sizeBytes > 0 ? ` — ${(sizeBytes / (1024 * 1024)).toFixed(1)} MB` : "";
  return `${res}${sizeMb}`;
}
