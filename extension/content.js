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
    chrome.storage.local.get(["keywords", "subreddits", "paths"], (data) => {
      cachedKeywords = data.keywords || [];
      cachedSubreddits = data.subreddits || [];
      cachedPaths = data.paths || [];
      rulesLoaded = true;
      if (onLoaded) onLoaded();
    });
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

  // Replaces the webpage content with a clean MindGate blocked screen
  // Theme: baby pink / white, matching the product's block-page identity (CONTEXT.md §5)
  function blockPage(reason) {
    window.stop(); // Stop any remaining page assets from loading (no-op on a pure SPA route change, harmless)
    clearTimeout(domScanTimer);

    const blockHtml = `
      <div style="
        position: fixed; top: 0; left: 0; width: 100vw; height: 100vh;
        background: #fff0f5; color: #4a2c3a; font-family: system-ui, sans-serif;
        display: flex; flex-direction: column; align-items: center; justify-content: center;
        z-index: 2147483647; text-align: center; box-sizing: border-box; padding: 20px;
      ">
        <div style="max-width: 500px; background: #ffffff; padding: 40px; border-radius: 16px; border: 1px solid #f7c5d8; box-shadow: 0 10px 25px -5px rgba(244, 114, 182, 0.25);">
          <span style="font-size: 48px;">🌸</span>
          <h1 style="font-size: 24px; margin-top: 16px; margin-bottom: 8px; font-weight: 700; color: #d6336c;">Access Blocked by MindGate</h1>
          <p style="color: #a15b76; font-size: 14px; margin-bottom: 24px; line-height: 1.5;">${reason}</p>
          <div style="height: 1px; background: #f7c5d8; margin-bottom: 24px;"></div>
          <button onclick="window.history.back()" style="
            background: #f472b6; color: white; border: none; padding: 10px 24px;
            font-size: 14px; font-weight: 600; border-radius: 8px; cursor: pointer;
            transition: background 0.2s;
          " onmouseover="this.style.background='#ec4899'" onmouseout="this.style.background='#f472b6'">
            Go Back
          </button>
        </div>
      </div>
    `;

    if (document.body) {
      document.documentElement.innerHTML = blockHtml;
    } else {
      document.addEventListener("DOMContentLoaded", () => {
        document.documentElement.innerHTML = blockHtml;
      });
    }
  }
})();