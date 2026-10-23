/**
 * RustIDM Video Download Panel - Content Script (v1.2.0)
 * يعرض الزر العائم الكلاسيكي (Download this video) فوق مشغلات الفيديو (بما في ذلك YouTube و HTML5 Video)
 * مع قائمة منسدلة تعرض جميع الجودات الحقيقية المستخرجة من البث أو الصفحة.
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
      min-width: 280px;
      max-width: 360px;
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

  shadow.appendChild(styleEl);
  shadow.appendChild(widgetEl);

  const triggerBtn = widgetEl.querySelector("#idmTriggerBtn");
  const btnLabel = widgetEl.querySelector("#idmBtnLabel");
  const qualityMenu = widgetEl.querySelector("#idmQualityMenu");

  function formatBytes(bytes) {
    if (!bytes || bytes <= 0) return "";
    if (bytes >= 1024 * 1024 * 1024) {
      return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
    }
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  /**
   * تحديث موضع الزر العائم ليظهر أعلى يمين مشغل الفيديو تماماً مثل IDM
   */
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

  /**
   * البحث عن عنصر الفيديو الرئيسي في الصفحة
   */
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

  /**
   * استخراج أفضل رابط بث مكتشف في التبويب (يفضل m3u8 أو mpd ثم الروابط المباشرة)
   */
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

  /**
   * عرض قائمة الجودات في القائمة المنسدلة
   */
  function renderQualityMenu(probe) {
    qualityMenu.innerHTML = "";

    const header = document.createElement("div");
    header.className = "idm-menu-header";
    header.textContent = "اختر جودة التحميل (RustIDM)";
    qualityMenu.appendChild(header);

    if (probe.drm_protected) {
      const drmRow = document.createElement("div");
      drmRow.className = "idm-error-row";
      drmRow.textContent = probe.drm_reason || "هذا الفيديو محمي بنظام DRM ولا يمكن تنزيله.";
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

  /**
   * إرسال الجودة المختارة إلى محرك RustIDM لبدء التحميل
   */
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

  /**
   * عند الضغط على زر Download this video: استكشاف جميع الجودات الفعلية وعرض القائمة
   */
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

  document.addEventListener("click", () => {
    if (isDropdownOpen) {
      isDropdownOpen = false;
      qualityMenu.classList.remove("open");
    }
  });

  chrome.runtime.onMessage.addListener((msg) => {
    if (msg && msg.type === "RUSTIDM_STREAMS_UPDATED") {
      discoveredStreams = msg.streams || [];
      cachedQualitiesKey = "";
      detectPrimaryVideo();
    }
  });

  // طلب التدفقات المسجلة مسبقاً عند تحميل الصفحة
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