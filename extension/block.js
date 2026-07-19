(function () {
  // Original lines, written for MindGate — calm and matter-of-fact,
  // not preachy. A new one on every visit, picked client-side with
  // Math.random(); no backend involved.
  const QUOTES = [
    "Stay here. The life you want is built one decision at a time.",
    "This moment passes either way. Choose the version of it you'll respect later.",
    "You didn't lock this by accident. You locked it on purpose.",
    "The urge is loud right now. It will be quiet again soon.",
    "Future you is watching. Make them proud.",
    "Discipline isn't punishment. It's a promise you're keeping.",
    "You already decided. This is just the follow-through.",
    "Small refusals, repeated, become a different life.",
    "Nothing here was worth what you're building instead.",
    "The version of you that locked this trusted the version of you that's reading this.",
    "Not now doesn't mean never. It means not like this.",
    "This is the boring, quiet part. It's also the part that works.",
  ];

  function setRandomQuote() {
    const quote = QUOTES[Math.floor(Math.random() * QUOTES.length)];
    document.getElementById("quote").textContent = quote;
  }

  // Formats the time remaining on the current lock into something
  // readable — "12 Days Remaining", "4 Hours Remaining", "Locked
  // Forever" for an untimed lock, etc. Reads from chrome.storage.local
  // rather than talking to the daemon directly: block.html has no
  // native-messaging access of its own, and background.js already
  // keeps `lockInfo` fresh there every sync cycle.
  function formatRemaining(lockInfo) {
    if (!lockInfo || !lockInfo.locked) {
      // Rare edge case: the lock expired in the moments between this
      // tab being blocked and this page loading. Still calm, still
      // accurate — not an error state.
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

  document.getElementById("go-back").addEventListener("click", (e) => {
    e.preventDefault();
    if (window.history.length > 1) {
      window.history.back();
    } else {
      window.location.href = "https://www.google.com";
    }
  });

  setRandomQuote();
  setLockValue();
})();