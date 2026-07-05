# delphin replay demo — render with:  vhs demo-replay.tape  (produces demo-replay.gif)
# Shows the replay harness: re-run a different arbiter over real history and
# see where it would have decided differently. Build the release binary and
# clear /tmp/delphin-replay-demo.sqlite3* before rendering.

Output demo-replay.gif

Set Shell "bash"
Set FontSize 22
Set Width 1400
Set Height 760
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 55ms
Set PlaybackSpeed 1

Sleep 600ms

# short alias so the typed commands stay brief
Type "alias d=./target/release/delphin"
Enter
Sleep 300ms
Type "clear"
Enter
Sleep 300ms

# Record a short real session under the default (heuristic) arbiter.
Type "d --ready 'you> ' --db /tmp/delphin-replay-demo.sqlite3 -- bash examples/mock-agent.sh"
Enter
Sleep 3s

Type "add a login endpoint"
Enter
Sleep 2s

# a QUESTION, asked while busy — the heuristic just queues it (not urgent)
Type "what database are we using?"
Enter
Sleep 5s

Ctrl+D
Sleep 1500ms
Type "clear"
Enter
Sleep 500ms

# Replay with a DIFFERENT arbiter — the question arbiter would have
# interrupted for that question instead of queueing it. Disagreement surfaces.
# This is the one result that matters, so it goes first (not last).
Type "d replay --db /tmp/delphin-replay-demo.sqlite3 --arbiter question"
Enter
Sleep 6s
