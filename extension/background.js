const HOST_NAME = "com.mindgate.protector";
const ALARM_NAME = "mindgate-sync";
// FIX: Chrome enforces a hard 1-minute FLOOR on chrome.alarms periods
// for installed extensions — requesting 0.5 (30s) doesn't get you 30s,
// it silently gets clamped to ~60s. The daemon side (server.rs's
// HEARTBEAT_TIMEOUT) was written assuming the requested 30s actually
// happened, so status reads were stale — and thus wrongly reported
// "disconnected" — for roughly half of every real cycle. Requesting
// exactly what Chrome will actually give us removes the guesswork.
// This replaces the old 5s setInterval, which looked more responsive
// but was silently unreliable (see syncRules() below — MV3 service
// workers get killed after ~30s idle, taking any setInterval with
// them with zero error anywhere).
const SYNC_PERIOD_MINUTES = 1; // Chrome's real floor — matches server.rs's HEARTBEAT_TIMEOUT

/**
 * Sends a native-messaging request and resolves with the parsed
 * response, or rejects on a bridge/transport error. Small wrapper so
 * callers can `await` instead of nesting callbacks — matters more now
 * that syncRules() needs to make multiple calls in sequence rather
 * than one.
 */
function sendToDaemon(cmd, args) {
  return new Promise((resolve, reject) => {
    chrome.runtime.sendNativeMessage(HOST_NAME, { cmd, args }, (response) => {
      if (chrome.runtime.lastError) {
        reject(new Error(chrome.runtime.lastError.message));
        return;
      }
      resolve(response);
    });
  });
}

/**
 * FIX: this was silently dropped when syncRules() got rewritten for
 * the lock feature (Status -> List chain replaced the simpler old
 * flow). AppState.last_heartbeat on the daemon side is only ever
 * updated by an ExtensionHeartbeat request — nothing else touches it.
 * With this missing, `mindgate status` will always report
 * "Extension Connected: NO" even when Status/List calls are
 * succeeding perfectly, because the daemon has no record of the
 * extension ever having pinged it.
 *
 * This is fire-and-forget: a failed heartbeat isn't fatal to syncing
 * rules, so it's logged but doesn't block/abort syncRules().
 */
async function sendHeartbeat() {
  try {
    await sendToDaemon("ExtensionHeartbeat");
  } catch (e) {
    console.warn("[MindGate background] Heartbeat failed:", e.message);
  }
}

/**
 * The core sync step. Checks lock status FIRST, then only fetches and
 * stores the actual rule contents if the ruleset is currently locked.
 *
 * This matters because `mindgate add` no longer activates anything —
 * it only stages a rule server-side (see server.rs's dispatch split).
 * If this function synced whatever `List` returned unconditionally,
 * like the old version did, a keyword you'd only just staged — and
 * haven't locked yet — would still get blocked in the browser. That
 * defeats the entire point of separating `add` from `lock`. So:
 * unlocked (or lock expired) → clear local enforcement entirely,
 * regardless of what's staged server-side. Locked → sync and enforce
 * whatever's actually committed.
 */
