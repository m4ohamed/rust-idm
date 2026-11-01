/**
 * StormGet (RustIDM v2.4.1) Integration Module — Background Service Worker (`background.js`)
 * يطبق:
 * 1. فحصاً حقيقياً لاتصال المحرك عبر Native Messaging وواجهة API المحلية (`127.0.0.1:16888`)
 *    مع مهلة زمنية محددة (AbortController) ومعالجة انقطاع الاتصال، وإرجاع الحالات الخمس الدقيقة:
 *    - متصل ("المحرك متصل ويعمل" - أخضر)
 *    - مغلق ("محرك StormGet مغلق" - رمادي)
 *    - جاري الاتصال ("جارٍ الاتصال بالمحرك" - أزرق)
 *    - فشل الاتصال ("تعذر الاتصال بالمحرك" - أحمر)
 *    - غير مستجيب ("المحرك لا يستجيب" - برتقالي)
 * 2. إدارة الإيقاف المؤقت الموقوت للإضافة (10 دقائق، ساعة، يوم، أو حتى إعادة التفعيل يدوياً)
 *    مع حفظ وقت البدء والانتهاء في `chrome.storage.local` ومزامنته مع `chrome.alarms` والمحرك.
 * 3. نظام حظر التحميل من موقع محدد نهائياً (`حظر التحميل من هذا الموقع` + `المواقع المحظورة`)
 *    مع تطبيع النطاقات، دعم النطاقات الفرعية، ومنع المطابقة الجزئية الخاطئة (مثل `evil-example.com`).
 * 4. الفصل الصارم بين الأوضاع الثلاثة المستقلة:
 *    - الوضع 1: الإضافة تعمل بشكل طبيعي.
 *    - الوضع 2: الإضافة متوقفة مؤقتًا على جميع المواقع.
 *    - الوضع 3: الإضافة تعمل، لكن بعض المواقع محظورة بصورة دائمة.
 * 5. التقاط جلسات يوتيوب الحديثة `SABR` وروابط HLS/DASH/MP4 واعتراض التنزيلات مع التسليم الآمن.
 */

try {
  if (typeof importScripts === "function") {
    importScripts("yt-core.js");
  }
} catch (_) {
  // في بيئة Firefox أو بيئة الاختبار حيث يتم تحميل yt-core.js بشكل منفصل
}

const EXTENSION_VERSION = "2.4.1";
const RUSTIDM_API_BASE = "http://127.0.0.1:16888/api/v1";
const NATIVE_HOST_NAME = "com.rustidm.native_host";
const STORAGE_CONTROL_KEY = "stormget_extension_control_v24";
const AUTO_RESUME_ALARM_NAME = "stormget_auto_resume_alarm";
const ENGINE_CHECK_TIMEOUT_MS = 1800;

const IPC_HEADERS = {
  "Content-Type": "application/json",
  "X-RustIDM": "1"
};

/**
 * تعريف الحالات الخمس الرسمية لاتصال المحرك (النص المعروض واللون الدقيق)
 */
const ENGINE_STATUS_DEFINITIONS = {
  connected: {
    code: "connected",
    shortLabel: "متصل",
    displayText: "المحرك متصل ويعمل",
    colorName: "green",
    colorHex: "#16A34A"
  },
  closed: {
    code: "closed",
    shortLabel: "مغلق",
    displayText: "محرك StormGet مغلق",
    colorName: "gray",
    colorHex: "#64748B"
  },
  connecting: {
    code: "connecting",
    shortLabel: "جاري الاتصال",
    displayText: "جارٍ الاتصال بالمحرك",
    colorName: "blue",
    colorHex: "#2563EB"
  },
  failed: {
    code: "failed",
    shortLabel: "فشل الاتصال",
    displayText: "تعذر الاتصال بالمحرك",
    colorName: "red",
    colorHex: "#DC2626"
  },
  unresponsive: {
    code: "unresponsive",
    shortLabel: "غير مستجيب",
    displayText: "المحرك لا يستجيب",
    colorName: "orange",
    colorHex: "#D97706"
  }
};

/**
 * الحالة الافتراضية للتحكم في الإضافة والمواقع المحظورة
 */
function createDefaultControlState() {
  return {
    enabled: true,
    pausedManually: false,
    pauseDurationKey: null,
    pauseStartedAt: null,
    pauseUntil: null,
    blockedSites: [],
    themePreference: "dark",
    rtlEnabled: true
  };
}

let cachedControlState = createDefaultControlState();
let lastEngineStatus = {
  ...ENGINE_STATUS_DEFINITIONS.connecting,
  checkedAt: Date.now(),
  engineVersion: null,
  channel: null
};

/**
 * تطبيع وتوحيد صيغة النطاق قبل التخزين والمقارنة:
 * - إزالة البروتوكول والمسار والمنفذ
 * - تحويل الأحرف إلى صغيرة (lowercase)
 * - إزالة النقطة الختامية و بادئة `www.`
 * - التحقق من صحة النطاق وعدم احتوائه على فراغات أو محارف غير صالحة
 */
