(function () {
  // Cached rules, refreshed once on load and again any time the
  // synced ruleset changes while this tab is open (see the
  // chrome.storage.onChanged listener below). Cached rather than
  // re-fetched from chrome.storage on every check because SPA
  // navigations can fire several times a second (e.g. YouTube's
  // internal router) — that shouldn't mean an async storage read on
  // every single one.
  let cachedKeywords = [];
  let cachedSubreddits = [];
  let cachedPaths = [];
  let rulesLoaded = false;

  function loadRules(onLoaded) {
    try {
      chrome.storage.local.get(["keywords", "subreddits", "paths"], (data) => {
        cachedKeywords = data.keywords || [];
        cachedSubreddits = data.subreddits || [];
        cachedPaths = data.paths || [];
        rulesLoaded = true;
        if (onLoaded) onLoaded();
      });
    } catch (e) {
      // Stale script, context already dead — nothing to do here but
      // avoid an uncaught exception; see isExtensionContextValid()'s
      // comment on blockPage() for the full reasoning.
      console.warn("[MindGate] loadRules failed, likely stale context:", e.message);
    }
  }

  // Keep the cache fresh if a sync happens while the tab is already
  // open (e.g. background.js's periodic alarm, or a lock just kicked
  // in) — otherwise a tab opened before you ran `mindgate lock` would
  // keep enforcing the stale (empty) ruleset it started with until the
  // next real navigation.
  chrome.storage.onChanged.addListener((changes, area) => {
    if (area !== "local") return;
    if (changes.keywords) cachedKeywords = changes.keywords.newValue || [];
    if (changes.subreddits) cachedSubreddits = changes.subreddits.newValue || [];
    if (changes.paths) cachedPaths = changes.paths.newValue || [];
  });

  function checkCurrentUrl() {
    if (!rulesLoaded) return;
    if (cachedKeywords.length === 0 && cachedSubreddits.length === 0 && cachedPaths.length === 0) return;

    const currentUrl = window.location.href.toLowerCase();

    // 0. Domain + path-prefix rules. Properly domain-scoped: the
    // hostname must equal the rule's domain, or be a subdomain of it,
    // and only THEN is the path prefix checked. Works for any site.
    const currentHost = window.location.hostname.toLowerCase();
    const currentPath = window.location.pathname.toLowerCase();
    for (const rule of cachedPaths) {
      const ruleDomain = (rule.domain || "").toLowerCase();
      if (!ruleDomain) continue;

      const hostMatches =
        currentHost === ruleDomain || currentHost.endsWith("." + ruleDomain);
      if (!hostMatches) continue;

      let rulePath = (rule.path || "").toLowerCase();
      if (!rulePath.startsWith("/")) rulePath = "/" + rulePath;

      if (currentPath.startsWith(rulePath)) {
        blockPage(`Path block matched: ${ruleDomain}${rulePath}`);
        return true;
      }
    }

    // 1. Legacy subreddit blocks (e.g. "r/gaming" entries that predate
    // the `add path` command and still live in the old subreddits list)
    for (const sub of cachedSubreddits) {
      let cleanSub = sub.toLowerCase();
      if (cleanSub.startsWith("r/")) {
        cleanSub = cleanSub.substring(2);
      }

      const subredditPattern = new RegExp(`(/r/|/reddit.com/r/)${cleanSub}(\\b|/|\\?)`, "i");
      if (subredditPattern.test(currentUrl)) {
        blockPage(`Subreddit block matched: r/${cleanSub}`);
        return true;
      }
    }

    // 2. Keyword blocks against the URL
    for (const keyword of cachedKeywords) {
      const cleanKeyword = keyword.toLowerCase();
      if (currentUrl.includes(cleanKeyword)) {
        blockPage(`URL Keyword block matched: "${keyword}"`);
        return true;
      }
    }

    return false;
  }

  // 3. Dynamic keyword scanning (DOM text content matching). Debounced
  // and delayed: on an SPA route change the new page's content hasn't
  // rendered yet at the moment pushState/replaceState fires, so
  // scanning immediately would just re-read the PREVIOUS page's text.
  // A short delay gives the framework (React/etc.) a chance to render
  // before we scan.
  let domScanTimer = null;
  function scheduleDomScan(delayMs) {
    if (cachedKeywords.length === 0) return;
    clearTimeout(domScanTimer);
    domScanTimer = setTimeout(() => {
      if (!document.body) return;
      // A block may have already fired for this URL via the checks
      // above — no need to scan text on a page we've already replaced.
      const pageText = document.body.innerText.toLowerCase();
      for (const keyword of cachedKeywords) {
        if (pageText.includes(keyword.toLowerCase())) {
          blockPage(`Page Content Keyword block matched: "${keyword}"`);
          return;
        }
      }
    }, delayMs);
  }

  function runAllChecks({ isInitialLoad }) {
    const blocked = checkCurrentUrl();
    if (blocked) return;
    // Initial load already has its own DOMContentLoaded-gated scan
    // below; SPA navigations get a short render delay instead.
    if (!isInitialLoad) scheduleDomScan(400);
  }

  // --- Initial load ---
  loadRules(() => {
    runAllChecks({ isInitialLoad: true });
  });

  document.addEventListener("DOMContentLoaded", () => {
    scheduleDomScan(0);
  });

  // --- SPA / client-side navigation support ---
  //
  // Reddit, YouTube (incl. Shorts), Twitter/X, and most modern sites
  // are single-page apps: navigating within them calls
  // history.pushState/replaceState rather than loading a new document.
  // A content script only runs its top-level code once, when the tab's
  // FIRST real document loads — so without this, path/keyword rules
  // would only ever be checked against whatever URL was open when the
  // tab first loaded, and every subsequent in-app navigation (e.g.
  // clicking into r/gaming, or swiping to the next Short) would
  // silently never be checked at all.
  (function watchSpaNavigation() {
    const notifyNavigation = () => runAllChecks({ isInitialLoad: false });

    const wrapHistoryMethod = (methodName) => {
      const original = history[methodName];
      history[methodName] = function (...args) {
        const result = original.apply(this, args);
        notifyNavigation();
        return result;
      };
    };

    wrapHistoryMethod("pushState");
    wrapHistoryMethod("replaceState");
    window.addEventListener("popstate", notifyNavigation);
  })();

  // Navigates the tab to MindGate's bundled block page. A real
  // navigation (not an innerHTML swap) — this is what makes block.html
  // a genuine, single, reusable page rather than markup duplicated
  // inline at every call site. `reason` is passed through as a query
  // param purely for anyone debugging via devtools; block.html itself
  // deliberately never reads or displays it — it doesn't know or care
  // whether this was a keyword, website, or path match, only that
  // MindGate protected you.
  // True once this content script's extension context has been
  // invalidated (e.g. the extension was reloaded/updated while this
  // tab was already open). Chrome doesn't kill an old content script
  // when that happens — it just cuts off its chrome.* access — so
  // this script keeps running and can still fire blockPage() against
  // a URL, it just can no longer complete the redirect.
  function isExtensionContextValid() {
    try {
      return !!(chrome.runtime && chrome.runtime.id);
    } catch (e) {
      return false;
    }
  }

  function blockPage(reason) {
    // Check BEFORE touching the page. The bug this avoids: a stale
    // script calls window.stop() first (freezing the page mid-render),
    // THEN discovers chrome.runtime.getURL() is dead because the
    // context was invalidated — leaving a permanently blank, frozen
    // tab with no redirect ever firing and no way out but a manual
    // reload. Checking first means a stale script does nothing at
    // all instead: the fresh content script Chrome injects on the
    // next real navigation (or a manual reload) will catch the same
    // URL correctly.
    if (!isExtensionContextValid()) {
      console.warn(
        "[MindGate] Stale content script (extension context invalidated) — " +
          "skipping block. Reload this tab to restore enforcement."
      );
      return;
    }

    let blockUrl;
    try {
      blockUrl = chrome.runtime.getURL("block.html") + "?reason=" + encodeURIComponent(reason);
    } catch (e) {
      // Context died between the check above and here — same
      // no-op-rather-than-freeze reasoning.
      console.warn("[MindGate] Extension context invalidated mid-block:", e.message);
      return;
    }

    window.stop();
    clearTimeout(domScanTimer);
    window.location.replace(blockUrl);
  }
})();