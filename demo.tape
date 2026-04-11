# VHS demo — https://github.com/charmbracelet/vhs
# Run: vhs demo.tape

Output demo.gif

Set Shell "bash"
Set FontSize 14
Set Width 1200
Set Height 700
Set Padding 10
Set TypingSpeed 150ms

Type "kingi"
Enter
Sleep 5s

# Navigate emails
Sleep 3s
Type "j"
Sleep 2s
Type "j"
Sleep 2s
Type "j"
Sleep 1.5s

# Open an email
Sleep 3s
Enter
Sleep 5s

# Scroll through it
Type "j"
Sleep 1.5s
Type "j"
Sleep 1.5s
Type "j"
Sleep 1.5s
Type "j"
Sleep 1.5s
Type "j"
Sleep 4s

# Reply with quote
Type "R"
Sleep 5s

# Tab to body field (Subject -> Cc -> Body)
Tab
Sleep 2s
Tab
Sleep 2s
Tab
Sleep 2.5s

# Type a reply
Type "O"
Sleep 1s
Type "Thank you for your email!"
Sleep 4s

# Open send dialog and discard
Ctrl+q
Sleep 3s
Type "j"
Sleep 2s
Type "j"
Sleep 2s
Enter
Sleep 4s

# Close tab
Type "q"
Sleep 4s

# Switch mailbox
Type "J"
Sleep 3s
Type "J"
Sleep 3s
Type "K"
Sleep 4s

# Search
Type "/"
Sleep 2s
Type "e"
Sleep 1.5s
Type "p"
Sleep 1.5s
Type "o"
Sleep 1.5s
Type "l"
Sleep 1.5s
Type "l"
Sleep 4s
Escape
Sleep 3s

# Toggle sort order
Type "s"
Sleep 3s
Type "s"
Sleep 4s

# Quit
Type "Q"
Sleep 2s
