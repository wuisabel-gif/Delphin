#!/usr/bin/env bash
# A fake "thinking" AI agent, for exercising Delphin without a real model.
#
# Reads a line, "thinks" for ~4s (printing dots), then answers. The think phase
# traps SIGINT, so it is interruptible — test barge-in with:
#   cargo run -- --interrupt ctrl-c -- bash examples/mock-agent.sh
set -u

think() {
  local n=${1:-20}
  trap 'echo; echo "  (interrupted — dropping this thought)"; return 130' INT
  for ((i = 0; i < n; i++)); do
    printf '.'
    sleep 0.2
  done
  trap - INT
  echo
}

echo "mock-agent ready. ask me anything."
printf 'you> '
while IFS= read -r line; do
  if [ -z "$line" ]; then
    printf 'you> '
    continue
  fi
  printf 'thinking about: %s ' "$line"
  if think 20; then
    echo "answer: I considered \"$line\" and here is a verbatim-ish reply."
  fi
  printf 'you> '
done
echo
echo "mock-agent: input closed, bye."
