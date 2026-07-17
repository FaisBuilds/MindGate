const HOST_NAME = "com.mindgate.protector";
let ruleFetchInterval = null;

function fetchRulesFromDaemon() {
  console.log("[MindGate background] Requesting rules from native host...");

  chrome.runtime.sendNativeMessage(
    HOST_NAME,
    { cmd: "List" }, // was "Status" — that only returns counts, not rule contents
    (response) => {
      if (chrome.runtime.lastError) {
        console.warn("[MindGate background] Bridge error:", chrome.runtime.lastError.message);
        return;
      }

      console.log("[MindGate background] Raw payload received:", JSON.stringify(response));

      // Response::Rules(RuleSet) encodes as { result: "Rules", data: { websites, keywords, subreddits } }
      if (response?.result === "Rules" && response?.data) {
        const keywords = (response.data.keywords || []).map((k) => k.value);
        const subreddits = (response.data.subreddits || []).map((s) => s.subreddit);

        console.log("[MindGate background] Extracted keywords:", keywords, "subreddits:", subreddits);

        chrome.storage.local.set({ keywords, subreddits }, () => {
          if (chrome.runtime.lastError) {
            console.error("[MindGate background] Storage error:", chrome.runtime.lastError.message);
          } else {
            console.log("[MindGate background] Rules saved to local storage.");
          }
        });
      } else if (response?.result === "Error") {
        console.error("[MindGate background] Daemon returned an error:", response.data?.message);
      } else {
        console.error(
          "[MindGate background] Unexpected payload format. Expected result === 'Rules'.",
          "Received payload:", JSON.stringify(response)
        );
      }
    }
  );
}

chrome.runtime.onInstalled.addListener(() => {
  fetchRulesFromDaemon();
  if (ruleFetchInterval) clearInterval(ruleFetchInterval);
  ruleFetchInterval = setInterval(fetchRulesFromDaemon, 5000);
});

chrome.runtime.onStartup.addListener(() => {
  fetchRulesFromDaemon();
  ruleFetchInterval = setInterval(fetchRulesFromDaemon, 5000);
});