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
// Requesting < 1 minute silently clamps to ~60s. We align with this reality.
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
 * If it fails, we log it, but we don't abort local enforcement. 
 * The daemon will handle the missing heartbeat by closing the browser.
 */
async function sendHeartbeat() {
  try {
    await sendToDaemon({ cmd: "ExtensionHeartbeat" });
  } catch (e) {
    console.warn("[MindGate] Heartbeat failed (daemon may be restarting or crashed):", e.message);
  }
}

/**
 * Rebuilds the declarativeNetRequest dynamic ruleset from the local 
 * 'websites' list. Redirects top-level navigations to block.html 
 * BEFORE Chrome attempts the network request.
 * 
 * Scoped to `resourceTypes: ["main_frame"]` to avoid breaking 
 * sub-resources or iframes on allowed sites.
 */
async function syncWebsiteBlockRules(domains = []) {
  try {
    const existing = await chrome.declarativeNetRequest.getDynamicRules();
    const removeRuleIds = existing.map((r) => r.id);

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
    console.log(`[MindGate] Website block rules synced: ${addRules.length} domain(s).`);
  } catch (e) {
    console.error("[MindGate] Failed to sync website block rules:", e.message);
  }
}

/**
 * Core initialization and sync. 
 * Reads rules directly from local storage (managed by the extension's Settings UI).
 */
async function initializeAndSync() {
  console.log("[MindGate] Initializing and syncing local rules...");
  
  // 1. Send immediate heartbeat on startup
  await sendHeartbeat();

  // 2. Load local rules and apply them
  const data = await chrome.storage.local.get(["websites", "keywords", "paths", "subreddits"]);
  const websites = data.websites || [];
  
  // Note: keywords, paths, and subreddits are handled by content.js, 
  // which reads directly from this same chrome.storage.local.
  
  await syncWebsiteBlockRules(websites);
  console.log("[MindGate] Local rules applied.");
}

// 1. Setup recurring heartbeat alarm (survives service worker termination)
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

// 3. INSTANT reactivity: When the user changes rules in the Settings UI, 
// update the blocking rules immediately. No polling needed.
chrome.storage.onChanged.addListener((changes, namespace) => {
  if (namespace === "local" && changes.websites) {
    console.log("[MindGate] Websites changed in storage, updating dNR rules...");
    syncWebsiteBlockRules(changes.websites.newValue || []);
  }
});

/**
 * Self-heal for the DNS-blackhole / declarativeNetRequest sync race.
 * 
 * If a tab navigates to a newly-added domain before the dNR rules have 
 * fully synced, Chrome might hit a raw DNS error. This listener catches 
 * `net::ERR_NAME_NOT_RESOLVED`, checks if the hostname is in our local 
 * blocked list, and forcefully redirects to block.html.
 * 
 * This turns a raw browser error into the premium MindGate block page.
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

  // Check local storage directly (no daemon round-trip needed)
  const data = await chrome.storage.local.get(["websites"]);
  const domains = data.websites || [];
  
  if (hostMatchesBlockedDomain(hostname, domains)) {
    console.log(`[MindGate] Self-heal: Redirecting blocked domain ${hostname} to block page.`);
    const blockUrl = chrome.runtime.getURL("block.html") + "?reason=website";
    chrome.tabs.update(details.tabId, { url: blockUrl });
  }
});