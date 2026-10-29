/**
 * StormGet (RustIDM v2.3.0) — Isolated Closed Shadow DOM Video Overlay & Link Grabber (`content.js`)
 * يجمع بين:
 * 1. عزل كامل للزر العائم داخل `Closed Shadow DOM` مع `:host { all: initial }` وتصميم زجاجي داكن حديث (مستوحى من `dlman`)
 *    بحيث لا تستطيع صفحات الويب إخفاءه أو تشويه تنسيقه، مع زر إغلاق (`×`) لكل فيديو.
 * 2. استقبال روابط البث المكتشفة من `media-hook.js` (`MAIN world`: `fetch`, `XHR`, `MediaSource`) بالإضافة إلى `webRequest`.
 * 3. ترحيل بيانات مشغل يوتيوب (`STORMGET_YOUTUBE_PLAYER_RESPONSE`) لدعم بروتوكول يوتيوب الحديث `SABR` (`sabr=1`) والجودات الكاملة.
 * 4. فحص DRM (`mediaKeys` / `webkitKeys`) واستخراج الجودات الفعلية ومعدلات البت والأحجام التقديرية.
 * 5. نافذة تجديد الرابط منتهي الصلاحية (`Refresh Expired Download Address Modal`).
 * 6. نافذة سحب كافة روابط الصفحة الدفعية (`Download All Links with StormGet Modal`).
 */