function normalizeDomain(rawInput) {
  if (!rawInput || typeof rawInput !== "string") return null;
  let candidate = rawInput.trim().toLowerCase();
  if (!candidate) return null;

  if (candidate.includes("://")) {
    try {
      const parsed = new URL(candidate);
      candidate = (parsed.hostname || "").toLowerCase();
    } catch (_) {
      candidate = candidate.split("://")[1] || "";
    }
  }

  candidate = candidate.split(/[/?#]/)[0].trim();

  // إزالة المنفذ من عناوين IPv4 والنطاقات
  if (candidate.startsWith("[")) {
    const closeIdx = candidate.indexOf("]");
    if (closeIdx !== -1) {
      candidate = candidate.slice(1, closeIdx);
    }
  } else if (candidate.includes(":")) {
    candidate = candidate.split(":")[0];
  }

  candidate = candidate.replace(/^\.+|\.+$/g, "");
  if (candidate.startsWith("*.")) {
    candidate = candidate.slice(2);
  }
  if (candidate.startsWith("www.") && candidate.length > 4) {
    candidate = candidate.slice(4);
  }

  if (
    !candidate ||
    candidate.length > 253 ||
    /[\s@]/.test(candidate) ||
    candidate.includes("..")
  ) {
    return null;
  }

  if (candidate !== "localhost" && !candidate.includes(".")) {
    return null;
  }

  const labels = candidate.split(".");
  for (const label of labels) {
    if (
      !label ||
      label.startsWith("-") ||
      label.endsWith("-") ||
      !/^[a-z0-9_-]+$/.test(label)
    ) {
      return null;
    }
  }

  return candidate;
}

/**
 * استخراج النطاق المطبّع من رابط كامل أو اسم مضيف
 */
function extractNormalizedHostFromUrl(urlOrHost) {
  if (!urlOrHost || typeof urlOrHost !== "string") return null;
  try {
    if (urlOrHost.includes("://")) {
      const u = new URL(urlOrHost);
      if (u.protocol !== "http:" && u.protocol !== "https:") return null;
      return normalizeDomain(u.hostname);
    }
  } catch (_) {}
  return normalizeDomain(urlOrHost);
}

/**
 * مطابقة نطاقات صارمة تدعم النطاقات الفرعية وتمنع المطابقة الجزئية الخاطئة:
 * - إذا كان `blockedDomain` هو `example.com`:
 *   - يطابق `example.com` -> true
 *   - يطابق `sub.example.com` و `a.b.example.com` -> true
 *   - لا يطابق `evil-example.com` أو `myexample.com` -> false
 */
function isHostMatchedByBlockedDomain(candidateUrlOrHost, blockedDomain) {
  const host = extractNormalizedHostFromUrl(candidateUrlOrHost);
  const blocked = normalizeDomain(blockedDomain);
  if (!host || !blocked) return false;
  if (host === blocked) return true;
  if (host.length > blocked.length && host.endsWith(`.${blocked}`)) {
    return true;
  }
  return false;
}

/**
 * فحص ما إذا كان الرابط أو صفحة المصدر ضمن قائمة المواقع المحظورة
 */
function findMatchingBlockedSite(urlOrHost, blockedSitesList) {
  if (!urlOrHost || !Array.isArray(blockedSitesList) || blockedSitesList.length === 0) {
    return null;
  }
  const host = extractNormalizedHostFromUrl(urlOrHost);
  if (!host) return null;
  for (const entry of blockedSitesList) {
    if (entry && entry.domain && isHostMatchedByBlockedDomain(host, entry.domain)) {
      return entry;
    }
  }
  return null;
}

/**
 * حساب الوضع التشغيلي المستقل للإضافة (أحد الأوضاع الثلاثة دون خلط):
 * 1. `normal`: الإضافة تعمل بشكل طبيعي (لا يوجد إيقاف مؤقت ولا مواقع محظورة)
 * 2. `paused_all`: الإضافة متوقفة مؤقتًا على جميع المواقع
 * 3. `normal_with_blocked_sites`: الإضافة تعمل، لكن بعض المواقع محظورة بصورة دائمة
 */
function computeOperatingMode(state, nowMs = Date.now()) {
  if (!state) return "normal";
  const isTimedPaused =
    typeof state.pauseUntil === "number" && state.pauseUntil > nowMs;
  const isPaused = !state.enabled || state.pausedManually || isTimedPaused;
  if (isPaused) {
    return "paused_all";
  }
  if (Array.isArray(state.blockedSites) && state.blockedSites.length > 0) {
    return "normal_with_blocked_sites";
  }
  return "normal";
}

function getOperatingModeDescription(state, nowMs = Date.now()) {
  const mode = computeOperatingMode(state, nowMs);
  if (mode === "paused_all") {
    if (state.pausedManually) {
      return {
        mode,
        badgeText: "متوقفة مؤقتاً (يدوياً)",
        description: "الإضافة متوقفة مؤقتًا على جميع المواقع حتى إعادة التفعيل يدويًا"
      };
    }
    const remainingMs = Math.max(0, (state.pauseUntil || nowMs) - nowMs);
    const remainingMins = Math.max(1, Math.ceil(remainingMs / 60000));
    return {
      mode,
      badgeText: `متوقفة مؤقتاً (${remainingMins} د)`,
      description: "الإضافة متوقفة مؤقتًا على جميع المواقع وتعود للعمل تلقائيًا عند انتهاء المدة"
    };
  }
  if (mode === "normal_with_blocked_sites") {
    const count = (state.blockedSites || []).length;
    return {
      mode,
      badgeText: `تعمل (${count} موقع محظور)`,
      description: `الإضافة تعمل، لكن بعض المواقع محظورة بصورة دائمة (${count})`
    };
  }
  return {
    mode: "normal",
    badgeText: "تعمل بشكل طبيعي",
    description: "الإضافة تعمل بشكل طبيعي على جميع المواقع"
  };
}

/**
 * تحميل وتحديث حالة التحكم في الإضافة من التخزين المحلي مع فحص انتهاء مدة الإيقاف المؤقت تلقائياً
 */
async function loadExtensionControlState() {
  return new Promise((resolve) => {
    try {
      if (typeof chrome === "undefined" || !chrome.storage || !chrome.storage.local) {
        resolve(cachedControlState);
        return;
      }
      chrome.storage.local.get([STORAGE_CONTROL_KEY], async (items) => {
        const stored = items && items[STORAGE_CONTROL_KEY];
        if (stored && typeof stored === "object") {
          cachedControlState = {
            ...createDefaultControlState(),
            ...stored,
            blockedSites: Array.isArray(stored.blockedSites) ? stored.blockedSites : []
          };
        }
        await checkAndExpireTemporaryPause();
        resolve(cachedControlState);
      });
    } catch (_) {
      resolve(cachedControlState);
    }
  });
}

async function saveExtensionControlState(nextState) {
  cachedControlState = {
    ...createDefaultControlState(),
    ...nextState,
    blockedSites: Array.isArray(nextState.blockedSites) ? nextState.blockedSites : []
  };

  await new Promise((resolve) => {
    try {
      if (typeof chrome !== "undefined" && chrome.storage && chrome.storage.local) {
        chrome.storage.local.set({ [STORAGE_CONTROL_KEY]: cachedControlState }, () => {
          resolve();
        });
      } else {
        resolve();
      }
    } catch (_) {
      resolve();
    }
  });

  updateActionBadgeForState(cachedControlState);
  syncControlStateToDesktopEngine(cachedControlState).catch(() => {});
  return cachedControlState;
}

function updateActionBadgeForState(state) {
  try {
    if (typeof chrome === "undefined" || !chrome.action) return;
    const mode = computeOperatingMode(state);
    if (mode === "paused_all") {
      chrome.action.setBadgeText({ text: "⏸" });
      chrome.action.setBadgeBackgroundColor({ color: "#D97706" });
    } else if (lastEngineStatus.code === "connected") {
      chrome.action.setBadgeText({ text: "" });
    } else if (lastEngineStatus.code === "closed" || lastEngineStatus.code === "failed") {
      chrome.action.setBadgeText({ text: "!" });
      chrome.action.setBadgeBackgroundColor({ color: "#64748B" });
    }
  } catch (_) {}
}

/**
 * التحقق من انتهاء مدة الإيقاف المؤقت وإعادة تفعيل الإضافة تلقائياً
 */
async function checkAndExpireTemporaryPause() {
  const now = Date.now();
  if (
    !cachedControlState.pausedManually &&
    typeof cachedControlState.pauseUntil === "number" &&
    cachedControlState.pauseUntil <= now
  ) {
    cachedControlState.enabled = true;
    cachedControlState.pausedManually = false;
    cachedControlState.pauseDurationKey = null;
    cachedControlState.pauseStartedAt = null;
    cachedControlState.pauseUntil = null;
    try {
      if (typeof chrome !== "undefined" && chrome.alarms) {
        chrome.alarms.clear(AUTO_RESUME_ALARM_NAME);
      }
    } catch (_) {}
    await saveExtensionControlState(cachedControlState);
    return true;
  }
  return false;
}

/**
 * تطبيق الإيقاف المؤقت لمدة محددة:
 * - "10m": 10 دقائق
 * - "1h": ساعة
 * - "1d": يوم
 * - "manual": حتى إعادة التفعيل يدوياً
 */
async function setTemporaryPause(durationKey) {
  const now = Date.now();
  let durationMs = null;
  if (durationKey === "10m") {
    durationMs = 10 * 60 * 1000;
  } else if (durationKey === "1h") {
    durationMs = 60 * 60 * 1000;
  } else if (durationKey === "1d") {
    durationMs = 24 * 60 * 60 * 1000;
  }

  const isManual = durationKey === "manual" || durationMs === null;
  const pauseUntil = isManual ? null : now + durationMs;

  try {
    if (typeof chrome !== "undefined" && chrome.alarms) {
      await new Promise((r) => chrome.alarms.clear(AUTO_RESUME_ALARM_NAME, () => r()));
      if (pauseUntil) {
        chrome.alarms.create(AUTO_RESUME_ALARM_NAME, { when: pauseUntil });
      }
    }
  } catch (_) {}

  return saveExtensionControlState({
    ...cachedControlState,
    enabled: false,
    pausedManually: isManual,
    pauseDurationKey: isManual ? "manual" : durationKey,
    pauseStartedAt: now,
    pauseUntil
  });
}

/**
 * إعادة تفعيل الإضافة فوراً في أي وقت قبل انتهاء المدة
 */
async function resumeExtensionNow() {
  try {
    if (typeof chrome !== "undefined" && chrome.alarms) {
      chrome.alarms.clear(AUTO_RESUME_ALARM_NAME);
    }
  } catch (_) {}

  return saveExtensionControlState({
    ...cachedControlState,
    enabled: true,
    pausedManually: false,
    pauseDurationKey: null,
    pauseStartedAt: null,
    pauseUntil: null
  });
}

/**
 * إضافة موقع إلى قائمة المواقع المحظورة الدائمة
 */
async function addBlockedSite(rawDomainOrUrl, customSiteName = null) {
  await loadExtensionControlState();
  const domain = extractNormalizedHostFromUrl(rawDomainOrUrl);
  if (!domain) {
    return { ok: false, error: "النطاق غير صالح للتحليل أو الحظر" };
  }

  const displayName =
    customSiteName && customSiteName.trim()
      ? customSiteName.trim()
      : domain;
  const todayIso = new Date().toISOString().slice(0, 10);

  const list = Array.isArray(cachedControlState.blockedSites)
    ? [...cachedControlState.blockedSites]
    : [];

  const existingIdx = list.findIndex((item) => normalizeDomain(item.domain) === domain);
  if (existingIdx >= 0) {
    list[existingIdx] = {
      ...list[existingIdx],
      siteName: displayName,
      domain
    };
  } else {
    list.unshift({
      siteName: displayName,
      domain,
      addedAt: todayIso
    });
  }

  const saved = await saveExtensionControlState({
    ...cachedControlState,
    blockedSites: list
  });
  return { ok: true, domain, state: saved };
}

/**
 * إزالة الحظر عن موقع محدد
 */
async function removeBlockedSite(rawDomain) {
  await loadExtensionControlState();
  const domain = normalizeDomain(rawDomain);
  if (!domain) {
    return { ok: false, error: "النطاق غير صالح" };
  }
  const list = (cachedControlState.blockedSites || []).filter(
    (item) => normalizeDomain(item.domain) !== domain
  );
  const saved = await saveExtensionControlState({
    ...cachedControlState,
    blockedSites: list
  });
  return { ok: true, domain, state: saved };
}

/**
 * تعديل موقع محظور (تعديل الاسم أو النطاق)
 */
async function updateBlockedSite(oldRawDomain, newRawDomain, newSiteName) {
  await loadExtensionControlState();
  const oldDomain = normalizeDomain(oldRawDomain);
  const newDomain = extractNormalizedHostFromUrl(newRawDomain);
  if (!oldDomain || !newDomain) {
    return { ok: false, error: "النطاق الجديد غير صالح" };
  }

  const list = [...(cachedControlState.blockedSites || [])];
  const idx = list.findIndex((item) => normalizeDomain(item.domain) === oldDomain);
  const todayIso = new Date().toISOString().slice(0, 10);
  const entry = {
    siteName: newSiteName && newSiteName.trim() ? newSiteName.trim() : newDomain,
    domain: newDomain,
    addedAt: idx >= 0 && list[idx].addedAt ? list[idx].addedAt : todayIso
  };

  if (idx >= 0) {
    list[idx] = entry;
  } else {
    list.unshift(entry);
  }

  // إزالة أي تكرار لنفس النطاق المطبّع
  const deduped = [];
  const seen = new Set();
  for (const it of list) {
    const norm = normalizeDomain(it.domain);
    if (norm && !seen.has(norm)) {
      seen.add(norm);
      deduped.push(it);
    }
  }

  const saved = await saveExtensionControlState({
    ...cachedControlState,
    blockedSites: deduped
  });
  return { ok: true, domain: newDomain, state: saved };
}

/**
 * فحص ما إذا كان يجب اعتراض التحميل لرابط معين أو صفحة معينة
 */
async function shouldInterceptDownloadForUrls(targetUrl, pageOrRefererUrl = null) {
  await loadExtensionControlState();
  const mode = computeOperatingMode(cachedControlState);
  if (mode === "paused_all") {
    return {
      allowed: false,
      reason: "extension_paused",
      mode
    };
  }

  const blockedList = cachedControlState.blockedSites || [];
  const matchedPage = pageOrRefererUrl
    ? findMatchingBlockedSite(pageOrRefererUrl, blockedList)
    : null;
  if (matchedPage) {
    return {
      allowed: false,
      reason: "site_blocked",
      blockedSite: matchedPage,
      mode
    };
  }

  const matchedTarget = targetUrl
    ? findMatchingBlockedSite(targetUrl, blockedList)
    : null;
  if (matchedTarget) {
    return {
      allowed: false,
      reason: "site_blocked",
      blockedSite: matchedTarget,
      mode
    };
  }

  return {
    allowed: true,
    reason: "active",
    mode
  };
}

/**
 * مزامنة حالة الإضافة والمواقع المحظورة مع محرك سطح المكتب (`/api/v1/extension-state`)
 */
async function syncControlStateToDesktopEngine(state) {
  try {
    const payload = {
      enabled: Boolean(state.enabled),
      paused_manually: Boolean(state.pausedManually),
      pause_started_at: state.pauseStartedAt || null,
      pause_until: state.pauseUntil || null,
      blocked_sites: (state.blockedSites || []).map((item) => ({
        site_name: item.siteName || item.domain,
        domain: item.domain,
        added_at: item.addedAt || new Date().toISOString().slice(0, 10)
      }))
    };
    await fetchFromStormGetEngine("/extension-state", {
      method: "POST",
      headers: IPC_HEADERS,
      body: JSON.stringify(payload)
    });
  } catch (_) {}
}

if (typeof chrome !== "undefined" && chrome.alarms && chrome.alarms.onAlarm) {
  chrome.alarms.onAlarm.addListener((alarm) => {
    if (alarm && alarm.name === AUTO_RESUME_ALARM_NAME) {
      checkAndExpireTemporaryPause();
    }
  });
}

/**
 * فحص حقيقي لحالة اتصال محرك StormGet عبر HTTP المحلي + Native Messaging مع مهلة زمنية محددة
 * لا يعرض "متصل" إلا بعد نجاح التحقق الفعلي واستلام `bridge_token` صالح.
 */
async function checkEngineStatus(options = { allowNativePing: true }) {
  lastEngineStatus = {
    ...ENGINE_STATUS_DEFINITIONS.connecting,
    checkedAt: Date.now(),
    engineVersion: null,
    channel: null
  };

  let httpTimedOut = false;
  let httpNetworkError = false;

  try {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), ENGINE_CHECK_TIMEOUT_MS);
    try {
      const resp = await fetch(`${RUSTIDM_API_BASE}/health`, {
        method: "GET",
        headers: { "X-RustIDM": "1" },
        signal: controller.signal,
        cache: "no-store"
      });
      clearTimeout(timer);

      if (resp.ok) {
        const data = await resp.json();
        if (data && data.status === "shutting_down") {
          lastEngineStatus = {
            ...ENGINE_STATUS_DEFINITIONS.closed,
            checkedAt: Date.now(),
            engineVersion: data.version || null,
            channel: "http_ipc"
          };
          updateActionBadgeForState(cachedControlState);
          return lastEngineStatus;
        }
        if (
          data &&
          data.status === "online" &&
          typeof data.bridge_token === "string" &&
          data.bridge_token.length >= 20
        ) {
          IPC_HEADERS["Authorization"] = `Bearer ${data.bridge_token}`;
          lastEngineStatus = {
            ...ENGINE_STATUS_DEFINITIONS.connected,
            checkedAt: Date.now(),
            engineVersion: data.version || "2.4.1",
            channel: "http_ipc"
          };
          updateActionBadgeForState(cachedControlState);
          return lastEngineStatus;
        }
      } else {
        lastEngineStatus = {
          ...ENGINE_STATUS_DEFINITIONS.unresponsive,
          checkedAt: Date.now(),
          engineVersion: null,
          channel: "http_ipc"
        };
        updateActionBadgeForState(cachedControlState);
        return lastEngineStatus;
      }
    } catch (fetchErr) {
      clearTimeout(timer);
      if (fetchErr && fetchErr.name === "AbortError") {
        httpTimedOut = true;
      } else {
        httpNetworkError = true;
      }
    }
  } catch (_) {
    httpNetworkError = true;
  }

  if (httpTimedOut) {
    lastEngineStatus = {
      ...ENGINE_STATUS_DEFINITIONS.unresponsive,
      checkedAt: Date.now(),
      engineVersion: null,
      channel: "http_timeout"
    };
    updateActionBadgeForState(cachedControlState);
    return lastEngineStatus;
  }

  // إذا لم يكن HTTP مستجيباً، نفحص Native Messaging Host للتمييز بين "مغلق" و "فشل الاتصال"
  if (options && options.allowNativePing && typeof chrome !== "undefined" && chrome.runtime && chrome.runtime.sendNativeMessage) {
    const nativeResult = await new Promise((resolve) => {
      let settled = false;
      const timeoutId = setTimeout(() => {
        if (!settled) {
          settled = true;
          resolve({ status: "timeout" });
        }
      }, ENGINE_CHECK_TIMEOUT_MS);

      try {
        chrome.runtime.sendNativeMessage(
          NATIVE_HOST_NAME,
          { action: "PING" },
          (resp) => {
            if (settled) return;
            settled = true;
            clearTimeout(timeoutId);
            if (chrome.runtime.lastError) {
              const errMsg = chrome.runtime.lastError.message || "";
              resolve({ status: "error", message: errMsg });
              return;
            }
            if (
              resp &&
              resp.status === "online" &&
              typeof resp.task_id === "string" &&
              resp.task_id.length >= 20
            ) {
              IPC_HEADERS["Authorization"] = `Bearer ${resp.task_id}`;
              resolve({ status: "online", resp });
            } else {
              resolve({ status: "closed", resp });
            }
          }
        );
      } catch (e) {
        if (!settled) {
          settled = true;
          clearTimeout(timeoutId);
          resolve({ status: "error", message: e.message || "Native error" });
        }
      }
    });

    if (nativeResult.status === "online") {
      // نتحقق مرة أخيرة من أن منفذ HTTP يعمل فعلياً أو أن Native Host متصل
      lastEngineStatus = {
        ...ENGINE_STATUS_DEFINITIONS.connected,
        checkedAt: Date.now(),
        engineVersion: "2.4.1",
        channel: "native_messaging"
      };
      updateActionBadgeForState(cachedControlState);
      return lastEngineStatus;
    }

    if (nativeResult.status === "timeout") {
      lastEngineStatus = {
        ...ENGINE_STATUS_DEFINITIONS.unresponsive,
        checkedAt: Date.now(),
        engineVersion: null,
        channel: "native_timeout"
      };
      updateActionBadgeForState(cachedControlState);
      return lastEngineStatus;
    }

    if (
      nativeResult.status === "error" &&
      nativeResult.message &&
      /not found|forbidden|access denied|specified native messaging host/i.test(
        nativeResult.message
      )
    ) {
      // إذا كان مضيف Native Messaging غير مسجل والمنفذ المحلي مغلق
      lastEngineStatus = {
        ...(httpNetworkError
          ? ENGINE_STATUS_DEFINITIONS.closed
          : ENGINE_STATUS_DEFINITIONS.failed),
        checkedAt: Date.now(),
        engineVersion: null,
        channel: "none"
      };
      updateActionBadgeForState(cachedControlState);
      return lastEngineStatus;
    }
  }

  lastEngineStatus = {
    ...ENGINE_STATUS_DEFINITIONS.closed,
    checkedAt: Date.now(),
    engineVersion: null,
    channel: "none"
  };
  updateActionBadgeForState(cachedControlState);
  return lastEngineStatus;
}

