# Demo GIF script for https://github.com/charmbracelet/vhs
#
#   vhs docs/assets/demo.tape    →  docs/assets/demo.gif
#
# ⚠ PRIVACY: this records your real terminal output. Run it against a staged
# demo store (--store /tmp/void-demo with seeded fake data), or carefully
# review every frame before publishing — `void inbox` shows real messages.

Output docs/assets/demo.gif

Set FontSize 16
Set Width 1200
Set Height 640
Set Theme "Catppuccin Mocha"
Set TypingSpeed 60ms

Type "void inbox"
Enter
Sleep 3s

Type "void search 'quarterly report'"
Enter
Sleep 3s

Type "void reply 42 --message 'On it — sending today.'"
Enter
Sleep 2.5s

Type "void archive 42"
Enter
Sleep 2s

Type "void inbox"
Enter
Sleep 3.5s
