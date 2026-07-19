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

  async function setRandomQuote() {
    let quote = FALLBACK_QUOTE;
    try {
      const res = await fetch(chrome.runtime.getURL("quotes.json"));
      const quotes = await res.json();
      if (Array.isArray(quotes) && quotes.length > 0) {
        quote = quotes[Math.floor(Math.random() * quotes.length)];
      }
    } catch (e) {
      console.warn("[MindGate] Could not load quotes.json, using fallback quote:", e.message);
    }
    document.getElementById("quote").textContent = quote.text;
    document.getElementById("quote-author").textContent = quote.author;
  }

  // ---------- lock status ----------
  //
  // Unchanged from before: block.html has no native-messaging access
  // of its own, so it reads the lockInfo that background.js already
  // keeps fresh in chrome.storage.local on every sync cycle, rather
  // than talking to the daemon directly.
  function formatRemaining(lockInfo) {
    if (!lockInfo || !lockInfo.locked) {
      return "Lock Expired";
    }

    if (lockInfo.unlockAt === null || lockInfo.unlockAt === undefined) {
      return "Locked Forever";
    }

    const remainingMs = lockInfo.unlockAt * 1000 - Date.now();
    if (remainingMs <= 0) {
      return "Lock Expired";
    }

    const minutes = Math.floor(remainingMs / 60000);
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
    const displayMinutes = Math.max(minutes, 1);
    return `${displayMinutes} Minute${displayMinutes === 1 ? "" : "s"} Remaining`;
  }

  function setLockValue() {
    chrome.storage.local.get(["lockInfo"], (data) => {
      document.getElementById("lock-value").textContent = formatRemaining(data.lockInfo);
    });
  }

  // ---------- ambient particles ----------
  //
  // Purely decorative, no logic dependency on anything above. A
  // handful of small dots drifting upward at staggered speeds/delays,
  // generated once on load so every visit feels slightly different.
  function spawnParticles() {
    const container = document.getElementById("particles");
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
})();