#!/usr/bin/env bash
# An agent that exits unexpectedly (like a crash) partway through its first
# turn, for testing delphin's handling of prompts still queued when the
# wrapped process disappears. Prints a ready marker so idle detection for the
# first prompt is fast/deterministic; reads exactly one line, then dies —
# anything queued after that first prompt is sent stays queued forever.
set -u

echo "crashy-agent ready."
printf 'you> '
read -r line
echo "processing: $line"
sleep 0.3
exit 1
