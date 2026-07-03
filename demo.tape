# Delphin demo — render with:  vhs demo.tape   (produces demo.gif)
# Build the binary and wipe the demo DB BEFORE running (see the render command),
# so the tape can start on a clean prompt without a hidden setup block.

Output demo.gif

Set Shell "bash"
Set FontSize 22
Set Width 1400
Set Height 760
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 55ms

Sleep 600ms

# wrap the (mock) agent
Type "./target/release/delphin --interrupt ctrl-c --ready 'you> ' --db /tmp/delphin-demo.sqlite3 -- bash examples/mock-agent.sh"
Enter
Sleep 4s

# first task — sent immediately, the agent starts thinking
Type "add a login endpoint"
Enter
Sleep 1800ms

# keep typing while it thinks — these QUEUE
Type "also add rate limiting"
Enter
Sleep 1600ms

Type "and write tests"
Enter
Sleep 1600ms

# an urgency word BARGES IN
Type "stop, use POST not GET"
Enter
Sleep 3500ms

# quit, clear the shutdown noise, then show it remembered everything
Ctrl+D
Sleep 1s
Type "clear"
Enter
Sleep 300ms
Type "./target/release/delphin recall --db /tmp/delphin-demo.sqlite3"
Enter
Sleep 3s
