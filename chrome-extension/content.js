/**
 * RustIDM Video Download Panel & Page Link Grabber - Content Script (v2.0.0)
 * يعرض:
 * 1. الزر العائم الكلاسيكي (Download this video) فوق مشغلات الفيديو (YouTube / HTML5 / HLS / DASH)
 *    مع فحص MSE/EME DRM واستخراج الجودات الفعلية.
 * 2. نافذة تجديد الرابط منتهي الصلاحية (Refresh Expired Download Address Modal).
 * 3. نافذة سحب كافة روابط الصفحة الدفعية (Download All Links with RustIDM Modal).
 */

(function () {
  if (window.__rustIdmContentInjected) return;
  window.__rustIdmContentInjected = true;

  let activeVideoElement = null;
  let discoveredStreams = [];
  let cachedQualitiesKey = "";
  let cachedProbeResult = null;
  let isDropdownOpen = false;
  let lastPageUrl = location.href;

  // إنشاء حاوية Shadow DOM معزولة حتى لا تتأثر بتنسيقات الموقع
  const hostEl = document.createElement("div");
  hostEl.id = "rustidm-floating-host";
  hostEl.style.cssText =
    "position:fixed;top:0;left:0;width:0;height:0;z-index:2147483647;pointer-events:none;";
  document.documentElement.appendChild(hostEl);

  const shadow = hostEl.attachShadow({ mode: "open" });

  const styleEl = document.createElement("style");
  styleEl.textContent = `
    .idm-widget {
      position: fixed;
      display: none;
      pointer-events: auto;
      font-family: "Segoe UI", Tahoma, Arial, sans-serif;
      font-size: 12px;
      user-select: none;
      z-index: 2147483647;
    }
    .idm-widget.visible {
      display: block;
    }
    .idm-main-btn {
      display: flex;
      align-items: center;
      gap: 7px;
      background: linear-gradient(180deg, #f7fbff 0%, #dcebfa 52%, #bdd9f4 100%);
      border: 1px solid #5b87b5;
      border-radius: 4px;
      padding: 4px 10px;
      color: #102a43;
      font-weight: 600;
      cursor: pointer;
      box-shadow: 0 2px 8px rgba(0, 0, 0, 0.28);
      transition: filter 0.12s ease;
    }
    .idm-main-btn:hover {
      filter: brightness(1.04);
      border-color: #2b6cb0;
    }
    .idm-orb {
      width: 15px;
      height: 15px;
      border-radius: 50%;
      background: radial-gradient(circle at 30% 30%, #68d391, #2f855a 65%, #2b6cb0 100%);
      border: 1px solid #1e4e8c;
      position: relative;
      flex-shrink: 0;
    }
    .idm-caret {
      border-left: 4px solid transparent;
      border-right: 4px solid transparent;
      border-top: 5px solid #1a365d;
      margin-left: 3px;
    }
    .idm-menu {
      margin-top: 3px;
      min-width: 290px;
      max-width: 380px;
      background: #ffffff;
      border: 1px solid #718096;
      border-radius: 3px;
      box-shadow: 0 6px 18px rgba(0, 0, 0, 0.32);
      overflow: hidden;
      display: none;
    }
    .idm-menu.open {
      display: block;
    }
    .idm-menu-header {
      background: #edf2f7;
      padding: 6px 10px;
      font-weight: 600;
      color: #2d3748;
      border-bottom: 1px solid #cbd5e0;
      display: flex;
      justify-content: space-between;
      align-items: center;
    }
    .idm-menu-item {
      padding: 7px 10px;
      display: flex;
      justify-content: space-between;
      align-items: center;
      gap: 10px;
      color: #1a202c;
      cursor: pointer;
      border-bottom: 1px solid #f0f4f8;
    }
    .idm-menu-item:hover {
      background: #cde8ff;
    }
    .idm-badge {
      background: #e2e8f0;
      color: #2d3748;
      border-radius: 3px;
      padding: 1px 6px;
      font-size: 11px;
      font-weight: 600;
      white-space: nowrap;
    }
    .idm-status-row {
      padding: 8px 10px;
      color: #4a5568;
      font-style: italic;
    }
    .idm-error-row {
      padding: 8px 10px;
      color: #c53030;
      background: #fff5f5;
    }
    .idm-modal-backdrop {
      position: fixed;
      inset: 0;
      background: rgba(15, 23, 42, 0.55);
      display: none;
      align-items: center;
      justify-content: center;
      pointer-events: auto;
      z-index: 2147483647;
      font-family: "Segoe UI", Tahoma, Arial, sans-serif;
      font-size: 12.5px;
    }
    .idm-modal-backdrop.open {
      display: flex;
    }
    .idm-modal-card {
      background: #f8fafc;
      border: 1px solid #64748b;
      border-radius: 5px;
      width: 560px;
      max-width: 94vw;
      max-height: 82vh;
      display: flex;
      flex-direction: column;
      box-shadow: 0 12px 32px rgba(0, 0, 0, 0.38);
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
      border-radius: 3px;
      background: #ffffff;
      color: #0f172a;
      cursor: pointer;
      font-weight: 600;
    }
    .idm-btn.primary {
      background: #0284c7;
      border-color: #0369a1;
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
      border-radius: 3px;
      width: 100%;
      box-sizing: border-box;
    }
  `;

  const widgetEl = document.createElement("div");
  widgetEl.className = "idm-widget";
  widgetEl.innerHTML = `
    <div class="idm-main-btn" id="idmTriggerBtn" title="Download this video with RustIDM">
      <span class="idm-orb"></span>
      <span id="idmBtnLabel">Download this video</span>
      <span class="idm-caret"></span>
    </div>
    <div class="idm-menu" id="idmQualityMenu"></div>
  `;

  const modalEl = document.createElement("div");
  modalEl.className = "idm-modal-backdrop";
  modalEl.innerHTML = `
    <div class="idm-modal-card">
      <div class="idm-modal-titlebar">
        <span id="idmModalTitle">RustIDM</span>
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
  const btnLabel = widgetEl.querySelector("#idmBtnLabel");
  const qualityMenu = widgetEl.querySelector("#idmQualityMenu");
  const modalTitle = modalEl.querySelector("#idmModalTitle");
  const modalBody = modalEl.querySelector("#idmModalBody");
  const modalFooter = modalEl.querySelector("#idmModalFooter");
  const modalCloseBtn = modalEl.querySelector("#idmModalCloseBtn");

  modalCloseBtn.addEventListener("click", () => {
    modalEl.classList.remove("open");
  });

  function formatBytes(bytes) {
    if (!bytes || bytes <= 0) return "";
    if (bytes >= 1024 * 1024 * 1024) {
      return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
    }
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  /**
   * فحص ما إذا كان عنصر الفيديو محمياً بـ Encrypted Media Extensions (Widevine/PlayReady DRM)
   */
  function isVideoDrmProtected(videoEl) {
    if (!videoEl) return false;
    return Boolean(videoEl.mediaKeys || videoEl.webkitKeys);
  }

  function updateWidgetPosition() {
    if (!activeVideoElement || !document.contains(activeVideoElement)) {
      widgetEl.classList.remove("visible");
      return;
    }

    const rect = activeVideoElement.getBoundingClientRect();
    if (rect.width < 180 || rect.height < 110 || rect.bottom < 0 || rect.top > window.innerHeight) {
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
      isDropdownOpen = false;
    }

    const videos = Array.from(document.querySelectorAll("video"));
    let bestVideo = null;
    let bestArea = 0;

    for (const v of videos) {
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
    if (activeVideoElement && activeVideoElement.currentSrc && activeVideoElement.currentSrc.startsWith("http")) {
      return activeVideoElement.currentSrc;
    }
    return location.href;
  }

  function renderQualityMenu(probe) {
    qualityMenu.innerHTML = "";

    const header = document.createElement("div");
    header.className = "idm-menu-header";
    header.textContent = "اختر جودة التحميل (RustIDM v2.0)";
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
      emptyRow.textContent = "لم يتم العثور على جودات متاحة لهذا الفيديو.";
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
    btnLabel.textContent = "Sending to RustIDM...";
    qualityMenu.classList.remove("open");
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
          btnLabel.textContent = "Downloading in RustIDM ✓";
          setTimeout(() => {
            btnLabel.textContent = "Download this video";
          }, 3000);
        } else {
          btnLabel.textContent = "Engine Offline!";
          setTimeout(() => {
            btnLabel.textContent = "Download this video";
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
      return;
    }

    qualityMenu.classList.add("open");
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
            (resp && resp.error) || "تعذر الاتصال بمحرك RustIDM"
          }</div>`;
          return;
        }
        cachedQualitiesKey = cacheKey;
        cachedProbeResult = resp.probe;
        renderQualityMenu(resp.probe);
      }
    );
  });

  /**
   * نافذة سحب كافة روابط الصفحة (Batch Link Grabber Modal)
   */
  function openBatchGrabberModal() {
    modalTitle.textContent = "Download All Links with RustIDM (Batch Page Grabber)";
    modalBody.innerHTML = "";
    modalFooter.innerHTML = "";

    const anchors = Array.from(document.querySelectorAll("a[href], video[src], source[src]"));
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
    filterInput.placeholder = "Filter by extension or keyword (e.g. .mp4, .pdf, .zip)...";

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
    submitBtn.textContent = "Queue Selected Links in RustIDM";
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

  /**
   * نافذة تجديد الرابط المنتهي لمهمة متوقفة في RustIDM
   */
  function openRefreshUrlModal(candidateUrl) {
    modalTitle.textContent = "Refresh Expired Download Link in RustIDM";
    modalBody.innerHTML = `<div class="idm-status-row">جاري جلب المهام المتوقفة من محرك RustIDM...</div>`;
    modalFooter.innerHTML = "";
    modalEl.classList.add("open");

    chrome.runtime.sendMessage({ type: "RUSTIDM_LIST_EXPIRED_TASKS" }, (resp) => {
      modalBody.innerHTML = "";
      if (!resp || !resp.ok || !resp.tasks || resp.tasks.length === 0) {
        modalBody.innerHTML = `<div class="idm-error-row">لا توجد مهام متوقفة أو منتهية الصلاحية حالياً في محرك RustIDM.</div>`;
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

  document.addEventListener("click", () => {
    if (isDropdownOpen) {
      isDropdownOpen = false;
      qualityMenu.classList.remove("open");
    }
  });

  chrome.runtime.onMessage.addListener((msg) => {
    if (!msg) return;
    if (msg.type === "RUSTIDM_STREAMS_UPDATED") {
      discoveredStreams = msg.streams || [];
      cachedQualitiesKey = "";
      detectPrimaryVideo();
    } else if (msg.type === "RUSTIDM_OPEN_BATCH_GRABBER_MODAL") {
      openBatchGrabberModal();
    } else if (msg.type === "RUSTIDM_OPEN_REFRESH_MODAL") {
      openRefreshUrlModal(msg.candidateUrl);
    }
  });

  chrome.runtime.sendMessage({ type: "RUSTIDM_GET_TAB_STREAMS" }, (resp) => {
    if (resp && resp.ok) {
      discoveredStreams = resp.streams || [];
    }
    detectPrimaryVideo();
  });

  window.addEventListener("scroll", updateWidgetPosition, { passive: true });
  window.addEventListener("resize", updateWidgetPosition, { passive: true });
  setInterval(detectPrimaryVideo, 1200);
})();
