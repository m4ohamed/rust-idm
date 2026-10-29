/**
 * StormGet (RustIDM v2.3) — MAIN-world Media & YouTube SABR Interception Hook (`media-hook.js`)
 * يجمع بين:
 * 1. اصطياد استجابات مشغل يوتيوب (`window.ytInitialPlayerResponse`, `movie_player.getPlayerResponse()`, و `/youtubei/v1/player` عبر `fetch`/`XHR`)
 *    لبناء جلسات `SABR` والجودات الكاملة مع `ustreamerConfig` (مستلهم من `awesome-downloader/extension/youtube-page.js`).
 * 2. اعتراض `window.fetch()` و `XMLHttpRequest` لملفات قوائم البث (`.m3u8` / `.mpd`) ومسارات الفيديو المباشرة.
 * 3. تتبع `URL.createObjectURL(MediaSource)` و `MediaSource.prototype.addSourceBuffer()`.
 * 4. تصفية نطاقات الإعلانات والتتبع.
 */

(function () {
  const HOOK_FLAG = "__stormget_main_media_hook_v23__";
  if (window[HOOK_FLAG]) return;
  window[HOOK_FLAG] = true;

  const HLS_PATTERN = /\.m3u8(\?|#|$)/i;
  const DASH_PATTERN = /\.mpd(\?|#|$)/i;
  const VIDEO_PATTERN = /\.(mp4|webm|mkv|mov|m4v|flv)(\?|#|$)/i;
  const MANIFEST_MIMES = /mpegurl|dash\+xml/i;
  const VIDEO_MIMES = /^video\//i;
  const AD_DOMAINS =
    /doubleclick\.net|googlesyndication|googleadservices|facebook\.com\/tr|analytics|adserver|adsystem|sabavision\.com|imasdk\.googleapis|moatads|serving-sys\.com/i;

  const MAX_PLAYER_JSON_BYTES = 2 * 1024 * 1024;
  const seenStreams = new Set();

  function isYoutubeHost() {
    const h = (location.hostname || "").toLowerCase();
    return h === "www.youtube.com" || h === "youtube.com" || h === "m.youtube.com";
  }

  function isYoutubePlayerEndpoint(rawUrl) {
    try {
      const u = new URL(rawUrl, location.href);
      return u.origin === location.origin && /^\/youtubei\/v1\/player$/.test(u.pathname);
    } catch (_) {
      return false;
    }
  }

  function extractYoutubePlayerPayload(raw) {
    try {
      if (!raw || typeof raw !== "object") return null;
      const vid = raw.videoDetails?.videoId;
      if (!vid || !/^[\w-]{11}$/.test(vid) || raw.videoDetails?.isLiveContent) {
        return null;
      }
      const formats = [
        ...(Array.isArray(raw.streamingData?.formats) ? raw.streamingData.formats : []),
        ...(Array.isArray(raw.streamingData?.adaptiveFormats)
          ? raw.streamingData.adaptiveFormats
          : [])
      ].slice(0, 150);
      if (!formats.length) return null;

      const ytcfg = window.ytcfg;
      const clientName =
        ytcfg?.get?.("INNERTUBE_CONTEXT_CLIENT_NAME") ||
        ytcfg?.data_?.INNERTUBE_CONTEXT_CLIENT_NAME ||
        1;
      const clientVersion =
        ytcfg?.get?.("INNERTUBE_CONTEXT_CLIENT_VERSION") ||
        ytcfg?.data_?.INNERTUBE_CONTEXT_CLIENT_VERSION ||
        "";

      return {
        videoId: vid,
        title: String(raw.videoDetails?.title || document.title || "YouTube Video").slice(0, 180),
        sabr: Boolean(raw.streamingData?.serverAbrStreamingUrl),
        sabrUrl: raw.streamingData?.serverAbrStreamingUrl || null,
        ustreamerConfig:
          raw.playerConfig?.mediaCommonConfig?.mediaUstreamerRequestConfig
            ?.videoPlaybackUstreamerConfig || "",
        clientName: Number(clientName) || 1,
        clientVersion: String(clientVersion || "").slice(0, 80),
        formats
      };
    } catch (_) {
      return null;
    }
  }

  function publishYoutubePlayerResponse(raw) {
    const payload = extractYoutubePlayerPayload(raw);
    if (!payload) return;
    window.postMessage(
      {
        source: "stormget-media-hook",
        type: "STORMGET_YOUTUBE_PLAYER_RESPONSE",
        data: payload,
        pageUrl: location.href
      },
      location.origin
    );
  }

  function inspectCurrentYoutubePagePlayer() {
    if (!isYoutubeHost()) return;
    try {
      if (window.ytInitialPlayerResponse) {
        publishYoutubePlayerResponse(window.ytInitialPlayerResponse);
      }
      const player = document.getElementById("movie_player");
      if (player && typeof player.getPlayerResponse === "function") {
        publishYoutubePlayerResponse(player.getPlayerResponse());
      }
    } catch (_) {}
  }

  function normalizeBaseUrl(rawUrl) {
    try {
      const u = new URL(rawUrl, location.href);
      return u.origin + u.pathname;
    } catch (_) {
      return rawUrl;
    }
  }

  function classifyMediaStream(url, mime) {
    if (!url || AD_DOMAINS.test(url)) return null;
    if (
      HLS_PATTERN.test(url) ||
      (mime && MANIFEST_MIMES.test(mime) && mime.toLowerCase().includes("mpegurl"))
    ) {
      return "hls";
    }
    if (
      DASH_PATTERN.test(url) ||
      (mime && MANIFEST_MIMES.test(mime) && mime.toLowerCase().includes("dash"))
    ) {
      return "dash";
    }
    if (VIDEO_PATTERN.test(url) || (mime && VIDEO_MIMES.test(mime))) {
      return "direct";
    }
    return null;
  }

  function emitDiscoveredStream(url, protocol, mime) {
    if (!url || url.startsWith("blob:") || url.startsWith("data:")) return;
    if (url.startsWith("http://localhost") || url.startsWith("http://127.0.0.1")) return;
    const lower = url.toLowerCase();
    if (
      (lower.endsWith(".ts") || lower.includes(".m4s")) &&
      !lower.includes(".m3u8") &&
      !lower.includes(".mpd")
    ) {
      return;
    }

    const dedupKey = `${protocol}::${normalizeBaseUrl(url)}`;
    if (seenStreams.has(dedupKey)) return;
    seenStreams.add(dedupKey);

    window.postMessage(
      {
        source: "stormget-media-hook",
        type: "STORMGET_MEDIA_DETECTED",
        url,
        protocol,
        mime: mime || null,
        pageUrl: location.href
      },
      "*"
    );
  }

  // 1. اعتراض window.fetch() (بما في ذلك /youtubei/v1/player لبث SABR)
  if (typeof window.fetch === "function") {
    const originalFetch = window.fetch;
    window.fetch = function (input, init) {
      let targetUrl;
      try {
        if (typeof input === "string") {
          targetUrl = new URL(input, location.href).toString();
        } else if (input instanceof URL) {
          targetUrl = input.toString();
        } else if (input && typeof input.url === "string") {
          targetUrl = input.url;
        }
      } catch (_) {}

      if (targetUrl) {
        const proto = classifyMediaStream(targetUrl);
        if (proto) emitDiscoveredStream(targetUrl, proto, null);
      }

      const result = originalFetch.apply(this, arguments);

      if (targetUrl && isYoutubePlayerEndpoint(targetUrl)) {
        result
          .then(async (response) => {
            if (
              !response ||
              !response.ok ||
              Number(response.headers?.get?.("content-length")) > MAX_PLAYER_JSON_BYTES
            ) {
              return;
            }
            const reader = response.clone().body?.getReader?.();
            if (!reader) return;
            const chunks = [];
            let size = 0;
            try {
              while (true) {
                const { done, value } = await reader.read();
                if (done) break;
                size += value.length;
                if (size > MAX_PLAYER_JSON_BYTES) {
                  void reader.cancel();
                  return;
                }
                chunks.push(value);
              }
              const bytes = new Uint8Array(size);
              let offset = 0;
              for (const c of chunks) {
                bytes.set(c, offset);
                offset += c.length;
              }
              publishYoutubePlayerResponse(
                JSON.parse(new TextDecoder().decode(bytes))
              );
            } catch (_) {}
          })
          .catch(() => {});
      }

      return result.then((resp) => {
        try {
          if (targetUrl && resp && resp.ok && resp.headers) {
            const ct = resp.headers.get("content-type");
            if (ct) {
              const proto = classifyMediaStream(targetUrl, ct);
              if (proto) emitDiscoveredStream(targetUrl, proto, ct);
            }
          }
        } catch (_) {}
        return resp;
      });
    };
  }

  // 2. اعتراض XMLHttpRequest (بما في ذلك /youtubei/v1/player)
  if (typeof XMLHttpRequest !== "undefined") {
    const XHRProto = XMLHttpRequest.prototype;
    const originalOpen = XHRProto.open;
    const originalSend = XHRProto.send;

    XHRProto.open = function (method, url) {
      try {
        const resolved = new URL(
          typeof url === "string" ? url : String(url),
          location.href
        ).toString();
        this.__stormget_url = resolved;
        const proto = classifyMediaStream(resolved);
        if (proto) emitDiscoveredStream(resolved, proto, null);

        if (isYoutubePlayerEndpoint(resolved)) {
          this.addEventListener(
            "load",
            function () {
              try {
                if (this.responseType === "json" && this.response) {
                  publishYoutubePlayerResponse(this.response);
                } else if (
                  (this.responseType === "" || this.responseType === "text") &&
                  typeof this.responseText === "string" &&
                  this.responseText.length <= MAX_PLAYER_JSON_BYTES
                ) {
                  publishYoutubePlayerResponse(JSON.parse(this.responseText));
                }
              } catch (_) {}
            },
            { once: true }
          );
        }
      } catch (_) {}
      return originalOpen.apply(this, arguments);
    };

    XHRProto.send = function () {
      this.addEventListener(
        "load",
        function () {
          try {
            const u = this.__stormget_url;
            if (!u) return;
            const ct = this.getResponseHeader("content-type");
            if (ct) {
              const proto = classifyMediaStream(u, ct);
              if (proto) emitDiscoveredStream(u, proto, ct);
            }
          } catch (_) {}
        },
        { once: true }
      );
      return originalSend.apply(this, arguments);
    };
  }

  // 3. تتبع URL.createObjectURL(MediaSource)
  if (typeof URL !== "undefined" && typeof URL.createObjectURL === "function") {
    const originalCreateObjectURL = URL.createObjectURL;
    URL.createObjectURL = function (obj) {
      const blobUrl = originalCreateObjectURL.apply(this, arguments);
      try {
        if (typeof MediaSource !== "undefined" && obj instanceof MediaSource) {
          window.postMessage(
            {
              source: "stormget-media-hook",
              type: "STORMGET_MSE_BLOB_CREATED",
              blobUrl,
              pageUrl: location.href
            },
            "*"
          );
        }
      } catch (_) {}
      return blobUrl;
    };
  }

  // 4. تتبع MediaSource.prototype.addSourceBuffer
  if (typeof MediaSource !== "undefined" && MediaSource.prototype) {
    const originalAddSourceBuffer = MediaSource.prototype.addSourceBuffer;
    if (typeof originalAddSourceBuffer === "function") {
      MediaSource.prototype.addSourceBuffer = function (mimeType) {
        const sb = originalAddSourceBuffer.apply(this, arguments);
        try {
          window.postMessage(
            {
              source: "stormget-media-hook",
              type: "STORMGET_MSE_SOURCE_BUFFER",
              mimeType: String(mimeType || ""),
              pageUrl: location.href
            },
            "*"
          );
        } catch (_) {}
        return sb;
      };
    }
  }

  // 5. أحداث مشغل يوتيوب (yt-navigate-finish + stormget-request-player)
  window.addEventListener("stormget-request-player", inspectCurrentYoutubePagePlayer);
  window.addEventListener("yt-navigate-finish", inspectCurrentYoutubePagePlayer);
  window.addEventListener("DOMContentLoaded", inspectCurrentYoutubePagePlayer, { once: true });
  setTimeout(inspectCurrentYoutubePagePlayer, 500);
})();
