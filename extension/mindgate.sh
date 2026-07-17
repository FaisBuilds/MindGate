#!/bin/bash

# Log any errors to a file so we can see them
exec 2>>/tmp/mindgate-bridge-error.log

# Run the binary with environment variables.
# Notice we DO NOT pass "$@" to the subcommand anymore! This ignores Chrome's extra arguments.
MINDGATE_SOCKET="/tmp/mindgate-dev/mindgate.sock" \
MINDGATE_CONFIG_DIR="/tmp/mindgate-dev" \
MINDGATE_RUN_DIR="/tmp/mindgate-dev" \
exec /home/faisal/Desktop/MindGate/target/debug/mindgate native-bridge