async function ensureBridgeToken(forceRefresh = false) {
  if (!forceRefresh && IPC_HEADERS["Authorization"]) {
    return true;
  }
  const status = await checkEngineStatus({ allowNativePing: true });
  return status.code === "connected";
}

async function fetchFromStormGetEngine(subpath, options = {}) {
  await ensureBridgeToken(false);
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), ENGINE_CHECK_TIMEOUT_MS + 1200);
  try {
    let resp = await fetch(`${RUSTIDM_API_BASE}${subpath}`, {
      ...options,
      signal: controller.signal,
      headers: { ...IPC_HEADERS, ...(options.headers || {}) }
    });
    if (resp.status === 403) {
      const refreshed = await ensureBridgeToken(true);
      if (refreshed) {
        resp = await fetch(`${RUSTIDM_API_BASE}${subpath}`, {
          ...options,
          signal: controller.signal,
          headers: { ...IPC_HEADERS, ...(options.headers || {}) }
        });
      }
    }
    clearTimeout(timeoutId);
    return resp;
  } catch (err) {
    clearTimeout(timeoutId);
    throw err;
  }
}

// تحميل الحالة المبدئية عند بدء Service Worker
loadExtensionControlState().then(() => {
  checkEngineStatus({ allowNativePing: false }).catch(() => {});
});