(function () {
  if (window.__rustIdmContentInjected) return;
  window.__rustIdmContentInjected = true;

  // حقن احتياطي لـ media-hook.js في سياق الصفحة للمتصفحات التي لا تدعم world: "MAIN" في manifest.json (مثل بعض إصدارات Firefox)
  try {
    if (typeof chrome !== "undefined" && chrome.runtime && chrome.runtime.getURL) {
      const hookScript = document.createElement("script");
      hookScript.src = chrome.runtime.getURL("media-hook.js");
      hookScript.onload = () => hookScript.remove();
      (document.head || document.documentElement).appendChild(hookScript);
    }
  } catch (_) {}

  let activeVideoElement = null;
  let discoveredStreams = [];
  let cachedQualitiesKey = "";
  let cachedProbeResult = null;
  let isDropdownOpen = false;
  let lastPageUrl = location.href;
  const dismissedVideos = new WeakSet();

  // إنشاء حاوية Closed Shadow DOM معزولة تماماً عن CSS وجافاسكربت الصفحة المضيفة
  const hostEl = document.createElement("stormget-overlay-host");
  hostEl.id = "rustidm-floating-host";
  hostEl.style.cssText =
    "all:initial;position:fixed;top:0;left:0;width:0;height:0;z-index:2147483647;pointer-events:none;display:block;";
  document.documentElement.appendChild(hostEl);

  const shadow = hostEl.attachShadow({ mode: "closed" });

  const styleEl = document.createElement("style");
  styleEl.textContent = `
    :host {
      all: initial;
      position: fixed;
      z-index: 2147483647;
      display: block;
      pointer-events: none;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Tahoma, Arial, sans-serif;
    }
    *, *::before, *::after {
      box-sizing: border-box;
      margin: 0;
      padding: 0;
    }
    .idm-widget {
      position: fixed;
      display: none;
      pointer-events: auto;
      font-size: 12px;
      user-select: none;
      z-index: 2147483647;
    }
    .idm-widget.visible {
      display: block;
    }
    .idm-main-btn {
      display: inline-flex;
      align-items: center;
      gap: 7px;
      height: 32px;
      padding: 0 12px 0 9px;
      background: rgba(18, 24, 38, 0.92);
      color: #f1f5f9;
      font-size: 12px;
      font-weight: 600;
      border: 1px solid rgba(255, 255, 255, 0.14);
      border-radius: 7px;
      cursor: pointer;
      white-space: nowrap;
      backdrop-filter: blur(12px);
      -webkit-backdrop-filter: blur(12px);
      box-shadow: 0 4px 16px rgba(0, 0, 0, 0.55);
      transition: background 0.15s ease, border-color 0.15s ease, transform 0.15s ease;
      position: relative;
    }
    .idm-main-btn:hover {
      background: rgba(30, 41, 59, 0.96);
      border-color: rgba(59, 130, 246, 0.55);
    }
    .idm-orb {
      width: 18px;
      height: 18px;
      border-radius: 4px;
      background: linear-gradient(135deg, #3b82f6, #2563eb);
      display: inline-flex;
      align-items: center;
      justify-content: center;
      box-shadow: 0 1px 4px rgba(59, 130, 246, 0.45);
      flex-shrink: 0;
    }
    .idm-orb svg {
      width: 12px;
      height: 12px;
      color: #ffffff;
    }
    .idm-caret {
      border-left: 4px solid transparent;
      border-right: 4px solid transparent;
      border-top: 5px solid #94a3b8;
      margin-left: 3px;
      transition: transform 0.2s ease;
    }
    .idm-main-btn.open .idm-caret {
      transform: rotate(180deg);
    }
    .idm-dismiss-btn {
      position: absolute;
      top: -6px;
      right: -6px;
      width: 16px;
      height: 16px;
      border-radius: 50%;
      background: #1e293b;
      color: #94a3b8;
      border: 1px solid #475569;
      font-size: 10px;
      line-height: 14px;
      text-align: center;
      cursor: pointer;
      opacity: 0;
      transition: opacity 0.15s ease, background 0.12s ease, color 0.12s ease;
    }
    .idm-main-btn:hover .idm-dismiss-btn {
      opacity: 1;
    }
    .idm-dismiss-btn:hover {
      background: #dc2626;
      color: #ffffff;
      border-color: #dc2626;
    }
    .idm-menu {
      margin-top: 6px;
      min-width: 280px;
      max-width: 390px;
      background: rgba(18, 24, 38, 0.96);
      border: 1px solid rgba(255, 255, 255, 0.14);
      border-radius: 8px;
      box-shadow: 0 10px 32px rgba(0, 0, 0, 0.72);
      backdrop-filter: blur(16px);
      -webkit-backdrop-filter: blur(16px);
      overflow: hidden;
      display: none;
    }
    .idm-menu.open {
      display: block;
    }
    .idm-menu-header {
      background: rgba(30, 41, 59, 0.85);
      padding: 7px 12px;
      font-size: 11px;
      font-weight: 700;
      color: #94a3b8;
      border-bottom: 1px solid rgba(255, 255, 255, 0.08);
      display: flex;
      justify-content: space-between;
      align-items: center;
    }
    .idm-menu-item {
      padding: 8px 12px;
      display: flex;
      justify-content: space-between;
      align-items: center;
      gap: 10px;
      color: #e2e8f0;
      cursor: pointer;
      border-bottom: 1px solid rgba(255, 255, 255, 0.05);
      transition: background 0.12s ease;
    }
    .idm-menu-item:hover {
      background: rgba(59, 130, 246, 0.35);
      color: #ffffff;
    }
    .idm-badge {
      background: rgba(51, 65, 85, 0.85);
      color: #cbd5e1;
      border-radius: 4px;
      padding: 2px 6px;
      font-size: 10.5px;
      font-weight: 600;
      white-space: nowrap;
    }
    .idm-status-row {
      padding: 10px 12px;
      color: #94a3b8;
      font-style: italic;
    }
    .idm-error-row {
      padding: 10px 12px;
      color: #fca5a5;
      background: rgba(127, 29, 29, 0.35);
    }
    .idm-modal-backdrop {
      position: fixed;
      inset: 0;
      background: rgba(15, 23, 42, 0.65);
      display: none;
      align-items: center;
      justify-content: center;
      pointer-events: auto;
      z-index: 2147483647;
      font-size: 12.5px;
    }
    .idm-modal-backdrop.open {
      display: flex;
    }
    .idm-modal-card {
      background: #f8fafc;
      border: 1px solid #64748b;
      border-radius: 6px;
      width: 560px;
      max-width: 94vw;
      max-height: 82vh;
      display: flex;
      flex-direction: column;
      box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
      color: #1e293b;
    }
    .idm-modal-titlebar {
      background: linear-gradient(180deg, #e2e8f0 0%, #cbd5e1 100%);
      padding: 8px 12px;
      font-weight: 700;
      display: flex;
      justify-content: space-between;
      align-items: center;
      border-bottom: 1px solid #94a3b8;
    }
    .idm-modal-body {
      padding: 12px;
      overflow-y: auto;
      display: flex;
      flex-direction: column;
      gap: 8px;
    }
    .idm-modal-footer {
      padding: 8px 12px;
      background: #f1f5f9;
      border-top: 1px solid #cbd5e1;
      display: flex;
      justify-content: flex-end;
      gap: 8px;
    }
    .idm-btn {
      padding: 5px 12px;
      border: 1px solid #475569;
      border-radius: 4px;
      background: #ffffff;
      color: #0f172a;
      cursor: pointer;
      font-weight: 600;
    }
    .idm-btn.primary {
      background: #2563eb;
      border-color: #1d4ed8;
      color: #ffffff;
    }
    .idm-link-list {
      max-height: 240px;
      overflow-y: auto;
      background: #ffffff;
      border: 1px solid #cbd5e1;
      padding: 6px;
    }
    .idm-link-row {
      display: flex;
      align-items: center;
      gap: 6px;
      padding: 3px 4px;
      border-bottom: 1px solid #f1f5f9;
      font-size: 11.5px;
      word-break: break-all;
    }
    .idm-input {
      padding: 5px 8px;
      border: 1px solid #94a3b8;
      border-radius: 4px;
      width: 100%;
      box-sizing: border-box;
    }
  `;

  const widgetEl = document.createElement("div");
  widgetEl.className = "idm-widget";
  widgetEl.innerHTML = `
    <div class="idm-main-btn" id="idmTriggerBtn" title="Download this video with StormGet">
      <span class="idm-orb">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
          <polyline points="7 10 12 15 17 10"/>
          <line x1="12" y1="15" x2="12" y2="3"/>
        </svg>
      </span>
      <span id="idmBtnLabel">Download Video</span>
      <span class="idm-caret"></span>
      <span class="idm-dismiss-btn" id="idmDismissBtn" title="Hide button for this video">×</span>
    </div>
    <div class="idm-menu" id="idmQualityMenu"></div>
  `;

  const modalEl = document.createElement("div");
  modalEl.className = "idm-modal-backdrop";
  modalEl.innerHTML = `
    <div class="idm-modal-card">
      <div class="idm-modal-titlebar">
        <span id="idmModalTitle">StormGet (RustIDM)</span>
        <button class="idm-btn" id="idmModalCloseBtn">×</button>
      </div>
      <div class="idm-modal-body" id="idmModalBody"></div>
      <div class="idm-modal-footer" id="idmModalFooter"></div>
    </div>
  `;

  shadow.appendChild(styleEl);
  shadow.appendChild(widgetEl);
  shadow.appendChild(modalEl);

  const triggerBtn = widgetEl.querySelector("#idmTriggerBtn");
  const dismissBtn = widgetEl.querySelector("#idmDismissBtn");
  const btnLabel = widgetEl.querySelector("#idmBtnLabel");
  const qualityMenu = widgetEl.querySelector("#idmQualityMenu");
  const modalTitle = modalEl.querySelector("#idmModalTitle");
  const modalBody = modalEl.querySelector("#idmModalBody");
  const modalFooter = modalEl.querySelector("#idmModalFooter");
  const modalCloseBtn = modalEl.querySelector("#idmModalCloseBtn");

  modalCloseBtn.addEventListener("click", () => {
    modalEl.classList.remove("open");
  });

  dismissBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    e.preventDefault();
    if (activeVideoElement) {
      dismissedVideos.add(activeVideoElement);
    }
    isDropdownOpen = false;
    qualityMenu.classList.remove("open");
    triggerBtn.classList.remove("open");
    widgetEl.classList.remove("visible");
  });

  // استقبال الروابط وبيانات مشغل يوتيوب SABR من MAIN-world hook (`media-hook.js`)
  window.addEventListener("message", (event) => {
    if (event.source !== window || !event.data) return;
    if (event.data.source !== "stormget-media-hook") return;

    if (event.data.type === "STORMGET_YOUTUBE_PLAYER_RESPONSE") {
      const playerData = event.data.data;
      if (
        playerData &&
        /^[\w-]{11}$/.test(playerData.videoId || "") &&
        Array.isArray(playerData.formats) &&
        playerData.formats.length <= 150
      ) {
        cachedQualitiesKey = "";
        chrome.runtime.sendMessage(
          {
            type: "RUSTIDM_YOUTUBE_PLAYER_RESPONSE",
            data: playerData,
            pageUrl: location.href
          },
          () => void chrome.runtime.lastError
        );
      }
      return;
    }

    if (event.data.type === "STORMGET_MEDIA_DETECTED") {
      const { url, protocol, hookSource } = event.data;
      if (url && !discoveredStreams.some((s) => s.url === url)) {
        const streamEntry = {
          url,
          protocol: protocol || "direct",
          hookSource: hookSource || "main-hook"
        };
        discoveredStreams.push(streamEntry);
        cachedQualitiesKey = "";
        chrome.runtime.sendMessage(
          {
            type: "RUSTIDM_REPORT_HOOK_STREAM",
            url,
            protocol: protocol || "direct",
            hookSource: hookSource || "main-hook"
          },
          () => void chrome.runtime.lastError
        );
        detectPrimaryVideo();
      }
    }
  });

  function formatBytes(bytes) {
    if (!bytes || bytes <= 0) return "";
    if (bytes >= 1024 * 1024 * 1024) {
      return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
    }
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function isVideoDrmProtected(videoEl) {
    if (!videoEl) return false;
    return Boolean(videoEl.mediaKeys || videoEl.webkitKeys);
  }

  function updateWidgetPosition() {
    if (
      !activeVideoElement ||
      !document.contains(activeVideoElement) ||
      dismissedVideos.has(activeVideoElement)
    ) {
      widgetEl.classList.remove("visible");
      return;
    }

    const rect = activeVideoElement.getBoundingClientRect();
    if (
      rect.width < 180 ||
      rect.height < 110 ||
      rect.bottom < 0 ||
      rect.top > window.innerHeight
    ) {
      widgetEl.classList.remove("visible");
      return;
    }

    const top = Math.max(8, rect.top + 10);
    const left = Math.max(8, Math.min(window.innerWidth - 210, rect.right - 190));

    widgetEl.style.top = `${top}px`;
    widgetEl.style.left = `${left}px`;
    widgetEl.classList.add("visible");
  }

  function detectPrimaryVideo() {
    if (location.href !== lastPageUrl) {
      lastPageUrl = location.href;
      cachedQualitiesKey = "";
      cachedProbeResult = null;
      qualityMenu.classList.remove("open");
      triggerBtn.classList.remove("open");
      isDropdownOpen = false;
      window.dispatchEvent(new Event("stormget-request-player"));
    }

    const videos = Array.from(document.querySelectorAll("video"));
    let bestVideo = null;
    let bestArea = 0;

    for (const v of videos) {
      if (dismissedVideos.has(v)) continue;
      const r = v.getBoundingClientRect();
      const area = r.width * r.height;
      if (r.width >= 180 && r.height >= 110 && area > bestArea) {
        bestArea = area;
        bestVideo = v;
      }
    }

    activeVideoElement = bestVideo;
    updateWidgetPosition();
  }

  function pickPrimaryStreamUrl() {
    const hls = discoveredStreams.find((s) => s.protocol === "hls");
    if (hls) return hls.url;
    const dash = discoveredStreams.find((s) => s.protocol === "dash");
    if (dash) return dash.url;
    if (discoveredStreams.length > 0) {
      return discoveredStreams[discoveredStreams.length - 1].url;
    }
    if (
      activeVideoElement &&
      activeVideoElement.currentSrc &&
      activeVideoElement.currentSrc.startsWith("http")
    ) {
      return activeVideoElement.currentSrc;
    }
    return location.href;
  }

  function renderQualityMenu(probe) {
    qualityMenu.innerHTML = "";

    const header = document.createElement("div");
    header.className = "idm-menu-header";
    header.textContent = "اختر جودة التحميل (StormGet v2.3)";
    qualityMenu.appendChild(header);

    if (probe.drm_protected || isVideoDrmProtected(activeVideoElement)) {
      const drmRow = document.createElement("div");
      drmRow.className = "idm-error-row";
      drmRow.textContent =
        probe.drm_reason ||
        "هذا الفيديو محمي بنظام EME/DRM (Widevine/PlayReady) ولا يمكن تنزيله.";
      qualityMenu.appendChild(drmRow);
      return;
    }

    if (!probe.qualities || probe.qualities.length === 0) {
      const emptyRow = document.createElement("div");
      emptyRow.className = "idm-status-row";
      emptyRow.textContent =
        probe.notice || "لم يتم العثور على جودات متاحة لهذا الفيديو. شغّل الفيديو لثوانٍ ثم أعد المحاولة.";
      qualityMenu.appendChild(emptyRow);
      return;
    }

    for (const opt of probe.qualities) {
      const row = document.createElement("div");
      row.className = "idm-menu-item";

      const titleSpan = document.createElement("span");
      titleSpan.textContent = opt.label;

      const badgeSpan = document.createElement("span");
      badgeSpan.className = "idm-badge";
      const sizeStr = formatBytes(opt.estimated_size_bytes);
      badgeSpan.textContent = sizeStr || opt.protocol.toUpperCase();

      row.appendChild(titleSpan);
      row.appendChild(badgeSpan);

      row.addEventListener("click", (e) => {
        e.stopPropagation();
        startVideoDownload(opt, probe.title);
      });

      qualityMenu.appendChild(row);
    }
  }

  function startVideoDownload(qualityOption, videoTitle) {
    btnLabel.textContent = "Sending to StormGet...";
    qualityMenu.classList.remove("open");
    triggerBtn.classList.remove("open");
    isDropdownOpen = false;

    chrome.runtime.sendMessage(
      {
        type: "RUSTIDM_DOWNLOAD_VIDEO_QUALITY",
        qualityOption,
        title: videoTitle,
        pageUrl: location.href,
        pageTitle: document.title
      },
      (resp) => {
        if (resp && resp.ok) {
          btnLabel.textContent = "Downloading ✓";
          setTimeout(() => {
            btnLabel.textContent = "Download Video";
          }, 3000);
        } else {
          btnLabel.textContent = "Engine Offline!";
          setTimeout(() => {
            btnLabel.textContent = "Download Video";
          }, 3000);
        }
      }
    );
  }

  triggerBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    isDropdownOpen = !isDropdownOpen;
    if (!isDropdownOpen) {
      qualityMenu.classList.remove("open");
      triggerBtn.classList.remove("open");
      return;
    }

    // تحديث بيانات مشغل يوتيوب (SABR) لحظياً قبل طلب الجودات
    window.dispatchEvent(new Event("stormget-request-player"));

    qualityMenu.classList.add("open");
    triggerBtn.classList.add("open");
    const streamUrl = pickPrimaryStreamUrl();
    const cacheKey = `${location.href}::${streamUrl}`;

    if (cachedQualitiesKey === cacheKey && cachedProbeResult) {
      renderQualityMenu(cachedProbeResult);
      return;
    }

    qualityMenu.innerHTML = `<div class="idm-status-row">جاري استخراج الجودات المتاحة...</div>`;

    chrome.runtime.sendMessage(
      {
        type: "RUSTIDM_PROBE_QUALITIES",
        streamUrl,
        pageUrl: location.href,
        pageTitle: document.title
      },
      (resp) => {
        if (!resp || !resp.ok) {
          qualityMenu.innerHTML = `<div class="idm-error-row">${
            (resp && resp.error) || "تعذر الاتصال بمحرك StormGet"
          }</div>`;
          return;
        }
        cachedQualitiesKey = cacheKey;
        cachedProbeResult = resp.probe;
        renderQualityMenu(resp.probe);
      }
    );
  });

  function openBatchGrabberModal() {
    modalTitle.textContent = "Download All Links with StormGet (Batch Page Grabber)";
    modalBody.innerHTML = "";
    modalFooter.innerHTML = "";

    const anchors = Array.from(
      document.querySelectorAll("a[href], video[src], source[src]")
    );
    const uniqueUrls = new Set();
    for (const el of anchors) {
      const raw = el.href || el.src;
      if (raw && raw.startsWith("http")) {
        uniqueUrls.add(raw);
      }
    }
    const allUrls = Array.from(uniqueUrls);

    const filterInput = document.createElement("input");
    filterInput.className = "idm-input";
    filterInput.placeholder =
      "Filter by extension or keyword (e.g. .mp4, .pdf, .zip)...";

    const listContainer = document.createElement("div");
    listContainer.className = "idm-link-list";

    const selectedMap = new Map();
    allUrls.forEach((u) => selectedMap.set(u, true));

    function renderLinks(filterText) {
      listContainer.innerHTML = "";
      const kw = (filterText || "").trim().toLowerCase();
      for (const u of allUrls) {
        if (kw && !u.toLowerCase().includes(kw)) continue;
        const row = document.createElement("label");
        row.className = "idm-link-row";
        const cb = document.createElement("input");
        cb.type = "checkbox";
        cb.checked = selectedMap.get(u) !== false;
        cb.addEventListener("change", () => selectedMap.set(u, cb.checked));
        const span = document.createElement("span");
        span.textContent = u;
        row.appendChild(cb);
        row.appendChild(span);
        listContainer.appendChild(row);
      }
    }

    filterInput.addEventListener("input", () => renderLinks(filterInput.value));
    renderLinks("");

    modalBody.appendChild(filterInput);
    modalBody.appendChild(listContainer);

    const submitBtn = document.createElement("button");
    submitBtn.className = "idm-btn primary";
    submitBtn.textContent = "Queue Selected Links in StormGet";
    submitBtn.addEventListener("click", () => {
      const kw = (filterInput.value || "").trim().toLowerCase();
      const chosen = allUrls.filter(
        (u) => selectedMap.get(u) && (!kw || u.toLowerCase().includes(kw))
      );
      if (chosen.length === 0) return;
      chrome.runtime.sendMessage(
        {
          type: "RUSTIDM_BATCH_ENQUEUE_LINKS",
          urls: chosen,
          pageUrl: location.href,
          queueName: "main",
          startImmediately: true
        },
        () => {
          modalEl.classList.remove("open");
        }
      );
    });

    modalFooter.appendChild(submitBtn);
    modalEl.classList.add("open");
  }

  function openRefreshUrlModal(candidateUrl) {
    modalTitle.textContent = "Refresh Expired Download Link in StormGet";
    modalBody.innerHTML = `<div class="idm-status-row">جاري جلب المهام المتوقفة من محرك StormGet...</div>`;
    modalFooter.innerHTML = "";
    modalEl.classList.add("open");

    chrome.runtime.sendMessage({ type: "RUSTIDM_LIST_EXPIRED_TASKS" }, (resp) => {
      modalBody.innerHTML = "";
      if (!resp || !resp.ok || !resp.tasks || resp.tasks.length === 0) {
        modalBody.innerHTML = `<div class="idm-error-row">لا توجد مهام متوقفة أو منتهية الصلاحية حالياً في محرك StormGet.</div>`;
        return;
      }

      const selectEl = document.createElement("select");
      selectEl.className = "idm-input";
      for (const t of resp.tasks) {
        const opt = document.createElement("option");
        opt.value = t.task_id;
        opt.textContent = `${t.filename} (${t.status})`;
        selectEl.appendChild(opt);
      }

      const urlInput = document.createElement("input");
      urlInput.className = "idm-input";
      urlInput.value = candidateUrl || "";

      modalBody.appendChild(selectEl);
      modalBody.appendChild(urlInput);

      const applyBtn = document.createElement("button");
      applyBtn.className = "idm-btn primary";
      applyBtn.textContent = "Verify ETag & Resume Download";
      applyBtn.addEventListener("click", () => {
        chrome.runtime.sendMessage(
          {
            type: "RUSTIDM_SUBMIT_REFRESH_URL",
            taskId: selectEl.value,
            newUrl: urlInput.value.trim()
          },
          () => {
            modalEl.classList.remove("open");
          }
        );
      });
      modalFooter.appendChild(applyBtn);
    });
  }

  const CLICK_INTERCEPT_EXTENSIONS = new Set([
    "3gp", "7z", "aac", "ace", "aif", "apk", "arj", "asf", "avi", "bin",
    "bz2", "deb", "dmg", "doc", "docx", "epub", "exe", "flac", "flv", "gz",
    "gzip", "img", "iso", "lzh", "m4a", "m4v", "mkv", "mov", "mp3", "mp4",
    "mpa", "mpe", "mpeg", "mpg", "msi", "msu", "ogg", "ogv", "pdf", "plj",
    "pps", "ppt", "pptx", "qt", "ra", "rar", "rm", "rmvb", "rpm", "sea",
    "sit", "sitx", "tar", "tif", "tiff", "wav", "webm", "wma", "wmv", "xls",
    "xlsx", "xz", "z", "zip", "zst"
  ]);

  function isLinkTargetInterceptable(url, downloadAttr) {
    if (downloadAttr) {
      const cleanDl = downloadAttr.split(/[?#]/)[0].trim().toLowerCase();
      const dotIdx = cleanDl.lastIndexOf(".");
      if (
        dotIdx !== -1 &&
        CLICK_INTERCEPT_EXTENSIONS.has(cleanDl.slice(dotIdx + 1))
      ) {
        return true;
      }
    }
    if (!url || (!url.startsWith("http://") && !url.startsWith("https://"))) {
      return false;
    }
    try {
      const u = new URL(url);
      const cleanPath = u.pathname.trim().toLowerCase();
      const dotIdx = cleanPath.lastIndexOf(".");
      if (
        dotIdx !== -1 &&
        CLICK_INTERCEPT_EXTENSIONS.has(cleanPath.slice(dotIdx + 1))
      ) {
        return true;
      }
      for (const [_, val] of u.searchParams.entries()) {
        const v = (val || "").trim().toLowerCase();
        const vDot = v.lastIndexOf(".");
        if (vDot !== -1 && CLICK_INTERCEPT_EXTENSIONS.has(v.slice(vDot + 1))) {
          return true;
        }
      }
    } catch (_) {}
    return false;
  }

  document.addEventListener(
    "click",
    (e) => {
      if (e.altKey) return;
      const anchor = e.target.closest && e.target.closest("a");
      if (!anchor || !anchor.href) return;
      const downloadAttr = anchor.getAttribute("download");
      if (isLinkTargetInterceptable(anchor.href, downloadAttr)) {
        e.preventDefault();
        e.stopPropagation();
        chrome.runtime.sendMessage({
          type: "RUSTIDM_INTERCEPT_LINK",
          url: anchor.href,
          filename: downloadAttr || null,
          pageUrl: location.href
        });
      }
    },
    true
  );

  document.addEventListener("click", () => {
    if (isDropdownOpen) {
      isDropdownOpen = false;
      qualityMenu.classList.remove("open");
      triggerBtn.classList.remove("open");
    }
  });

  chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
    if (!msg) return;
    if (msg.type === "RUSTIDM_REFRESH_YOUTUBE_PLAYER") {
      window.dispatchEvent(new Event("stormget-request-player"));
      if (typeof sendResponse === "function") sendResponse({ ok: true });
      return;
    }
    if (msg.type === "RUSTIDM_STREAMS_UPDATED") {
      const incoming = msg.streams || [];
      for (const s of incoming) {
        if (!discoveredStreams.some((x) => x.url === s.url)) {
          discoveredStreams.push(s);
        }
      }
      cachedQualitiesKey = "";
      detectPrimaryVideo();
    } else if (msg.type === "RUSTIDM_OPEN_BATCH_GRABBER_MODAL") {
      openBatchGrabberModal();
    } else if (msg.type === "RUSTIDM_OPEN_REFRESH_MODAL") {
      openRefreshUrlModal(msg.candidateUrl);
    }
  });

  chrome.runtime.sendMessage({ type: "RUSTIDM_GET_TAB_STREAMS" }, (resp) => {
    if (resp && resp.ok && Array.isArray(resp.streams)) {
      for (const s of resp.streams) {
        if (!discoveredStreams.some((x) => x.url === s.url)) {
          discoveredStreams.push(s);
        }
      }
    }
    detectPrimaryVideo();
  });

  window.addEventListener("scroll", updateWidgetPosition, {
    passive: true,
    capture: true
  });
  window.addEventListener("resize", updateWidgetPosition, { passive: true });
  setInterval(detectPrimaryVideo, 1000);
})();
