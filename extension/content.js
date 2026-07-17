(function () {
  // Read synchronized rules from background storage
  chrome.storage.local.get(["keywords", "subreddits"], (data) => {
    const keywords = data.keywords || [];
    const subreddits = data.subreddits || [];

    if (keywords.length === 0 && subreddits.length === 0) return;

    const currentUrl = window.location.href.toLowerCase();

    // 1. Check subreddit blocks (e.g., blocking "r/gaming" or "/r/gaming/")
    for (const sub of subreddits) {
      // Normalize rule syntax (e.g. "r/gaming" or "gaming")
      let cleanSub = sub.toLowerCase();
      if (cleanSub.startsWith("r/")) {
        cleanSub = cleanSub.substring(2);
      }

      const subredditPattern = new RegExp(`(/r/|/reddit.com/r/)${cleanSub}(\\b|/|\\?)`, 'i');
      if (subredditPattern.test(currentUrl)) {
        blockPage(`Subreddit block matched: r/${cleanSub}`);
        return;
      }
    }

    // 2. Check keyword blocks against the URL
    for (const keyword of keywords) {
      const cleanKeyword = keyword.toLowerCase();
      if (currentUrl.includes(cleanKeyword)) {
        blockPage(`URL Keyword block matched: "${keyword}"`);
        return;
      }
    }

    // 3. Dynamic Keyword scanning (DOM text content matching on load)
    document.addEventListener("DOMContentLoaded", () => {
      const pageText = document.body.innerText.toLowerCase();
      for (const keyword of keywords) {
        if (pageText.includes(keyword.toLowerCase())) {
          blockPage(`Page Content Keyword block matched: "${keyword}"`);
          return;
        }
      }
    });
  });

  // Replaces the webpage content with a clean MindGate blocked screen
  function blockPage(reason) {
    window.stop(); // Stop any remaining page assets from loading
    
    const blockHtml = `
      <div style="
        position: fixed; top: 0; left: 0; width: 100vw; height: 100vh;
        background: #0f172a; color: #f8fafc; font-family: system-ui, sans-serif;
        display: flex; flex-direction: column; align-items: center; justify-content: center;
        z-index: 2147483647; text-align: center; box-sizing: border-box; padding: 20px;
      ">
        <div style="max-width: 500px; background: #1e293b; padding: 40px; border-radius: 12px; border: 1px solid #334155; box-shadow: 0 10px 15px -3px rgba(0, 0, 0, 0.3);">
          <span style="font-size: 48px;">🛡️</span>
          <h1 style="font-size: 24px; margin-top: 16px; margin-bottom: 8px; font-weight: 700;">Access Blocked by MindGate</h1>
          <p style="color: #94a3b8; font-size: 14px; margin-bottom: 24px; line-height: 1.5;">${reason}</p>
          <div style="height: 1px; background: #334155; margin-bottom: 24px;"></div>
          <button onclick="window.history.back()" style="
            background: #3b82f6; color: white; border: none; padding: 10px 20px;
            font-size: 14px; font-weight: 600; border-radius: 6px; cursor: pointer;
            transition: background 0.2s;
          " onmouseover="this.style.background='#2563eb'" onmouseout="this.style.background='#3b82f6'">
            Go Back
          </button>
        </div>
      </div>
    `;

    // Wait until document.body is accessible to inject the block screen
    if (document.body) {
      document.documentElement.innerHTML = blockHtml;
    } else {
      document.addEventListener("DOMContentLoaded", () => {
        document.documentElement.innerHTML = blockHtml;
      });
    }
  }
})();