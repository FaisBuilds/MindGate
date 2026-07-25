/**
 * MindGate Settings Popup
 * 
 * Manages blocking rules locally via chrome.storage.local.
 * Changes are instantly picked up by background.js and content.js
 * through the chrome.storage.onChanged listener.
 */

(function () {
  // Storage keys — must match background.js and content.js
  const STORAGE_KEYS = {
    website: "websites",
    path: "paths",
    keyword: "keywords",
    subreddit: "subreddits",
  };

  // Input elements
  const inputs = {
    website: document.getElementById("website-input"),
    path: document.getElementById("path-input"),
    keyword: document.getElementById("keyword-input"),
    subreddit: document.getElementById("subreddit-input"),
  };

  // List elements
  const lists = {
    website: document.getElementById("websites-list"),
    path: document.getElementById("paths-list"),
    keyword: document.getElementById("keywords-list"),
    subreddit: document.getElementById("subreddits-list"),
  };

  // Count elements
  const counts = {
    website: document.getElementById("websites-count"),
    path: document.getElementById("paths-count"),
    keyword: document.getElementById("keywords-count"),
    subreddit: document.getElementById("subreddits-count"),
  };

  // --- Validation ---

  function isValidDomain(s) {
    const trimmed = s.trim().toLowerCase();
    if (!trimmed || /\s/.test(trimmed)) return false;
    if (trimmed.includes("://") || trimmed.includes("/")) return false;
    const labels = trimmed.split(".");
    if (labels.length < 2) return false;
    return labels.every(
      (l) => l.length > 0 && /^[a-z0-9-]+$/.test(l) && !l.startsWith("-") && !l.endsWith("-")
    );
  }

  function parsePathInput(s) {
    const trimmed = s.trim();
    const slashIdx = trimmed.indexOf("/");
    if (slashIdx === -1) return null;
    const domain = trimmed.slice(0, slashIdx).toLowerCase();
    const path = trimmed.slice(slashIdx);
    if (!isValidDomain(domain) || path.length <= 1) return null;
    return { domain, path };
  }

  // --- Rendering ---

  function renderList(type, items) {
    const list = lists[type];
    const count = counts[type];
    list.innerHTML = "";
    count.textContent = items.length;

    if (items.length === 0) {
      const empty = document.createElement("div");
      empty.className = "empty";
      empty.textContent = "None yet";
      list.appendChild(empty);
      return;
    }

    items.forEach((item, index) => {
      const rule = document.createElement("div");
      rule.className = "rule";

      const text = document.createElement("span");
      text.className = "rule-text";
      text.textContent = formatItem(type, item);

      const del = document.createElement("button");
      del.className = "delete-btn";
      del.textContent = "×";
      del.title = "Remove";
      del.addEventListener("click", () => removeItem(type, index));

      rule.appendChild(text);
      rule.appendChild(del);
      list.appendChild(rule);
    });
  }

  function formatItem(type, item) {
    if (type === "path") return `${item.domain}${item.path}`;
    return item;
  }

  // --- Storage operations ---

  async function loadAll() {
    const data = await chrome.storage.local.get(Object.values(STORAGE_KEYS));
    for (const type of Object.keys(STORAGE_KEYS)) {
      const key = STORAGE_KEYS[type];
      renderList(type, data[key] || []);
    }
  }

  async function addItem(type, value) {
    const key = STORAGE_KEYS[type];
    const data = await chrome.storage.local.get([key]);
    const items = data[key] || [];

    // Check for duplicates
    const isDuplicate = items.some((existing) => {
      if (type === "path") {
        return existing.domain === value.domain && existing.path === value.path;
      }
      return existing === value;
    });

    if (isDuplicate) return;

    items.push(value);
    await chrome.storage.local.set({ [key]: items });
    renderList(type, items);
  }

  async function removeItem(type, index) {
    const key = STORAGE_KEYS[type];
    const data = await chrome.storage.local.get([key]);
    const items = data[key] || [];
    items.splice(index, 1);
    await chrome.storage.local.set({ [key]: items });
    renderList(type, items);
  }

  // --- Event listeners ---

  // Add buttons
  document.querySelectorAll(".add-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      const type = btn.dataset.type;
      handleAdd(type);
    });
  });

  // Enter key in inputs
  Object.entries(inputs).forEach(([type, input]) => {
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        handleAdd(type);
      }
    });
  });

  function handleAdd(type) {
    const input = inputs[type];
    const raw = input.value.trim();
    if (!raw) return;

    if (type === "website") {
      if (!isValidDomain(raw)) {
        flashError(input);
        return;
      }
      addItem("website", raw.toLowerCase());
    } else if (type === "path") {
      const parsed = parsePathInput(raw);
      if (!parsed) {
        flashError(input);
        return;
      }
      addItem("path", parsed);
    } else if (type === "keyword") {
      addItem("keyword", raw.toLowerCase());
    } else if (type === "subreddit") {
      let clean = raw.toLowerCase().trim();
      if (clean.startsWith("r/")) clean = clean.slice(2);
      if (clean.includes("/") || clean.includes(" ")) {
        flashError(input);
        return;
      }
      addItem("subreddit", clean);
    }

    input.value = "";
  }

  function flashError(input) {
    input.style.borderColor = "#ef4444";
    setTimeout(() => {
      input.style.borderColor = "";
    }, 800);
  }

  // Live updates from storage (e.g., if changed elsewhere)
  chrome.storage.onChanged.addListener((changes, area) => {
    if (area !== "local") return;
    for (const type of Object.keys(STORAGE_KEYS)) {
      const key = STORAGE_KEYS[type];
      if (changes[key]) {
        renderList(type, changes[key].newValue || []);
      }
    }
  });

  // --- Init ---
  loadAll();

  // ==========================================
  // FOCUS LOCK UI LOGIC (UI ONLY)
  // ==========================================

  function updateLockUI(lockState) {
    const lockControls = document.getElementById("lock-controls");
    const lockedView = document.getElementById("locked-view");
    const statusText = document.getElementById("lock-status-text");
    const ruleSections = document.querySelectorAll(".section:not(.lock-section)");

    if (lockState && lockState.locked) {
      if (lockState.unlockAt === null || lockState.unlockAt === undefined) {
        document.getElementById("timer-display").textContent = "FOREVER";
      } else if (lockState.unlockAt > Date.now()) {
        startTimerCountdown(lockState.unlockAt);
      } else {
        // Expired, treat as unlocked
        unlockSystem();
        return;
      }

      lockControls.style.display = "none";
      lockedView.style.display = "block";
      statusText.textContent = "Locked";
      statusText.classList.add("locked-active");
      
      ruleSections.forEach(s => s.classList.add("locked"));
    } else {
      unlockSystem();
    }
  }

  function unlockSystem() {
    const lockControls = document.getElementById("lock-controls");
    const lockedView = document.getElementById("locked-view");
    const statusText = document.getElementById("lock-status-text");
    const ruleSections = document.querySelectorAll(".section:not(.lock-section)");

    lockControls.style.display = "block";
    lockedView.style.display = "none";
    statusText.textContent = "Unlocked";
    statusText.classList.remove("locked-active");
    
    ruleSections.forEach(s => s.classList.remove("locked"));
    clearInterval(window.lockTimer);
  }

  // UI-ONLY Timer: Does NOT clear storage. Background.js handles expiration.
  function startTimerCountdown(unlockAt) {
    clearInterval(window.lockTimer);
    
    const updateTimer = () => {
      const remaining = Math.max(0, unlockAt - Date.now());
      const hours = Math.floor(remaining / 3600000);
      const minutes = Math.floor((remaining % 3600000) / 60000);
      const seconds = Math.floor((remaining % 60000) / 1000);
      
      document.getElementById("timer-display").textContent = 
        `${hours.toString().padStart(2, '0')}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`;
      
      // NOTE: We do NOT clear storage here. If the popup is closed, this interval dies.
      // background.js is responsible for checking expiration and clearing storage.
      if (remaining <= 0) {
        clearInterval(window.lockTimer);
        unlockSystem();
      }
    };
    
    updateTimer();
    window.lockTimer = setInterval(updateTimer, 1000);
  }

  // Handle Preset Buttons
  document.querySelectorAll(".lock-btn-sm").forEach((btn) => {
    btn.addEventListener("click", () => {
      const durationSecs = parseInt(btn.dataset.duration, 10);
      const unlockAt = Date.now() + (durationSecs * 1000);
      chrome.storage.local.set({ lockState: { locked: true, unlockAt } });
    });
  });

  // Handle Custom Lock Button
  document.getElementById("custom-lock-btn").addEventListener("click", () => {
    const value = parseInt(document.getElementById("custom-duration").value, 10);
    const multiplier = parseInt(document.getElementById("custom-unit").value, 10);
    
    if (isNaN(value) || value < 1) {
      const input = document.getElementById("custom-duration");
      input.style.borderColor = "#ef4444";
      setTimeout(() => input.style.borderColor = "", 800);
      return;
    }

    const durationSecs = value * multiplier;
    const unlockAt = Date.now() + (durationSecs * 1000);
    chrome.storage.local.set({ lockState: { locked: true, unlockAt } });
  });

  // ==========================================
  // DYNAMIC MAX LIMITS FOR CUSTOM INPUT
  // ==========================================
  const unitSelect = document.getElementById("custom-unit");
  const durationInput = document.getElementById("custom-duration");

  const maxLimits = {
    "60": 180,         // Minutes -> max 180 (3 hours)
    "3600": 48,        // Hours -> max 48 (2 days)
    "86400": 30,       // Days -> max 30 (1 month)
    "604800": 8,       // Weeks -> max 8 (2 months)
    "2592000": 6,      // Months -> max 6 (half a year)
    "31536000": 2      // Years -> max 2 (2 years)
  };

  unitSelect.addEventListener("change", () => {
    const selectedUnit = unitSelect.value;
    durationInput.max = maxLimits[selectedUnit] || 180;
    
    const currentVal = parseInt(durationInput.value, 10);
    if (currentVal > durationInput.max) {
      durationInput.value = durationInput.max;
    }
  });

  // Initialize lock state on load
  chrome.storage.local.get("lockState").then(data => {
    updateLockUI(data.lockState);
  });

  // Listen for lock state changes
  chrome.storage.onChanged.addListener((changes) => {
    if (changes.lockState) {
      updateLockUI(changes.lockState.newValue);
    }
  });

})();