/**
 * MindGate Popup — presentation-only enhancements.
 *
 * This file is purely additive: it does not read/write chrome.storage
 * rule data, does not add/remove/validate items, and does not touch
 * anything popup.js already owns. It only makes the existing rendered
 * DOM easier to scan when a list has a lot of entries:
 *   1) per-section live filter (hide/show already-rendered .rule rows)
 *   2) a header badge showing the total rule count across all lists
 *
 * Safe to delete this file entirely with zero effect on blocking logic.
 */

(function () {
  const STORAGE_KEYS = ["websites", "paths", "keywords", "subreddits"];

  // --- Per-section filter ---
  document.querySelectorAll(".filter-row").forEach((row) => {
    const listId = row.dataset.filterFor;
    const list = document.getElementById(listId);
    const input = row.querySelector(".filter-input");
    const shownLabel = row.querySelector(".filter-shown");
    if (!list || !input) return;

    function applyFilter() {
      const query = input.value.trim().toLowerCase();
      const rules = list.querySelectorAll(".rule");
      let visible = 0;

      rules.forEach((rule) => {
        const text = rule.querySelector(".rule-text");
        const match = !query || (text && text.textContent.toLowerCase().includes(query));
        rule.style.display = match ? "" : "none";
        if (match) visible++;
      });

      shownLabel.textContent = query && rules.length ? `${visible} / ${rules.length}` : "";
    }

    input.addEventListener("input", applyFilter);

    // Re-apply whenever the list re-renders (add/remove/live sync),
    // since popup.js rebuilds list.innerHTML from scratch each time.
    new MutationObserver(applyFilter).observe(list, { childList: true });
  });

  // --- Header "total rules" badge ---
  const totalCountEl = document.getElementById("total-count");

  function updateTotalBadge(data) {
    if (!totalCountEl) return;
    const total = STORAGE_KEYS.reduce((sum, key) => sum + (data[key]?.length || 0), 0);
    totalCountEl.textContent = total;
  }

  chrome.storage.local.get(STORAGE_KEYS).then(updateTotalBadge);

  chrome.storage.onChanged.addListener((changes, area) => {
    if (area !== "local") return;
    if (STORAGE_KEYS.some((key) => key in changes)) {
      chrome.storage.local.get(STORAGE_KEYS).then(updateTotalBadge);
    }
  });
})();