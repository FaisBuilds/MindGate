sudo MINDGATE_SOCKET=/tmp/mindgate-dev/mindgate.sock MINDGATE_CONFIG_DIR=/tmp/mindgate-dev MINDGATE_RUN_DIR=/tmp/mindgate-dev target/debug/mindgated

sudo nft flush ruleset && sudo systemctl restart NetworkManager
sudo nft flush ruleset && sudo rm -f /tmp/mindgate-dev/mindgate.sock