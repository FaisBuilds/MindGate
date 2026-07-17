MindGate — Dev Testing Guide



Terminal 1 — the daemon (start once, leave running)

bashcd ~/Desktop/MindGate
source mindgate-env.sh
sudo -E target/debug/mindgated


sudo is required — nftables changes need root.
-E is required — without it, sudo wipes the env vars you just
exported, and you're back to the default-paths problem above.




Terminal 2 — CLI commands (separate window/tab)

bashcd ~/Desktop/MindGate
source mindgate-env.sh

Then any of:

bashsudo -E target/debug/mindgate add example.com          # block a website (network-wide)
sudo -E target/debug/mindgate remove example.com        # unblock it
sudo -E target/debug/mindgate add-keyword pizza          # block a keyword (browser layer)
sudo -E target/debug/mindgate remove-keyword pizza
sudo -E target/debug/mindgate add-subreddit gonewild      # block a subreddit (browser layer)
sudo -E target/debug/mindgate remove-subreddit gonewild
sudo -E target/debug/mindgate list                        # show current rules
sudo -E target/debug/mindgate status                      # daemon health + extension connection


RESET DNS (If internet stops working):
sudo nft flush ruleset && sudo systemctl restart NetworkManager