// تخزين روابط البث العام المكتشفة لكل تبويب: tabId -> Map<url, StreamEntry>
const tabStreams = new Map();
const tabYoutubeStreams = new Map();
const youtubePlayers = new Map();
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
  description,
  bypassSitePolicy = false
}) {
  if (!bypassSitePolicy) {
    const policy = await shouldInterceptDownloadForUrls(url, referer);
    if (!policy.allowed) {
      return {
        ok: false,
        skippedByPolicy: true,
        reason: policy.reason,
        error:
          policy.reason === "extension_paused"
            ? "الإضافة متوقفة مؤقتاً"
            : "الموقع محظور من اعتراض التنزيلات"
      };
    }
  }

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
    const resp = await fetchFromStormGetEngine("/intercept", {
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

/**
 * فتح نافذة StormGet على سطح المكتب من داخل الإضافة
 */
async function openStormGetDesktopWindow() {
  try {
    const resp = await fetchFromStormGetEngine("/desktop/show", {
      method: "POST",
      headers: IPC_HEADERS
    });
    if (resp.ok) {
      await checkEngineStatus({ allowNativePing: false });
      return { ok: true, channel: "http_ipc" };
    }
  } catch (_) {}

  try {
    const nativeResp = await sendViaNativeMessagingHost({
      action: "OPEN_DESKTOP_WINDOW"
    });
    await checkEngineStatus({ allowNativePing: true });
    return { ok: true, channel: "native_messaging", data: nativeResp };
  } catch (err) {
    return {
      ok: false,
      error: "تعذر فتح نافذة StormGet. تأكد من تثبيت البرنامج على سطح المكتب."
    };
  }
}

function setupContextMenus() {
  if (typeof chrome === "undefined" || !chrome.contextMenus) return;
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: "rustidm_download_link",
      title: "التحميل بواسطة StormGet",
      contexts: ["link", "video", "audio"]
    });
    chrome.contextMenus.create({
      id: "rustidm_download_all_links",
      title: "تحميل جميع روابط الصفحة بواسطة StormGet (دفعي)...",
      contexts: ["page", "frame"]
    });
    chrome.contextMenus.create({
      id: "rustidm_refresh_expired_link",
      title: "استخدام هذا الرابط لتجديد تحميل منتهي الصلاحية في StormGet...",
      contexts: ["link"]
    });
    chrome.contextMenus.create({
      id: "rustidm_block_current_site",
      title: "حظر التحميل من هذا الموقع في StormGet",
      contexts: ["page", "frame"]
    });
  });
}

