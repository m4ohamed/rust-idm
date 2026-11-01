/**
 * StormGet v2.4.0 — Extension Popup Controller (`popup.js`)
 * يربط العناصر التسعة في الواجهة بحالة المحرك الحقيقية، وأوضاع التشغيل الثلاثة،
 * والعداد التنازلي للإيقاف المؤقت، وإدارة قائمة المواقع المحظورة الدائمة.
 */

(function () {
  const extVersionBadge = document.getElementById("extVersionBadge");
  const headerStatusDot = document.getElementById("headerStatusDot");
  const themeToggleBtn = document.getElementById("themeToggleBtn");
  const refreshEngineBtn = document.getElementById("refreshEngineBtn");

  const engineStatusBadge = document.getElementById("engineStatusBadge");
  const engineStatusRing = document.getElementById("engineStatusRing");
  const engineStatusText = document.getElementById("engineStatusText");
  const engineChannelSub = document.getElementById("engineChannelSub");

  const extensionModeBadge = document.getElementById("extensionModeBadge");
  const extensionModeDesc = document.getElementById("extensionModeDesc");
  const toggleExtensionMasterBtn = document.getElementById("toggleExtensionMasterBtn");
  const toggleExtensionMasterLabel = document.getElementById("toggleExtensionMasterLabel");

  const pauseCountdownPill = document.getElementById("pauseCountdownPill");
  const pauseChips = Array.from(document.querySelectorAll(".pause-chip"));
  const resumeExtensionNowBtn = document.getElementById("resumeExtensionNowBtn");

  const currentSitePolicyBadge = document.getElementById("currentSitePolicyBadge");
  const currentSiteTitle = document.getElementById("currentSiteTitle");
  const currentSiteDomain = document.getElementById("currentSiteDomain");

  const blockCurrentSiteBtn = document.getElementById("blockCurrentSiteBtn");
  const blockCurrentSiteLabel = document.getElementById("blockCurrentSiteLabel");

  const blockedSitesCountPill = document.getElementById("blockedSitesCountPill");
  const quickAddBlockForm = document.getElementById("quickAddBlockForm");
  const quickBlockDomainInput = document.getElementById("quickBlockDomainInput");
  const blockedSitesContainer = document.getElementById("blockedSitesContainer");
  const blockedSitesEmpty = document.getElementById("blockedSitesEmpty");

  const editBlockedSitePanel = document.getElementById("editBlockedSitePanel");
  const editOldDomainInput = document.getElementById("editOldDomainInput");
  const editSiteNameInput = document.getElementById("editSiteNameInput");
  const editNewDomainInput = document.getElementById("editNewDomainInput");
  const saveEditBlockedSiteBtn = document.getElementById("saveEditBlockedSiteBtn");
  const cancelEditBlockedSiteBtn = document.getElementById("cancelEditBlockedSiteBtn");

  const openOptionsPageBtn = document.getElementById("openOptionsPageBtn");
  const openDesktopAppBtn = document.getElementById("openDesktopAppBtn");
  const popupToastFeedback = document.getElementById("popupToastFeedback");

  let activeTabInfo = { url: "", title: "", domain: null };
  let currentPopupState = null;
  let countdownInterval = null;

  function showToast(message) {
    if (!popupToastFeedback) return;
    popupToastFeedback.textContent = message;
    popupToastFeedback.classList.remove("hidden");
    setTimeout(() => {
      popupToastFeedback.classList.add("hidden");
    }, 3200);
  }

  function applyTheme(theme) {
    const mode = theme === "light" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", mode);
  }

  function setStatusClass(el, statusCode) {
    if (!el) return;
    el.classList.remove(
      "status-connected",
      "status-closed",
      "status-connecting",
      "status-failed",
      "status-unresponsive"
    );
    el.classList.add(`status-${statusCode || "connecting"}`);
  }

  function renderEngineStatus(engineStatus) {
    if (!engineStatus) return;
    const code = engineStatus.code || "connecting";
    setStatusClass(headerStatusDot, code);
    setStatusClass(engineStatusBadge, code);
    setStatusClass(engineStatusRing, code);

    engineStatusBadge.textContent = engineStatus.shortLabel || "جاري الاتصال";
    engineStatusText.textContent = engineStatus.displayText || "جارٍ الاتصال بالمحرك";

    if (code === "connected") {
      const ver = engineStatus.engineVersion ? `v${engineStatus.engineVersion}` : "v2.4.0";
      const ch =
        engineStatus.channel === "native_messaging"
          ? "Native Messaging Host"
          : "HTTP IPC 127.0.0.1:16888";
      engineChannelSub.textContent = `${ver} • ${ch}`;
    } else if (code === "unresponsive") {
      engineChannelSub.textContent = "انتهت مهلة الاستجابة (Timeout > 1.8s)";
    } else if (code === "failed") {
      engineChannelSub.textContent = "تعذر التحقق من المنفذ المحلي ومضيف المتصفح";
    } else {
      engineChannelSub.textContent = "127.0.0.1:16888 • Native Messaging";
    }
  }

  function formatRemainingTime(pauseUntil) {
    if (!pauseUntil) return "يدوي";
    const diffSec = Math.max(0, Math.floor((pauseUntil - Date.now()) / 1000));
    const hrs = Math.floor(diffSec / 3600);
    const mins = Math.floor((diffSec % 3600) / 60);
    const secs = diffSec % 60;
    if (hrs > 0) {
      return `${hrs}h ${mins}m ${secs}s`;
    }
    return `${mins}m ${secs}s`;
  }

  function renderControlAndMode(controlState, modeInfo) {
    if (!controlState || !modeInfo) return;
    applyTheme(controlState.themePreference || "dark");

    const isPaused = modeInfo.mode === "paused_all";

    if (modeInfo.mode === "paused_all") {
      extensionModeBadge.textContent = "الإضافة متوقفة مؤقتًا على جميع المواقع";
      extensionModeDesc.textContent = modeInfo.description;
      toggleExtensionMasterBtn.classList.remove("active");
      toggleExtensionMasterBtn.classList.add("paused");
      toggleExtensionMasterLabel.textContent = "متوقفة";
      resumeExtensionNowBtn.classList.remove("hidden");
    } else if (modeInfo.mode === "normal_with_blocked_sites") {
      extensionModeBadge.textContent = "الإضافة تعمل، لكن بعض المواقع محظورة بصورة دائمة";
      extensionModeDesc.textContent = modeInfo.description;
      toggleExtensionMasterBtn.classList.remove("paused");
      toggleExtensionMasterBtn.classList.add("active");
      toggleExtensionMasterLabel.textContent = "نشطة";
      resumeExtensionNowBtn.classList.add("hidden");
    } else {
      extensionModeBadge.textContent = "الإضافة تعمل بشكل طبيعي";
      extensionModeDesc.textContent =
        "تلتقط الإضافة روابط التنزيل والفيديو وترسلها إلى محرك StormGet تلقائيًا.";
      toggleExtensionMasterBtn.classList.remove("paused");
      toggleExtensionMasterBtn.classList.add("active");
      toggleExtensionMasterLabel.textContent = "نشطة";
      resumeExtensionNowBtn.classList.add("hidden");
    }

    pauseChips.forEach((chip) => {
      const dur = chip.getAttribute("data-duration");
      if (isPaused && controlState.pauseDurationKey === dur) {
        chip.classList.add("selected");
      } else {
        chip.classList.remove("selected");
      }
    });

    if (countdownInterval) {
      clearInterval(countdownInterval);
      countdownInterval = null;
    }

    if (isPaused) {
      pauseCountdownPill.classList.remove("hidden");
      if (controlState.pausedManually || !controlState.pauseUntil) {
        pauseCountdownPill.textContent = "متوقفة يدويًا";
      } else {
        const updateTimer = () => {
          if (Date.now() >= controlState.pauseUntil) {
            refreshPopupState(false);
            return;
          }
          pauseCountdownPill.textContent = formatRemainingTime(controlState.pauseUntil);
        };
        updateTimer();
        countdownInterval = setInterval(updateTimer, 1000);
      }
    } else {
      pauseCountdownPill.classList.add("hidden");
    }

    renderBlockedSitesList(controlState.blockedSites || []);
  }

  function renderCurrentSiteSection(currentSite, modeInfo) {
    const domain = currentSite && currentSite.domain ? currentSite.domain : null;
    if (!domain) {
      currentSiteTitle.textContent = "صفحة داخلية للمتصفح";
      currentSiteDomain.textContent = "لا ينطبق عليها اعتراض الروابط";
      currentSitePolicyBadge.textContent = "محايد";
      currentSitePolicyBadge.className = "site-policy-badge allowed";
      blockCurrentSiteBtn.disabled = true;
      blockCurrentSiteBtn.classList.remove("unblock-mode");
      blockCurrentSiteLabel.textContent = "حظر التحميل من هذا الموقع";
      return;
    }

    blockCurrentSiteBtn.disabled = false;
    currentSiteTitle.textContent = activeTabInfo.title || domain;
    currentSiteDomain.textContent = domain;

    if (currentSite.isBlocked) {
      const ruleDomain =
        (currentSite.matchedRule && currentSite.matchedRule.domain) || domain;
      currentSitePolicyBadge.textContent = "محظور دائمًا";
      currentSitePolicyBadge.className = "site-policy-badge blocked";
      blockCurrentSiteBtn.classList.add("unblock-mode");
      blockCurrentSiteBtn.setAttribute("data-action", "unblock");
      blockCurrentSiteBtn.setAttribute("data-domain", ruleDomain);
      blockCurrentSiteLabel.textContent = `إزالة الحظر عن (${ruleDomain})`;
    } else {
      if (modeInfo && modeInfo.mode === "paused_all") {
        currentSitePolicyBadge.textContent = "الإضافة متوقفة مؤقتًا";
        currentSitePolicyBadge.className = "site-policy-badge blocked";
      } else {
        currentSitePolicyBadge.textContent = "التقاط نشط";
        currentSitePolicyBadge.className = "site-policy-badge allowed";
      }
      blockCurrentSiteBtn.classList.remove("unblock-mode");
      blockCurrentSiteBtn.setAttribute("data-action", "block");
      blockCurrentSiteBtn.setAttribute("data-domain", domain);
      blockCurrentSiteLabel.textContent = "حظر التحميل من هذا الموقع";
    }
  }

  function renderBlockedSitesList(blockedSites) {
    blockedSitesCountPill.textContent = String(blockedSites.length);
    blockedSitesContainer.innerHTML = "";

    if (!blockedSites || blockedSites.length === 0) {
      blockedSitesContainer.appendChild(blockedSitesEmpty);
      blockedSitesEmpty.classList.remove("hidden");
      return;
    }

    for (const item of blockedSites) {
      const row = document.createElement("div");
      row.className = "blocked-site-row";

      const details = document.createElement("div");
      details.className = "blocked-site-details";

      const nameEl = document.createElement("span");
      nameEl.className = "blocked-site-name";
      nameEl.textContent = item.siteName || item.domain;

      const subEl = document.createElement("div");
      subEl.className = "blocked-site-sub";

      const domainSpan = document.createElement("span");
      domainSpan.setAttribute("dir", "ltr");
      domainSpan.textContent = item.domain;

      const dateSpan = document.createElement("span");
      dateSpan.setAttribute("dir", "ltr");
      dateSpan.textContent = `• ${item.addedAt || ""}`;

      subEl.appendChild(domainSpan);
      subEl.appendChild(dateSpan);

      details.appendChild(nameEl);
      details.appendChild(subEl);

      const actions = document.createElement("div");
      actions.className = "blocked-site-actions";

      const editBtn = document.createElement("button");
      editBtn.type = "button";
      editBtn.className = "mini-btn";
      editBtn.textContent = "تعديل";
      editBtn.addEventListener("click", () => {
        editOldDomainInput.value = item.domain;
        editSiteNameInput.value = item.siteName || item.domain;
        editNewDomainInput.value = item.domain;
        editBlockedSitePanel.classList.remove("hidden");
      });

      const removeBtn = document.createElement("button");
      removeBtn.type = "button";
      removeBtn.className = "mini-btn danger";
      removeBtn.textContent = "إزالة الحظر";
      removeBtn.addEventListener("click", () => {
        chrome.runtime.sendMessage(
          {
            type: "STORMGET_UNBLOCK_SITE",
            domain: item.domain
          },
          () => {
            showToast(`تمت إزالة الحظر عن ${item.domain}`);
            refreshPopupState(false);
          }
        );
      });

      actions.appendChild(editBtn);
      actions.appendChild(removeBtn);

      row.appendChild(details);
      row.appendChild(actions);
      blockedSitesContainer.appendChild(row);
    }
  }

  async function queryActiveBrowserTab() {
    return new Promise((resolve) => {
      try {
        if (typeof chrome !== "undefined" && chrome.tabs && chrome.tabs.query) {
          chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
            const tab = tabs && tabs[0];
            resolve({
              url: (tab && tab.url) || "",
              title: (tab && tab.title) || ""
            });
          });
        } else {
          resolve({ url: "", title: "" });
        }
      } catch (_) {
        resolve({ url: "", title: "" });
      }
    });
  }

  async function refreshPopupState(forceEngineCheck = false) {
    if (forceEngineCheck) {
      refreshEngineBtn.classList.add("spinning");
      renderEngineStatus({
        code: "connecting",
        shortLabel: "جاري الاتصال",
        displayText: "جارٍ الاتصال بالمحرك"
      });
    }

    const tab = await queryActiveBrowserTab();
    activeTabInfo.url = tab.url;
    activeTabInfo.title = tab.title;

    chrome.runtime.sendMessage(
      {
        type: "STORMGET_GET_POPUP_STATE",
        forceEngineCheck,
        currentTabUrl: tab.url
      },
      (resp) => {
        refreshEngineBtn.classList.remove("spinning");
        if (!resp || !resp.ok) return;
        currentPopupState = resp;
        if (resp.version) {
          extVersionBadge.textContent = `v${resp.version}`;
        }
        renderEngineStatus(resp.engineStatus);
        renderControlAndMode(resp.controlState, resp.modeInfo);
        renderCurrentSiteSection(resp.currentSite, resp.modeInfo);
      }
    );
  }

  refreshEngineBtn.addEventListener("click", () => {
    refreshPopupState(true);
  });

  themeToggleBtn.addEventListener("click", () => {
    const currentTheme = document.documentElement.getAttribute("data-theme") || "dark";
    const nextTheme = currentTheme === "dark" ? "light" : "dark";
    applyTheme(nextTheme);
    chrome.runtime.sendMessage({
      type: "STORMGET_SET_UI_PREFS",
      themePreference: nextTheme
    });
  });

  toggleExtensionMasterBtn.addEventListener("click", () => {
    if (!currentPopupState || !currentPopupState.modeInfo) return;
    if (currentPopupState.modeInfo.mode === "paused_all") {
      chrome.runtime.sendMessage({ type: "STORMGET_RESUME_NOW" }, () => {
        showToast("تمت إعادة تفعيل الإضافة بنجاح");
        refreshPopupState(false);
      });
    } else {
      chrome.runtime.sendMessage(
        { type: "STORMGET_SET_PAUSE", durationKey: "manual" },
        () => {
          showToast("تم إيقاف الإضافة مؤقتًا حتى إعادة التفعيل يدويًا");
          refreshPopupState(false);
        }
      );
    }
  });

  pauseChips.forEach((chip) => {
    chip.addEventListener("click", () => {
      const durationKey = chip.getAttribute("data-duration") || "10m";
      chrome.runtime.sendMessage(
        {
          type: "STORMGET_SET_PAUSE",
          durationKey
        },
        () => {
          showToast(`تم تفعيل الإيقاف المؤقت (${chip.textContent})`);
          refreshPopupState(false);
        }
      );
    });
  });

  resumeExtensionNowBtn.addEventListener("click", () => {
    chrome.runtime.sendMessage({ type: "STORMGET_RESUME_NOW" }, () => {
      showToast("عادت الإضافة للعمل الطبيعي");
      refreshPopupState(false);
    });
  });

  blockCurrentSiteBtn.addEventListener("click", () => {
    const action = blockCurrentSiteBtn.getAttribute("data-action");
    const domain = blockCurrentSiteBtn.getAttribute("data-domain");
    if (!domain) return;

    if (action === "unblock") {
      chrome.runtime.sendMessage(
        {
          type: "STORMGET_UNBLOCK_SITE",
          domain
        },
        () => {
          showToast(`تمت إزالة الحظر عن ${domain}`);
          refreshPopupState(false);
        }
      );
    } else {
      chrome.runtime.sendMessage(
        {
          type: "STORMGET_BLOCK_SITE",
          domainOrUrl: activeTabInfo.url || domain,
          siteName: activeTabInfo.title || domain
        },
        (resp) => {
          if (resp && resp.ok) {
            showToast(`تم حظر التحميل من ${resp.domain} ونطاقاته الفرعية`);
            refreshPopupState(false);
          } else {
            showToast((resp && resp.error) || "تعذر حظر النطاق");
          }
        }
      );
    }
  });

  quickAddBlockForm.addEventListener("submit", (e) => {
    e.preventDefault();
    const raw = (quickBlockDomainInput.value || "").trim();
    if (!raw) return;
    chrome.runtime.sendMessage(
      {
        type: "STORMGET_BLOCK_SITE",
        domainOrUrl: raw,
        siteName: raw
      },
      (resp) => {
        if (resp && resp.ok) {
          quickBlockDomainInput.value = "";
          showToast(`تمت إضافة ${resp.domain} إلى المواقع المحظورة`);
          refreshPopupState(false);
        } else {
          showToast((resp && resp.error) || "يرجى إدخال نطاق صحيح مثل example.com");
        }
      }
    );
  });

  saveEditBlockedSiteBtn.addEventListener("click", () => {
    const oldDomain = editOldDomainInput.value;
    const newDomain = editNewDomainInput.value;
    const siteName = editSiteNameInput.value;
    chrome.runtime.sendMessage(
      {
        type: "STORMGET_UPDATE_BLOCKED_SITE",
        oldDomain,
        newDomain,
        siteName
      },
      (resp) => {
        if (resp && resp.ok) {
          editBlockedSitePanel.classList.add("hidden");
          showToast(`تم تحديث بيانات الحظر للنطاق ${resp.domain}`);
          refreshPopupState(false);
        } else {
          showToast((resp && resp.error) || "النطاق المدخل غير صالح");
        }
      }
    );
  });

  cancelEditBlockedSiteBtn.addEventListener("click", () => {
    editBlockedSitePanel.classList.add("hidden");
  });

  openOptionsPageBtn.addEventListener("click", () => {
    if (chrome.runtime && chrome.runtime.openOptionsPage) {
      chrome.runtime.openOptionsPage();
    } else {
      window.open(chrome.runtime.getURL("options.html"), "_blank");
    }
  });

  openDesktopAppBtn.addEventListener("click", () => {
    showToast("جاري فتح نافذة StormGet على سطح المكتب...");
    chrome.runtime.sendMessage({ type: "STORMGET_OPEN_DESKTOP_APP" }, (resp) => {
      if (resp && resp.ok) {
        showToast("تم إظهار نافذة StormGet على سطح المكتب ✓");
        refreshPopupState(true);
      } else {
        showToast((resp && resp.error) || "تعذر تشغيل أو إظهار StormGet");
      }
    });
  });

  // الفحص الفوري عند فتح الـ Popup
  refreshPopupState(true);
})();
