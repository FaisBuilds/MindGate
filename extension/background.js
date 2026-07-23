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

// Chrome enforces a hard 1-minute FLOOR on chrome.alarms periods for installed extensions.
const HEARTBEAT_PERIOD_MINUTES = 1;

/**
 * Sends a native-messaging request and resolves with the parsed response.
 */
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

/**
 * Fire-and-forget heartbeat. 
 */
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

/**
 * Rebuilds the declarativeNetRequest dynamic ruleset from the local 'websites' list.
 */
async function syncWebsiteBlockRules(domains = []) {
  try {
    // STRICT CHECK: Must be explicitly true
    const { lockState } = await chrome.storage.local.get("lockState");
    const isLocked = !!(lockState && lockState.locked === true);
    
    console.log(`[MindGate] syncWebsiteBlockRules: isLocked = ${isLocked}`);

    const existing = await chrome.declarativeNetRequest.getDynamicRules();
    const removeRuleIds = existing.map((r) => r.id);

    // If NOT locked, aggressively clear any existing blocking rules
    if (!isLocked) {
      if (removeRuleIds.length > 0) {
        await chrome.declarativeNetRequest.updateDynamicRules({ removeRuleIds, addRules: [] });
        console.log("[MindGate] Unlocked: Website block rules cleared.");
      } else {
        console.log("[MindGate] Unlocked: No rules to clear.");
      }
      return;
    }

    // If LOCKED, apply the rules
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
 * Core initialization and sync. 
 */
async function initializeAndSync() {
  console.log("[MindGate] Initializing and syncing local rules...");
  
  await sendHeartbeat();

  // Fetch websites AND lockState together
  const data = await chrome.storage.local.get(["websites", "keywords", "paths", "subreddits", "lockState"]);
  const websites = data.websites || [];
  
  await syncWebsiteBlockRules(websites);
  console.log("[MindGate] Local rules initialization complete.");
}

// 1. Setup recurring heartbeat alarm
chrome.alarms.create(HEARTBEAT_ALARM_NAME, { periodInMinutes: HEARTBEAT_PERIOD_MINUTES });
chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === HEARTBEAT_ALARM_NAME) {
    sendHeartbeat();
  }
});

setTimeout(() => {
  console.log("[MindGate] Extension woke up. Sending initial heartbeat...");
  sendHeartbeat();
}, 1000);

// 2. Initialize on install, update, or browser startup
chrome.runtime.onInstalled.addListener(initializeAndSync);
chrome.runtime.onStartup.addListener(initializeAndSync);

// 3. INSTANT reactivity
chrome.storage.onChanged.addListener((changes, namespace) => {
  if (namespace !== "local") return;
  
  if (changes.websites) {
    console.log("[MindGate] Websites changed in storage, updating dNR rules...");
    syncWebsiteBlockRules(changes.websites.newValue || []);
  }
  
  if (changes.lockState) {
    console.log("[MindGate] Lock state changed, re-evaluating dNR rules...");
    chrome.storage.local.get(["websites"]).then(data => {
      syncWebsiteBlockRules(data.websites || []);
    });
  }
});

/**
 * Self-heal for the DNS-blackhole / declarativeNetRequest sync race.
 */
function hostMatchesBlockedDomain(hostname, domains) {
  const host = hostname.toLowerCase();
  return domains.some((d) => {
    const domain = (d || "").toLowerCase();
    return domain && (host === domain || host.endsWith("." + domain));
  });
}

chrome.webNavigation.onErrorOccurred.addListener(async (details) => {
  if (details.frameId !== 0) return; // Main frame only
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
  const isLocked = !!(data.lockState && data.lockState.locked === true);
  
  // STRICT CHECK: Only self-heal redirect if we are actually locked
  if (isLocked && hostMatchesBlockedDomain(hostname, domains)) {
    console.log(`[MindGate] Self-heal: Redirecting blocked domain ${hostname} to block page.`);
    const blockUrl = chrome.runtime.getURL("block.html") + "?reason=website";
    chrome.tabs.update(details.tabId, { url: blockUrl });
  }
});