if (typeof chrome !== "undefined" && chrome.runtime && chrome.runtime.onInstalled) {
  chrome.runtime.onInstalled.addListener(() => {
    setupContextMenus();
    loadExtensionControlState();
  });
}

if (typeof chrome !== "undefined" && chrome.contextMenus && chrome.contextMenus.onClicked) {
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
          description: "Context Menu Download",
          bypassSitePolicy: true
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
    } else if (info.menuItemId === "rustidm_block_current_site") {
      if (pageUrl) {
        await addBlockedSite(pageUrl, tab && tab.title ? tab.title : null);
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

  if (typeof chrome !== "undefined" && chrome.tabs) {
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
}

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

if (
  typeof chrome !== "undefined" &&
  chrome.webRequest &&
  chrome.webRequest.onHeadersReceived
) {
  chrome.webRequest.onHeadersReceived.addListener(
    (details) => {
      if (details.tabId < 0) return;
      const url = details.url;
      const urlLower = url.toLowerCase();

      if (url.startsWith(RUSTIDM_API_BASE)) {
        return;
      }

      // احترام حالة الإيقاف المؤقت والمواقع المحظورة
      if (computeOperatingMode(cachedControlState) === "paused_all") {
        return;
      }
      if (
        findMatchingBlockedSite(url, cachedControlState.blockedSites) ||
        (details.initiator &&
          findMatchingBlockedSite(details.initiator, cachedControlState.blockedSites))
      ) {
        return;
      }

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
}

if (typeof chrome !== "undefined" && chrome.tabs && chrome.tabs.onRemoved) {
  chrome.tabs.onRemoved.addListener((tabId) => {
    tabStreams.delete(tabId);
    tabYoutubeStreams.delete(tabId);
    youtubePlayers.delete(tabId);
    clearSabrCaptureForTab(tabId);
  });
}

if (typeof chrome !== "undefined" && chrome.tabs && chrome.tabs.onUpdated) {
  chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
    if (changeInfo.status === "loading") {
      clearSabrCaptureForTab(tabId);
    }
  });
}

const interceptedDownloadIds = new Set();

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

  // التحقق الصارم من حالة الإيقاف المؤقت وقائمة المواقع المحظورة قبل لمس تنزيل المتصفح
  const policy = await shouldInterceptDownloadForUrls(url, downloadItem.referrer || null);
  if (!policy.allowed) {
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
    description: "Browser Intercepted Download",
    bypassSitePolicy: true
  });

  if (outcome.ok) {
    chrome.downloads.cancel(id, () => {
      void chrome.runtime.lastError;
      chrome.downloads.erase({ id }, () => {
        void chrome.runtime.lastError;
      });
    });
  } else {
    interceptedDownloadIds.delete(id);
    if (pausedInBrowser) {
      chrome.downloads.resume(id, () => void chrome.runtime.lastError);
    }
  }
}

