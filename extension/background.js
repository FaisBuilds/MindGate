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
 * (Re)builds the declarativeNetRequest dynamic ruleset from the
 * current website list, redirecting top-level navigations to
 * block.html BEFORE Chrome attempts the network request at all — this
 * is what makes whole-website blocks show the branded page instead of
 * Chrome's raw DNS_PROBE_FINISHED_NXDOMAIN / connection-error screen
 * (nftables/dnsmasq still exist underneath as the real fail-safe for
 * non-browser traffic; this only improves what the browser shows).
 *
 * Scoped to `resourceTypes: ["main_frame"]` ON PURPOSE — this only
 * intercepts a tab actually navigating TO a blocked domain, never a
 * sub-resource or iframe embed of that domain on some OTHER, allowed
 * page (e.g. a Discord widget embedded on an unrelated site). Getting
 * that wrong would silently break pages that have nothing to do with
 * what you're trying to block, which is exactly the kind of
 * regression to avoid here.
 *
 * Always fully replaces the ruleset (remove every existing dynamic
 * rule ID, then add fresh ones numbered 1..N) rather than diffing —
 * with a personal website list this is a handful of rules at most, so
 * atomic replace-on-every-sync is simpler and can't drift from
 * whatever the daemon actually reports.
 */
async function syncWebsiteBlockRules(domains) {
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
        // "||domain^" (Adblock-style) matches the domain itself and
        // any subdomain (www.discord.com, canary.discord.com, ...),
        // but not unrelated domains that merely contain the string
        // (e.g. "notdiscord.com" or "discord.com.evil.net") — same
        // domain-scoping principle content.js already applies to path
        // rules.
        urlFilter: `||${domain}^`,
        resourceTypes: ["main_frame"],
      },
    }));

    await chrome.declarativeNetRequest.updateDynamicRules({ removeRuleIds, addRules });
    console.log(
      `[MindGate background] Website block rules synced: ${addRules.length} domain(s).`
    );
  } catch (e) {
    console.error("[MindGate background] Failed to sync website block rules:", e.message);
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

  // Stored alongside the rule data (not just used transiently here) so
  // block.html — a plain extension page with no native-messaging
  // access of its own — can read "how much longer is this locked"
  // straight out of chrome.storage.local instead of needing its own
  // bridge round trip. Stored every cycle, in both branches below, so
  // it can't go stale relative to what's actually enforced.
  await chrome.storage.local.set({
    lockInfo: {
      locked,
      unlockAt: statusResponse?.data?.lock?.unlock_at ?? null,
    },
  });

  if (!locked) {
    console.log("[MindGate background] Ruleset is not locked — clearing local enforcement.");
    await chrome.storage.local.set({ keywords: [], subreddits: [], paths: [], lockedWebsites: [] });
    await syncWebsiteBlockRules([]);
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
    const websites = (listResponse.data.websites || []).map((w) => w.domain);
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
      "[MindGate background] Extracted websites:", websites,
      "keywords:", keywords,
      "subreddits:", subreddits,
      "paths:", paths
    );

    // `lockedWebsites` is stored alongside the others (not just used
    // to build dNR rules) so the onErrorOccurred self-heal listener
    // below can check "is this hostname actually locked" straight out
    // of storage, without a second native-bridge round trip.
    await chrome.storage.local.set({ keywords, subreddits, paths, lockedWebsites: websites });
    await syncWebsiteBlockRules(websites);
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
// window.
//
// nftables/dnsmasq website blocks don't have this problem
// (engine.apply() is synchronous, so `mindgate lock` doesn't return
// until they're already active) — but the declarativeNetRequest-based
// website redirect rules ARE extension-side sync state, same as
// keywords/paths, so they share the exact same staleness window this
// fix closes.
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

/**
 * Self-heal for the DNS-blackhole / declarativeNetRequest sync race.
 *
 * Whole-website blocks are enforced at two layers that can race on a
 * brand-new lock:
 *   1. nftables/dnsmasq (daemon-side) blackholes the domain at DNS —
 *      synchronous, active the instant `mindgate lock` returns.
 *   2. declarativeNetRequest (here) redirects the request to
 *      block.html BEFORE a DNS lookup is even attempted — but only
 *      once this extension has actually synced that domain into its
 *      dynamic ruleset, which is async (bridge round trip).
 *
 * If a tab navigates to a freshly-locked domain before step 2 has
 * synced, step 2 never fires, the browser falls through to a real DNS
 * lookup, and step 1 wins instead — Chrome's raw
 * DNS_PROBE_FINISHED_NXDOMAIN instead of the pink card. This is a
 * one-time race on a domain's first hit after being newly locked, not
 * ongoing flakiness — once dNR has synced a domain once, every later
 * navigation to it is caught cleanly by step 2.
 *
 * Rather than trying to shrink the race window further, this reacts
 * directly to the failure signature: `net::ERR_NAME_NOT_RESOLVED` is
 * Chrome's actual net-error string for what displays as
 * DNS_PROBE_FINISHED_NXDOMAIN. On a main-frame nav failing with it, we
 * force an immediate sync (so `lockedWebsites` in storage can't be
 * the stale copy that lost the race) and then check whether the
 * failed hostname is actually locked; if so we redirect that exact
 * tab to block.html ourselves. This turns "sometimes raw error,
 * sometimes pretty page" into "always pretty page, occasionally with
 * a beat of raw error first" — within roughly one bridge round trip.
 */
function hostMatchesLockedDomain(hostname, domains) {
  const host = hostname.toLowerCase();
  return domains.some((d) => {
    const domain = (d || "").toLowerCase();
    return domain && (host === domain || host.endsWith("." + domain));
  });
}

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

  // Force a fresh sync rather than trusting whatever was cached
  // before this navigation started — that cache is exactly what may
  // have just lost the race.
  await syncRules();

  chrome.storage.local.get(["lockedWebsites"], (data) => {
    const domains = data.lockedWebsites || [];
    if (!hostMatchesLockedDomain(hostname, domains)) return;

    const blockUrl = chrome.runtime.getURL("block.html") + "?reason=website";
    chrome.tabs.update(details.tabId, { url: blockUrl });
  });
});