(function () {
  // ---------- quote ----------
  //
  // Quotes live in quotes.json (single source of truth, easy to grow
  // toward the full curated set over time) rather than inline in this
  // file, so the list can be edited/extended without touching logic.
  // Picked once per page load with Math.random() — same behavior as
  // before, so a lock that's still open keeps its quote until the tab
  // is closed/reopened, per spec.
  //
  // block.html is same-origin with this file (both served from
  // chrome-extension://<id>/), so a relative fetch works without any
  // extra manifest permissions.
  const FALLBACK_QUOTE = {
    text: "The impediment to action advances action. What stands in the way becomes the way.",
    author: "Marcus Aurelius",
  };

  // ---------- no-repeat cycle ----------
  //
  // Tracks which quote indices have been shown in the current cycle.
  // Once every quote has been displayed, the array resets and a fresh
  // shuffled order begins. This guarantees no quote repeats until the
  // full set (~90 quotes) has been exhausted. Stored in
  // chrome.storage.local so it persists across tab closes and browser
  // restarts — a user opening 5 blocked tabs in a row sees 5 different
  // quotes, not the same one 5 times.

  function pickQuoteNoRepeat(quotes) {
    return new Promise((resolve) => {
      chrome.storage.local.get(["shownIndices"], (data) => {
        let shown = data.shownIndices || [];

        // Cycle complete — reset
        if (shown.length >= quotes.length) {
          shown = [];
        }

        // Build pool of unshown indices
        const remaining = [];
        for (let i = 0; i < quotes.length; i++) {
          if (!shown.includes(i)) remaining.push(i);
        }

        // Pick random from remaining
        const pick = remaining[Math.floor(Math.random() * remaining.length)];
        shown.push(pick);

        // Persist updated shown list
        chrome.storage.local.set({ shownIndices: shown });

        resolve(quotes[pick]);
      });
    });
  }

  async function setRandomQuote() {
    let quote = FALLBACK_QUOTE;
    try {
      const res = await fetch(chrome.runtime.getURL("quotes.json"));
      const quotes = await res.json();
      if (Array.isArray(quotes) && quotes.length > 0) {
        quote = await pickQuoteNoRepeat(quotes);
      }
    } catch (e) {
      console.warn("[MindGate] Could not load quotes.json, using fallback quote:", e.message);
    }

    // Only text and author are shown. Category exists in the JSON for
    // internal organization but is never displayed to the user.
    document.getElementById("quote").textContent = quote.text;
    document.getElementById("quote-author").textContent = quote.author;
  }

  // ---------- lock status ----------
  //
  // UPDATED: Reads 'lockState' (not 'lockInfo') and targets 'status-value'
  // to match the updated architecture.
  function formatRemaining(lockState) {
    if (!lockState || !lockState.locked) {
      return "Unlocked";
    }

    if (lockState.unlockAt === null || lockState.unlockAt === undefined) {
      return "Locked Forever";
    }

    // FIXED: unlockAt is already in milliseconds, no need to multiply by 1000
    const remainingMs = lockState.unlockAt - Date.now();
    if (remainingMs <= 0) {
      return "Lock Expired";
    }

    const totalSeconds = Math.floor(remainingMs / 1000);
    const minutes = Math.floor(totalSeconds / 60);
    const hours = Math.floor(minutes / 60);
    const days = Math.floor(hours / 24);

    if (days >= 1) {
      const remHours = hours % 24;
      return remHours > 0
        ? `${days} Day${days === 1 ? "" : "s"}, ${remHours} Hour${remHours === 1 ? "" : "s"} Remaining`
        : `${days} Day${days === 1 ? "" : "s"} Remaining`;
    }
    if (hours >= 1) {
      const remMinutes = minutes % 60;
      return remMinutes > 0
        ? `${hours} Hour${hours === 1 ? "" : "s"}, ${remMinutes} Minute${remMinutes === 1 ? "" : "s"} Remaining`
        : `${hours} Hour${hours === 1 ? "" : "s"} Remaining`;
    }
    
    const displayMinutes = Math.max(Math.floor(totalSeconds / 60), 0);
    const displaySeconds = totalSeconds % 60;
    return `${displayMinutes}m ${displaySeconds}s Remaining`;
  }

  function setLockValue() {
    // FIXED: Reads 'lockState' and targets 'status-value'
    chrome.storage.local.get(["lockState"], (data) => {
      const statusElement = document.getElementById("status-value");
      if (statusElement) {
        statusElement.textContent = formatRemaining(data.lockState);
      }
    });
  }

  // ---------- ambient particles ----------
  //
  // Purely decorative, no logic dependency on anything above. A
  // handful of small dots drifting upward at staggered speeds/delays,
  // generated once on load so every visit feels slightly different.
  function spawnParticles() {
    const container = document.getElementById("particles");
    if (!container) return;
    
    const COUNT = 18;
    for (let i = 0; i < COUNT; i++) {
      const p = document.createElement("div");
      p.className = "particle";
      p.style.left = `${Math.random() * 100}%`;
      p.style.animationDuration = `${12 + Math.random() * 10}s`;
      p.style.animationDelay = `${Math.random() * 12}s`;
      p.style.opacity = String(0.3 + Math.random() * 0.4);
      container.appendChild(p);
    }
  }

  setRandomQuote();
  setLockValue();
  spawnParticles();

  // ==========================================
  // NEW: LIVE LOCK COUNTDOWN EXTENSION
  // ==========================================
  //
  // Extends the existing setLockValue to update every second.
  // This gives the user a live, ticking countdown on the block page
  // so they can see exactly when their focus session ends.
  
  setInterval(() => {
    setLockValue();
  }, 1000);

})();