if (typeof chrome !== "undefined" && chrome.downloads) {
  if (chrome.downloads.onCreated) {
    chrome.downloads.onCreated.addListener((downloadItem) => {
      handleDownloadInterception(downloadItem);
    });
  }
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
}

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

if (typeof chrome !== "undefined" && chrome.runtime && chrome.runtime.onMessage) {
  chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    const tabId = sender && sender.tab ? sender.tab.id : -1;

    // ================== أوامر واجهة Popup وإعدادات الإضافة الجديدة ==================
    if (message.type === "STORMGET_GET_POPUP_STATE") {
      (async () => {
        await loadExtensionControlState();
        const engineStatus = message.forceEngineCheck
          ? await checkEngineStatus({ allowNativePing: true })
          : lastEngineStatus.code === "connecting"
          ? await checkEngineStatus({ allowNativePing: false })
          : lastEngineStatus;
        const modeInfo = getOperatingModeDescription(cachedControlState);
        const currentUrl = message.currentTabUrl || (sender && sender.tab && sender.tab.url) || "";
        const currentDomain = extractNormalizedHostFromUrl(currentUrl);
        const matchedBlock = currentDomain
          ? findMatchingBlockedSite(currentDomain, cachedControlState.blockedSites)
          : null;

        sendResponse({
          ok: true,
          version: EXTENSION_VERSION,
          engineStatus,
          controlState: cachedControlState,
          modeInfo,
          currentSite: {
            url: currentUrl,
            domain: currentDomain,
            isBlocked: Boolean(matchedBlock),
            matchedRule: matchedBlock || null
          }
        });
      })();
      return true;
    }

    if (message.type === "STORMGET_CHECK_ENGINE") {
      (async () => {
        const engineStatus = await checkEngineStatus({ allowNativePing: true });
        sendResponse({ ok: true, engineStatus });
      })();
      return true;
    }

    if (message.type === "STORMGET_SET_PAUSE") {
      (async () => {
        const state = await setTemporaryPause(message.durationKey || "manual");
        const modeInfo = getOperatingModeDescription(state);
        sendResponse({ ok: true, controlState: state, modeInfo });
      })();
      return true;
    }

    if (message.type === "STORMGET_RESUME_NOW") {
      (async () => {
        const state = await resumeExtensionNow();
        const modeInfo = getOperatingModeDescription(state);
        sendResponse({ ok: true, controlState: state, modeInfo });
      })();
      return true;
    }

    if (message.type === "STORMGET_BLOCK_SITE") {
      (async () => {
        const res = await addBlockedSite(message.domainOrUrl, message.siteName || null);
        const modeInfo = getOperatingModeDescription(cachedControlState);
        sendResponse({ ...res, modeInfo });
      })();
      return true;
    }

    if (message.type === "STORMGET_UNBLOCK_SITE") {
      (async () => {
        const res = await removeBlockedSite(message.domain);
        const modeInfo = getOperatingModeDescription(cachedControlState);
        sendResponse({ ...res, modeInfo });
      })();
      return true;
    }

    if (message.type === "STORMGET_UPDATE_BLOCKED_SITE") {
      (async () => {
        const res = await updateBlockedSite(
          message.oldDomain,
          message.newDomain,
          message.siteName
        );
        const modeInfo = getOperatingModeDescription(cachedControlState);
        sendResponse({ ...res, modeInfo });
      })();
      return true;
    }

    if (message.type === "STORMGET_SET_UI_PREFS") {
      (async () => {
        await loadExtensionControlState();
        const saved = await saveExtensionControlState({
          ...cachedControlState,
          themePreference: message.themePreference || cachedControlState.themePreference,
          rtlEnabled:
            typeof message.rtlEnabled === "boolean"
              ? message.rtlEnabled
              : cachedControlState.rtlEnabled
        });
        sendResponse({ ok: true, controlState: saved });
      })();
      return true;
    }

    if (message.type === "STORMGET_OPEN_DESKTOP_APP") {
      (async () => {
        const res = await openStormGetDesktopWindow();
        sendResponse(res);
      })();
      return true;
    }

    if (message.type === "STORMGET_CHECK_SITE_POLICY") {
      (async () => {
        const policy = await shouldInterceptDownloadForUrls(
          message.url || message.pageUrl,
          message.pageUrl || null
        );
        sendResponse({ ok: true, ...policy });
      })();
      return true;
    }

    // ================== أوامر التقاط الفيديو والروابط ==================
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
      (async () => {
        const policy = await shouldInterceptDownloadForUrls(
          message.url,
          sender && sender.tab ? sender.tab.url : null
        );
        if (policy.allowed && tabId !== null && tabId >= 0 && message.url) {
          registerTabStream(tabId, {
            url: message.url,
            protocol: message.protocol || "direct",
            contentType: message.hookSource || "main-hook",
            detectedAt: Date.now()
          });
        }
        sendResponse({ ok: true, allowed: policy.allowed });
      })();
      return true;
    }

    if (message.type === "RUSTIDM_GET_TAB_STREAMS") {
      (async () => {
        const pageUrl = (sender && sender.tab && sender.tab.url) || message.pageUrl || "";
        const policy = await shouldInterceptDownloadForUrls(pageUrl, pageUrl);
        if (!policy.allowed) {
          sendResponse({ ok: true, allowed: false, streams: [] });
          return;
        }
        const streams = tabStreams.has(tabId)
          ? Array.from(tabStreams.get(tabId).values())
          : [];
        sendResponse({ ok: true, allowed: true, streams });
      })();
      return true;
    }

    if (message.type === "RUSTIDM_PROBE_QUALITIES") {
      (async () => {
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

          const resp = await fetchFromStormGetEngine("/video/probe", {
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

          const resp = await fetchFromStormGetEngine("/video/download", {
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
          const resp = await fetchFromStormGetEngine("/downloads", {
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
          const resp = await fetchFromStormGetEngine("/refresh-url", {
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
          const resp = await fetchFromStormGetEngine("/batch", {
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
}

// تصدير الدوال في بيئة Node.js للاختبار الآلي المباشر
if (typeof module !== "undefined" && module.exports) {
  module.exports = {
    ENGINE_STATUS_DEFINITIONS,
    normalizeDomain,
    extractNormalizedHostFromUrl,
    isHostMatchedByBlockedDomain,
    findMatchingBlockedSite,
    computeOperatingMode,
    getOperatingModeDescription,
    setTemporaryPause,
    resumeExtensionNow,
    addBlockedSite,
    removeBlockedSite,
    updateBlockedSite,
    shouldInterceptDownloadForUrls
  };
}
