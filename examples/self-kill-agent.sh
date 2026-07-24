#!/usr/bin/env bash
# Terminates itself with SIGKILL mid-turn — no graceful shutdown at all,
# simulating an abrupt "link loss" (the PTY's other end just vanishes)
# rather than crashy-agent.sh's clean `exit 1`. Reads exactly one line.
set -u

echo "self-kill-agent ready."
printf 'you> '
read -r line
echo "processing: $line"
# Leave enough time for the conformance test to enqueue a second prompt even
# on a loaded runner before simulating the abrupt link loss.
sleep 0.8
kill -9 $$
