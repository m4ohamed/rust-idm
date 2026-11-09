/**
 * StormGet v2.4.7 — Extension Options Controller (`options.js`)
 */

(function () {
  const optThemeToggleBtn = document.getElementById("optThemeToggleBtn");
  const optCheckEngineBtn = document.getElementById("optCheckEngineBtn");
  const optEngineBadge = document.getElementById("optEngineBadge");
  const optEngineRing = document.getElementById("optEngineRing");
  const optEngineText = document.getElementById("optEngineText");
  const optModeText = document.getElementById("optModeText");
  const optResumeBtn = document.getElementById("optResumeBtn");
  const optBlockedCount = document.getElementById("optBlockedCount");
  const optAddBlockForm = document.getElementById("optAddBlockForm");
  const optSiteNameInput = document.getElementById("optSiteNameInput");
  const optDomainInput = document.getElementById("optDomainInput");
  const optBlockedList = document.getElementById("optBlockedList");
  const pauseButtons = Array.from(document.querySelectorAll("[data-opt-pause]"));

  function setStatusClass(el, code) {
    if (!el) return;
    el.classList.remove(
      "status-connected",
      "status-closed",
      "status-connecting",
      "status-failed",
      "status-unresponsive"
    );
    el.classList.add(`status-${code || "connecting"}`);
  }

  function refreshOptionsView(forceCheck = false) {
    chrome.runtime.sendMessage(
      {
        type: "STORMGET_GET_POPUP_STATE",
        forceEngineCheck: forceCheck
      },
      (resp) => {
        if (!resp || !resp.ok) return;
        const st = resp.engineStatus || {};
        const ctrl = resp.controlState || {};
        const modeInfo = resp.modeInfo || {};

        document.documentElement.setAttribute(
          "data-theme",
          ctrl.themePreference === "light" ? "light" : "dark"
        );

        setStatusClass(optEngineBadge, st.code);
        setStatusClass(optEngineRing, st.code);
        optEngineBadge.textContent = st.shortLabel || "جاري الاتصال";
        optEngineText.textContent = st.displayText || "جارٍ الاتصال بالمحرك";

        optModeText.textContent = modeInfo.description || "الإضافة تعمل بشكل طبيعي";
        if (modeInfo.mode === "paused_all") {
          optResumeBtn.classList.remove("hidden");
        } else {
          optResumeBtn.classList.add("hidden");
        }

        const list = ctrl.blockedSites || [];
        optBlockedCount.textContent = String(list.length);
        optBlockedList.innerHTML = "";
        if (list.length === 0) {
          const empty = document.createElement("div");
          empty.className = "empty-state";
          empty.textContent = "لا توجد مواقع محظورة حاليًا.";
          optBlockedList.appendChild(empty);
          return;
        }

        for (const item of list) {
          const row = document.createElement("div");
          row.className = "blocked-site-row";

          const details = document.createElement("div");
          details.className = "blocked-site-details";
          const nameEl = document.createElement("span");
          nameEl.className = "blocked-site-name";
          nameEl.textContent = item.siteName || item.domain;
          const subEl = document.createElement("div");
          subEl.className = "blocked-site-sub";
          subEl.textContent = `${item.domain} • تاريخ الإضافة: ${item.addedAt || ""}`;
          details.appendChild(nameEl);
          details.appendChild(subEl);

          const actions = document.createElement("div");
          actions.className = "blocked-site-actions";
          const delBtn = document.createElement("button");
          delBtn.type = "button";
          delBtn.className = "mini-btn danger";
          delBtn.textContent = "إزالة الحظر";
          delBtn.addEventListener("click", () => {
            chrome.runtime.sendMessage(
              { type: "STORMGET_UNBLOCK_SITE", domain: item.domain },
              () => refreshOptionsView(false)
            );
          });
          actions.appendChild(delBtn);

          row.appendChild(details);
          row.appendChild(actions);
          optBlockedList.appendChild(row);
        }
      }
    );
  }

  optCheckEngineBtn.addEventListener("click", () => refreshOptionsView(true));

  optThemeToggleBtn.addEventListener("click", () => {
    const cur = document.documentElement.getAttribute("data-theme") || "dark";
    const next = cur === "dark" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", next);
    chrome.runtime.sendMessage({
      type: "STORMGET_SET_UI_PREFS",
      themePreference: next
    });
  });

  pauseButtons.forEach((btn) => {
    btn.addEventListener("click", () => {
      const durationKey = btn.getAttribute("data-opt-pause");
      chrome.runtime.sendMessage(
        { type: "STORMGET_SET_PAUSE", durationKey },
        () => refreshOptionsView(false)
      );
    });
  });

  optResumeBtn.addEventListener("click", () => {
    chrome.runtime.sendMessage({ type: "STORMGET_RESUME_NOW" }, () =>
      refreshOptionsView(false)
    );
  });

  optAddBlockForm.addEventListener("submit", (e) => {
    e.preventDefault();
    const rawDomain = (optDomainInput.value || "").trim();
    const siteName = (optSiteNameInput.value || "").trim();
    if (!rawDomain) return;
    chrome.runtime.sendMessage(
      {
        type: "STORMGET_BLOCK_SITE",
        domainOrUrl: rawDomain,
        siteName: siteName || rawDomain
      },
      (resp) => {
        if (resp && resp.ok) {
          optDomainInput.value = "";
          optSiteNameInput.value = "";
          refreshOptionsView(false);
        }
      }
    );
  });

  refreshOptionsView(true);
})();