async function syncRules() {
  console.log("[MindGate background] Checking lock status...");

  // FIX: send this every cycle, alongside the sync, so the daemon
  // always has a fresh timestamp regardless of lock state.
  await sendHeartbeat();

  let statusResponse;
  try {
    statusResponse = await sendToDaemon("Status");
  } catch (e) {
    console.warn("[MindGate background] Bridge error on Status:", e.message);
    return;
  }

  if (statusResponse?.result === "Error") {
    console.error("[MindGate background] Daemon returned an error on Status:", statusResponse.data?.message);
    return;
  }

  const locked = Boolean(statusResponse?.data?.lock?.locked);

  if (!locked) {
    console.log("[MindGate background] Ruleset is not locked — clearing local enforcement.");
    await chrome.storage.local.set({ keywords: [], subreddits: [], paths: [] });
    return;
  }

  console.log("[MindGate background] Ruleset is locked — syncing active rules...");

  let listResponse;
  try {
    listResponse = await sendToDaemon("List");
  } catch (e) {
    console.warn("[MindGate background] Bridge error on List:", e.message);
    return;
  }

  // Response::Rules(RuleSet) encodes as { result: "Rules", data: { websites, keywords, subreddits, paths } }
  if (listResponse?.result === "Rules" && listResponse?.data) {
    const keywords = (listResponse.data.keywords || []).map((k) => k.value);
    const subreddits = (listResponse.data.subreddits || []).map((s) => s.subreddit);
    // NEW: general domain + path-prefix rules, synced alongside
    // keywords/subreddits — doesn't change how either of those are
    // extracted or stored.
    const paths = (listResponse.data.paths || []).map((p) => ({
      domain: p.domain,
      path: p.path,
    }));

    console.log(
      "[MindGate background] Extracted keywords:", keywords,
      "subreddits:", subreddits,
      "paths:", paths
    );

    await chrome.storage.local.set({ keywords, subreddits, paths });
    console.log("[MindGate background] Active rules saved to local storage.");
  } else if (listResponse?.result === "Error") {
    console.error("[MindGate background] Daemon returned an error on List:", listResponse.data?.message);
  } else {
    console.error(
      "[MindGate background] Unexpected payload format on List. Expected result === 'Rules'.",
      "Received payload:", JSON.stringify(listResponse)
    );
  }
}

// chrome.alarms, not setInterval: MV3 background scripts are service
// workers, and Chrome kills an idle one after roughly 30s of no
// activity — which silently wipes out any running setInterval with
// zero error anywhere. That was the actual cause of "only old
// keywords work, new ones never sync": the interval died quietly in
// the background and nothing was polling anymore. chrome.alarms wakes
// a terminated service worker back up on schedule instead of dying
// with it, which is the whole reason this API exists.
chrome.alarms.create(ALARM_NAME, { periodInMinutes: SYNC_PERIOD_MINUTES });
chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === ALARM_NAME) syncRules();
});

chrome.runtime.onInstalled.addListener(() => {
  syncRules();
});

chrome.runtime.onStartup.addListener(() => {
  syncRules();
});

// FIX: this closes the "sometimes it doesn't lock" gap for keyword/
// subreddit blocks. Previously the ONLY thing that triggered a re-sync
// was the alarm, which fires at most once a minute. If you ran
// `mindgate lock` and then immediately loaded a page in the same
// cycle, the extension was still enforcing whatever it had synced
// BEFORE you locked — not a bug in the lock itself, just a stale-data
// window. Website blocks don't have this problem (engine.apply() is
// synchronous, so `mindgate lock` doesn't return until nftables/
// dnsmasq are already active) — this gap was extension-only.
//
// Triggering a sync on every real top-level navigation shrinks that
// window from "up to 60s" to "as fast as the native-bridge round trip
// takes" (a few hundred ms), without waiting on the alarm at all.
//
// `frameId === 0` filters to main-frame navigations only — this fires
// per-frame otherwise, and there's no reason to re-sync once for every
// ad iframe on a page.
//
// Debounced against SYNC_DEBOUNCE_MS: with `<all_urls>` host
// permissions this can fire on every tab switch/redirect across every
// open tab, and spawning a fresh native-bridge process (mindgate.sh)
// per event would be wasteful for something like an SPA that
// re-navigates internally many times a second. The periodic alarm
// still covers you even if nothing gets clicked for a while.
const SYNC_DEBOUNCE_MS = 3000;
let lastSyncAt = 0;

chrome.webNavigation.onBeforeNavigate.addListener((details) => {
  if (details.frameId !== 0) return;
  const now = Date.now();
  if (now - lastSyncAt < SYNC_DEBOUNCE_MS) return;
  lastSyncAt = now;
  syncRules();
});