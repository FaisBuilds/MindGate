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
})();