#!/usr/bin/env bash
# A fake agent that goes fully SILENT while working — no dots, no output at
# all, unlike mock-agent.sh — simulating a tool-call/build step with zero
# intermediate output. It also prints no distinctive ready-prompt, unlike
# mock-agent.sh's "you> ". Exercises two conformance cases real, arbitrary
# PTY CLIs can have: an agent with no configurable --ready marker, and a
# busy period where idle can't be inferred from a mid-line tail because
# there's no new output at all to look at.
#
#   cargo run -- --interrupt ctrl-c --min-busy-ms 4000 -- bash examples/silent-agent.sh
set -u

work() {
  local n=${1:-15} # 15 * 0.2s ~= 3s of silent work
  trap 'echo "(interrupted mid-work)"; return 130' INT
  for ((i = 0; i < n; i++)); do
    sleep 0.2
  done
  trap - INT
}

echo "silent-agent ready."
while IFS= read -r line; do
  [ -z "$line" ] && continue
  if work 15; then
    echo "done: $line"
  fi
done
echo "silent-agent: input closed, bye."
