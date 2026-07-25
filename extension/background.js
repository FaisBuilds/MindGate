/**
   * MindGate Background Service Worker (MV3)
   * 
   * Per MVP1: "The browser extension owns blocking. The daemon only protects the blocker."
   * 
   * Responsibilities:
   * 1. Continuously send ExtensionHeartbeat to the daemon.
   * 2. Manage website blocking rules locally via chrome.storage.local.
   * 3. Apply declarativeNetRequest rules instantly when local storage changes.
   */

  const HOST_NAME = "com.mindgate.protector";
  const HEARTBEAT_ALARM_NAME = "mindgate-heartbeat";
  const LOCK_EXPIRY_ALARM_NAME = "mindgate-lock-expiry";

  // Chrome enforces a hard 1-minute FLOOR on chrome.alarms periods for installed extensions.
  const HEARTBEAT_PERIOD_MINUTES = 0.5;

  function sendToDaemon(payload) {
    return new Promise((resolve, reject) => {
      chrome.runtime.sendNativeMessage(HOST_NAME, payload, (response) => {
        if (chrome.runtime.lastError) {
          reject(new Error(chrome.runtime.lastError.message));
          return;
        }
        resolve(response);
      });
    });
  }

  async function sendHeartbeat() {
    try {
      const { lockState } = await chrome.storage.local.get("lockState");
      await sendToDaemon({ 
        cmd: "ExtensionHeartbeat",
        lockState: lockState || null 
      });
    } catch (e) {
      console.warn("[MindGate] Heartbeat failed:", e.message);
    }
  }

  function isLockActive(lockState) {
    if (!lockState || !lockState.locked) {
      return false;
    }
    if (!lockState.unlockAt) {
      return true;
    }
    const now = Date.now();
    if (lockState.unlockAt <= now) {
      return false;
    }
    return true;
  }

  async function syncWebsiteBlockRules(domains = []) {
    try {
      const { lockState } = await chrome.storage.local.get("lockState");
      const isLocked = isLockActive(lockState);
      
      console.log(`[MindGate] syncWebsiteBlockRules: isLocked = ${isLocked}, lockState =`, lockState);

      const existing = await chrome.declarativeNetRequest.getDynamicRules();
      const removeRuleIds = existing.map((r) => r.id);

      if (!isLocked) {
        if (removeRuleIds.length > 0) {
          await chrome.declarativeNetRequest.updateDynamicRules({ removeRuleIds, addRules: [] });
          console.log("[MindGate] Unlocked: Website block rules cleared.");
        } else {
          console.log("[MindGate] Unlocked: No rules to clear.");
        }
        return;
      }

      const addRules = domains.map((domain, index) => ({
        id: index + 1,
        priority: 1,
        action: {
          type: "redirect",
          redirect: { extensionPath: "/block.html?reason=website" },
        },
        condition: {
          urlFilter: `||${domain}^`,
          resourceTypes: ["main_frame"],
        },
      }));

      await chrome.declarativeNetRequest.updateDynamicRules({ removeRuleIds, addRules });
      console.log(`[MindGate] Locked: Website block rules synced: ${addRules.length} domain(s).`);
    } catch (e) {
      console.error("[MindGate] Failed to sync website block rules:", e.message);
    }
  }

  /**
   * NEW: schedules (or clears) a one-shot alarm that fires exactly at unlockAt,
   * so expiry is detected immediately instead of waiting for the next
   * heartbeat tick (which can lag by up to ~60s).
   */
  async function scheduleLockExpiryAlarm(lockState) {
    await chrome.alarms.clear(LOCK_EXPIRY_ALARM_NAME);
    if (lockState && lockState.locked && lockState.unlockAt) {
      chrome.alarms.create(LOCK_EXPIRY_ALARM_NAME, { when: lockState.unlockAt });
      console.log(`[MindGate] Scheduled expiry alarm for ${new Date(lockState.unlockAt).toISOString()}`);
    }
  }

  async function clearExpiredLockAndSync() {
    const { lockState } = await chrome.storage.local.get("lockState");
    if (lockState && lockState.locked && lockState.unlockAt && lockState.unlockAt <= Date.now()) {
      console.log("[MindGate] Expiry alarm fired: clearing lock and syncing rules.");
      await chrome.storage.local.remove("lockState");
      const { websites } = await chrome.storage.local.get("websites");
      await syncWebsiteBlockRules(websites || []);
      // NEW: tell the daemon right away instead of waiting for the next
      // periodic heartbeat tick — otherwise it keeps enforcing against a
      // lock_state that, from its point of view, hasn't cleared yet.
      await sendHeartbeat();
    }
  }

  async function initializeAndSync() {
    console.log("[MindGate] Initializing and syncing local rules...");
    
    const { lockState } = await chrome.storage.local.get("lockState");
    if (lockState && lockState.locked && lockState.unlockAt && lockState.unlockAt <= Date.now()) {
      console.log("[MindGate] Found expired lock on startup, clearing...");
      await chrome.storage.local.remove("lockState");
    } else {
      // Re-arm the exact-expiry alarm in case the service worker was
      // restarted (alarms persist, but do this defensively on init too).
      await scheduleLockExpiryAlarm(lockState);
    }

    await sendHeartbeat();

    const data = await chrome.storage.local.get(["websites", "keywords", "paths", "subreddits"]);
    const websites = data.websites || [];
    
    await syncWebsiteBlockRules(websites);
    console.log("[MindGate] Local rules initialization complete.");
  }

  chrome.alarms.create(HEARTBEAT_ALARM_NAME, { periodInMinutes: HEARTBEAT_PERIOD_MINUTES });
  chrome.alarms.onAlarm.addListener(async (alarm) => {
    if (alarm.name === LOCK_EXPIRY_ALARM_NAME) {
      await clearExpiredLockAndSync();
      return;
    }
    if (alarm.name === HEARTBEAT_ALARM_NAME) {
      // Keep this as a safety net in case the exact-expiry alarm was ever
      // missed (e.g. system sleep), but it's no longer the primary path.
      await clearExpiredLockAndSync();
      await sendHeartbeat();
    }
  });

  setTimeout(() => {
    console.log("[MindGate] Extension woke up. Sending initial heartbeat...");
    sendHeartbeat();
  }, 1000);

  chrome.runtime.onInstalled.addListener(initializeAndSync);
  chrome.runtime.onStartup.addListener(initializeAndSync);

  chrome.storage.onChanged.addListener((changes, namespace) => {
    if (namespace !== "local") return;
    
    if (changes.websites) {
      console.log("[MindGate] Websites changed in storage, updating dNR rules...");
      syncWebsiteBlockRules(changes.websites.newValue || []);
    }
    
    if (changes.lockState) {
      console.log("[MindGate] Lock state changed, re-evaluating dNR rules...");
      // NEW: (re)schedule the exact-expiry alarm whenever lockState changes,
      // e.g. when a fresh lock is created via the popup.
      scheduleLockExpiryAlarm(changes.lockState.newValue).then(() => {
        chrome.storage.local.get(["websites"]).then(data => {
          syncWebsiteBlockRules(data.websites || []);
        });
      });
      // NEW: push the new lock state to the daemon immediately. Without
      // this, the daemon only learns about a brand-new lock (or an
      // explicit unlock) at the next periodic ~60s heartbeat tick, which
      // runs on its own clock unrelated to when the lock actually
      // started — the exact gap that let `mindgate stop` slip through
      // early against a stale cached lock_state.
      sendHeartbeat();
    }
  });

  function hostMatchesBlockedDomain(hostname, domains) {
    const host = hostname.toLowerCase();
    return domains.some((d) => {
      const domain = (d || "").toLowerCase();
      return domain && (host === domain || host.endsWith("." + domain));
    });
  }

  // ==========================================
  // NEW: remember the real URL behind a block, so we can send the tab
  // back to it once the lock clears (instead of just saying "open a new
  // tab"). Stored in chrome.storage.session — cleared automatically when
  // the browser session ends, and separate from the persistent 'local'
  // namespace used for rules/lockState.
  // ==========================================

  function sessionKeyForTab(tabId) {
    return `originalUrl:${tabId}`;
  }

  chrome.webNavigation.onBeforeNavigate.addListener(async (details) => {
    if (details.frameId !== 0) return; // main frame only

    let hostname;
    try {
      hostname = new URL(details.url).hostname;
    } catch {
      return;
    }
    if (!hostname) return;

    const data = await chrome.storage.local.get(["websites", "lockState"]);
    const domains = data.websites || [];
    const isLocked = isLockActive(data.lockState);

    // Only remember it if this navigation is actually about to be blocked.
    if (isLocked && hostMatchesBlockedDomain(hostname, domains)) {
      await chrome.storage.session.set({ [sessionKeyForTab(details.tabId)]: details.url });
    }
  });

  // Best-effort cleanup so stale entries don't pile up in session storage.
  chrome.tabs.onRemoved.addListener((tabId) => {
    chrome.storage.session.remove(sessionKeyForTab(tabId)).catch(() => {});
  });

  // Single source of truth for "is it still locked right now", plus a way
  // for block.js to fetch back the URL it was originally headed to.
  // Keeping this logic here (instead of duplicating isLockActive in
  // block.js) avoids the two files drifting out of sync again.
  chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (!message || typeof message !== "object") return;

    if (message.cmd === "checkLock") {
      chrome.storage.local.get("lockState").then(({ lockState }) => {
        sendResponse({ isLocked: isLockActive(lockState) });
      });
      return true; // keep the message channel open for the async response
    }

    if (message.cmd === "getOriginalUrl") {
      const tabId = sender.tab && sender.tab.id;
      if (tabId === undefined) {
        sendResponse({ url: null });
        return true;
      }
      chrome.storage.session.get(sessionKeyForTab(tabId)).then((data) => {
        sendResponse({ url: data[sessionKeyForTab(tabId)] || null });
      });
      return true;
    }
  });

  chrome.webNavigation.onErrorOccurred.addListener(async (details) => {
    if (details.frameId !== 0) return;
    if (details.error !== "net::ERR_NAME_NOT_RESOLVED") return;

    let hostname;
    try {
      hostname = new URL(details.url).hostname;
    } catch {
      return;
    }
    if (!hostname) return;

    const data = await chrome.storage.local.get(["websites", "lockState"]);
    const domains = data.websites || [];
    const isLocked = isLockActive(data.lockState);
    
    if (isLocked && hostMatchesBlockedDomain(hostname, domains)) {
      console.log(`[MindGate] Self-heal: Redirecting blocked domain ${hostname} to block page.`);
      const blockUrl = chrome.runtime.getURL("block.html") + "?reason=website";
      chrome.tabs.update(details.tabId, { url: blockUrl });
    }
  });