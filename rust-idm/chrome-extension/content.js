/**
 * RustIDM Integration Module v1.1.0 - Content Script (Manifest V3)
 * يميز بدقة بين:
 * 1. روابط الفيديو المباشرة (Direct MP4/WebM).
 * 2. قوائم البث المتكيف المكتشفة عبر الشبكة (HLS .m3u8 / MPEG-DASH .mpd).
 * 3. تدفقات MediaSource Extensions (blob:)؛ حيث ينبه المستخدم بوضوح إذا كان البث محمياً أو يتطلب اختيار قائمة البث.
 * 4. نافذة تجديد الرابط منتهي الصلاحية (Refresh Expired URL Modal).
 */

(function () {
  let detectedStreams = [];
  const attachedVideos = new WeakSet();

  chrome.runtime.sendMessage({ action: "GET_TAB_STREAMS" }, (res) => {
    if (res && Array.isArray(res.streams)) {
      detectedStreams = res.streams;
    }
  });

  chrome.runtime.onMessage.addListener((msg) => {
    if (msg.action === "STREAMS_UPDATED" && Array.isArray(msg.streams)) {
      detectedStreams = msg.streams;
    } else if (msg.action === "COLLECT_ALL_PAGE_LINKS") {
      showBatchGrabberModal();
    } else if (msg.action === "PROMPT_REFRESH_TASK_ID" && msg.new_url) {
      showRefreshTaskModal(msg.new_url);
    }
  });

  function scanAndAttachVideoOverlays() {
    const videos = document.querySelectorAll("video");
    videos.forEach((videoEl) => {
      if (attachedVideos.has(videoEl)) return;
      attachedVideos.add(videoEl);
      createFloatingVideoBar(videoEl);
    });
  }

  function createFloatingVideoBar(videoEl) {
    const container = document.createElement("div");
    container.className = "rustidm-floating-panel";
    Object.assign(container.style, {
      position: "fixed",
      zIndex: "2147483647",
      display: "none",
      direction: "rtl",
      fontFamily: "system-ui, -apple-system, sans-serif",
      background: "linear-gradient(135deg, #0b0f19 0%, #1e293b 100%)",
      color: "#f8fafc",
      padding: "6px 12px",
      borderRadius: "8px",
      boxShadow: "0 6px 20px rgba(0, 0, 0, 0.55)",
      border: "1px solid #f97316",
      fontSize: "13px",
      fontWeight: "600",
      cursor: "pointer",
      userSelect: "none",
      alignItems: "center",
      gap: "8px"
    });

    const badge = document.createElement("span");
    badge.textContent = "⚡ RustIDM";
    Object.assign(badge.style, {
      background: "#f97316",
      color: "#0b0f19",
      padding: "2px 6px",
      borderRadius: "4px",
      fontSize: "11px",
      fontWeight: "800"
    });

    const mainBtn = document.createElement("span");
    mainBtn.textContent = "تحميل هذا الفيديو";

    const dropdownMenu = document.createElement("div");
    Object.assign(dropdownMenu.style, {
      position: "absolute",
      top: "100%",
      right: "0",
      marginTop: "6px",
      background: "#0b0f19",
      border: "1px solid #334155",
      borderRadius: "8px",
      minWidth: "280px",
      display: "none",
      flexDirection: "column",
      overflow: "hidden",
      boxShadow: "0 10px 25px rgba(0,0,0,0.65)"
    });

    container.appendChild(badge);
    container.appendChild(mainBtn);
    container.appendChild(dropdownMenu);
    document.body.appendChild(container);

    function updatePosition() {
      const rect = videoEl.getBoundingClientRect();
      if (rect.width < 160 || rect.height < 100 || (videoEl.paused && rect.top < 0)) {
        container.style.display = "none";
        return;
      }
      container.style.top = `${Math.max(8, rect.top + 8)}px`;
      container.style.left = `${Math.max(8, rect.right - 235)}px`;
      container.style.display = "flex";
    }

    videoEl.addEventListener("mouseenter", updatePosition);
    videoEl.addEventListener("play", updatePosition);
    window.addEventListener(
      "scroll",
      () => {
        if (container.style.display !== "none") updatePosition();
      },
      { passive: true }
    );

    container.addEventListener("click", (e) => {
      e.stopPropagation();
      dropdownMenu.innerHTML = "";

      const available = [...detectedStreams];
      const src = videoEl.currentSrc || videoEl.src || "";
      const hasMediaKeys = Boolean(videoEl.mediaKeys);

      if (src && !src.startsWith("blob:")) {
        if (!available.some((s) => s.url === src)) {
          available.unshift({
            url: src,
            mime_type: "video/mp4",
            quality_label: "المصدر المباشر للمشغل (Direct File)"
          });
        }
      } else if (src.startsWith("blob:") && hasMediaKeys) {
        const drmNotice = document.createElement("div");
        drmNotice.textContent = "🔒 هذا المشغل يستخدم تشفير DRM/EME (Widevine/PlayReady) ولا يمكن استخراج البث المحمي.";
        Object.assign(drmNotice.style, {
          padding: "10px 12px",
          fontSize: "11px",
          color: "#fca5a5",
          background: "#450a0a",
          borderBottom: "1px solid #7f1d1d"
        });
        dropdownMenu.appendChild(drmNotice);
      } else if (src.startsWith("blob:") && available.length === 0) {
        available.push({
          url: window.location.href,
          mime_type: "video/mp4",
          quality_label: "تدفق MSE (استخراج عبر جسر yt-dlp لرابط الصفحة)"
        });
      }

      available.forEach((stream) => {
        const item = document.createElement("div");
        item.textContent = `🎬 ${stream.quality_label || "جودة قياسية"}`;
        Object.assign(item.style, {
          padding: "9px 12px",
          borderBottom: "1px solid #1e293b",
          fontSize: "12px",
          color: "#e2e8f0"
        });
        item.addEventListener("mouseenter", () => (item.style.background = "#1e293b"));
        item.addEventListener("mouseleave", () => (item.style.background = "transparent"));
        item.addEventListener("click", (ev) => {
          ev.stopPropagation();
          dropdownMenu.style.display = "none";
          mainBtn.textContent = "⏳ جارٍ الإرسال إلى RustIDM...";
          chrome.runtime.sendMessage(
            {
              action: "DOWNLOAD_VIDEO_STREAM",
              payload: {
                url: stream.url,
                page_title: document.title,
                mime_type: stream.mime_type,
                content_length: stream.content_length,
                quality_label: stream.quality_label
              }
            },
            (res) => {
              if (res && res.status === "video_download_started") {
                mainBtn.textContent = "✅ بدأ التحميل في RustIDM!";
              } else {
                mainBtn.textContent = "⚠️ شغّل محرك RustIDM أولاً";
              }
              setTimeout(() => {
                mainBtn.textContent = "تحميل هذا الفيديو";
              }, 3000);
            }
          );
        });
        dropdownMenu.appendChild(item);
      });

      dropdownMenu.style.display =
        dropdownMenu.style.display === "flex" ? "none" : "flex";
    });

    document.addEventListener("click", () => {
      dropdownMenu.style.display = "none";
    });
  }

  function showRefreshTaskModal(newUrl) {
    const existing = document.getElementById("rustidm-refresh-modal");
    if (existing) existing.remove();

    const overlay = document.createElement("div");
    overlay.id = "rustidm-refresh-modal";
    Object.assign(overlay.style, {
      position: "fixed",
      inset: "0",
      background: "rgba(11, 15, 25, 0.78)",
      zIndex: "2147483647",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      direction: "rtl",
      fontFamily: "system-ui, -apple-system, sans-serif"
    });

    const card = document.createElement("div");
    Object.assign(card.style, {
      background: "#0f172a",
      border: "1px solid #10b981",
      borderRadius: "12px",
      padding: "20px",
      width: "420px",
      maxWidth: "92vw",
      color: "#f8fafc"
    });

    const title = document.createElement("h3");
    title.textContent = "🔄 تجديد رابط مهمة التحميل (Refresh Expired URL)";
    Object.assign(title.style, { margin: "0 0 10px 0", fontSize: "15px", color: "#34d399" });

    const input = document.createElement("input");
    input.type = "text";
    input.placeholder = "أدخل معرف المهمة (task_id) في RustIDM...";
    Object.assign(input.style, {
      width: "100%",
      boxSizing: "border-box",
      padding: "8px 10px",
      borderRadius: "6px",
      border: "1px solid #334155",
      background: "#1e293b",
      color: "#f8fafc",
      marginBottom: "14px",
      fontSize: "13px"
    });

    const submitBtn = document.createElement("button");
    submitBtn.textContent = "تحديث الرابط واستئناف التحميل";
    Object.assign(submitBtn.style, {
      padding: "7px 14px",
      borderRadius: "6px",
      border: "none",
      background: "#10b981",
      color: "#0b0f19",
      fontWeight: "700",
      cursor: "pointer"
    });
    submitBtn.addEventListener("click", () => {
      const taskId = input.value.trim();
      if (!taskId) return;
      chrome.runtime.sendMessage({
        action: "REFRESH_EXPIRED_TASK_URL",
        payload: { task_id: taskId, new_url: newUrl }
      });
      overlay.remove();
    });

    card.appendChild(title);
    card.appendChild(input);
    card.appendChild(submitBtn);
    overlay.appendChild(card);
    document.body.appendChild(overlay);
  }

  function showBatchGrabberModal() {
    const existing = document.getElementById("rustidm-batch-modal");
    if (existing) existing.remove();

    const allAnchors = Array.from(document.querySelectorAll("a[href]"))
      .map((a) => a.href)
      .filter((href) => href.startsWith("http"));
    const uniqueLinks = [...new Set(allAnchors)];

    const overlay = document.createElement("div");
    overlay.id = "rustidm-batch-modal";
    Object.assign(overlay.style, {
      position: "fixed",
      inset: "0",
      background: "rgba(11, 15, 25, 0.78)",
      zIndex: "2147483647",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      direction: "rtl",
      fontFamily: "system-ui, -apple-system, sans-serif"
    });

    const card = document.createElement("div");
    Object.assign(card.style, {
      background: "#0f172a",
      border: "1px solid #f97316",
      borderRadius: "12px",
      padding: "20px",
      width: "420px",
      maxWidth: "92vw",
      color: "#f8fafc",
      boxShadow: "0 20px 40px rgba(0,0,0,0.65)"
    });

    const title = document.createElement("h3");
    title.textContent = `🕷 ساحب الروابط الذكي (RustIDM Grabber) — ${uniqueLinks.length} رابط`;
    Object.assign(title.style, { margin: "0 0 10px 0", fontSize: "15px", color: "#fb923c" });

    const desc = document.createElement("p");
    desc.textContent = "حدد امتدادات الملفات المطلوب تصفيتها وإرسالها إلى طابور RustIDM:";
    Object.assign(desc.style, { margin: "0 0 10px 0", fontSize: "13px", color: "#94a3b8" });

    const input = document.createElement("input");
    input.type = "text";
    input.value = "mp4,mkv,pdf,zip,rar,mp3";
    Object.assign(input.style, {
      width: "100%",
      boxSizing: "border-box",
      padding: "8px 10px",
      borderRadius: "6px",
      border: "1px solid #334155",
      background: "#1e293b",
      color: "#f8fafc",
      marginBottom: "14px",
      fontSize: "13px"
    });

    const actions = document.createElement("div");
    Object.assign(actions.style, { display: "flex", gap: "8px", justifyContent: "flex-end" });

    const cancelBtn = document.createElement("button");
    cancelBtn.textContent = "إلغاء";
    Object.assign(cancelBtn.style, {
      padding: "7px 14px",
      borderRadius: "6px",
      border: "1px solid #475569",
      background: "transparent",
      color: "#cbd5e1",
      cursor: "pointer"
    });
    cancelBtn.addEventListener("click", () => overlay.remove());

    const submitBtn = document.createElement("button");
    submitBtn.textContent = "إرسال إلى RustIDM";
    Object.assign(submitBtn.style, {
      padding: "7px 14px",
      borderRadius: "6px",
      border: "none",
      background: "#f97316",
      color: "#0b0f19",
      fontWeight: "700",
      cursor: "pointer"
    });
    submitBtn.addEventListener("click", () => {
      const filterExts = input.value
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
      chrome.runtime.sendMessage({
        action: "BATCH_DOWNLOAD_LINKS",
        payload: {
          page_url: window.location.href,
          links: uniqueLinks,
          filter_extensions: filterExts
        }
      });
      overlay.remove();
    });

    actions.appendChild(cancelBtn);
    actions.appendChild(submitBtn);
    card.appendChild(title);
    card.appendChild(desc);
    card.appendChild(input);
    card.appendChild(actions);
    overlay.appendChild(card);
    document.body.appendChild(overlay);
  }

  scanAndAttachVideoOverlays();
  const observer = new MutationObserver(() => scanAndAttachVideoOverlays());
  observer.observe(document.documentElement, { childList: true, subtree: true });
})();
