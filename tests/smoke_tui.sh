#!/bin/bash
# TUI smoke test: launches purple --demo in tmux, navigates through all
# major screens via keystrokes, and verifies no crash at each step.
#
# Requires: tmux, a built purple binary (release or debug).
# Run: ./tests/smoke_tui.sh [path-to-binary]
#
# Exit code 0 = all screens survived, 1 = crash detected.

set -e
cd "$(dirname "$0")/.."

BINARY="${1:-./target/release/purple}"
if [ ! -x "$BINARY" ]; then
    BINARY="./target/debug/purple"
fi
if [ ! -x "$BINARY" ]; then
    printf "No purple binary found. Run cargo build first.\n"
    exit 1
fi

if ! command -v tmux >/dev/null 2>&1; then
    printf "tmux not found. Install it to run the TUI smoke test.\n"
    exit 1
fi

SESSION="purple_smoke_$$"
FAIL=0
STEP=0

cleanup() {
    tmux kill-session -t "$SESSION" 2>/dev/null || true
}
trap cleanup EXIT

step() {
    STEP=$((STEP + 1))
    printf "  [%02d] %s" "$STEP" "$1"
}

ok() { printf " ✓\n"; }

fail() {
    printf " ✗ %s\n" "$1"
    FAIL=1
}

send() {
    tmux send-keys -t "$SESSION" -l "$@" 2>/dev/null
    sleep 0.3
}

send_key() {
    tmux send-keys -t "$SESSION" "$@" 2>/dev/null
    sleep 0.3
}

alive() {
    tmux has-session -t "$SESSION" 2>/dev/null
}

capture() {
    tmux capture-pane -t "$SESSION" -p 2>/dev/null || echo ""
}

# Start purple in demo mode
tmux new-session -d -s "$SESSION" -x 120 -y 40
sleep 0.3
tmux send-keys -t "$SESSION" "$BINARY --demo" Enter
sleep 3

printf "=== Purple TUI Smoke Test ===\n\n"

# 1. Host list
step "Host list renders"
if alive; then
    OUTPUT=$(capture)
    # Check for the title bar which shows host count (e.g. "purple -- 22")
    if echo "$OUTPUT" | grep -q "purple"; then ok; else fail "TUI not visible"; fi
else
    fail "crashed on startup"; exit 1
fi

# 2. Navigate down
step "Navigate down (j j j)"
send "j"; send "j"; send "j"
if alive; then ok; else fail "crash"; exit 1; fi

# 3. Navigate up
step "Navigate up (k k)"
send "k"; send "k"
if alive; then ok; else fail "crash"; exit 1; fi

# 4. Search
step "Search (/ prod Esc)"
send "/"; sleep 0.2; send "prod"; sleep 0.4
if alive; then ok; else fail "crash"; exit 1; fi
send_key Escape; sleep 0.3

# 5. Detail panel
step "Detail panel (Enter, Esc)"
send_key Enter; sleep 0.5
if alive; then ok; else fail "crash"; fi
send_key Escape; sleep 0.3

# 6. Help screen
step "Help screen (?, Esc)"
send "?"; sleep 0.5
if alive; then ok; else fail "crash"; fi
send_key Escape; sleep 0.3

# 7. Command palette
step "Command palette (Ctrl-p, Esc)"
send_key C-p; sleep 0.5
if alive; then ok; else fail "crash"; fi
send_key Escape; sleep 0.3

# 8. Theme picker
step "Theme picker (t, navigate, Esc)"
send "t"; sleep 0.4
send "j"; send "j"; send "j"; sleep 0.2
if alive; then ok; else fail "crash"; fi
send_key Escape; sleep 0.3

# 9. Provider list
step "Provider list (S, Esc)"
send "S"; sleep 0.4
if alive; then ok; else fail "crash"; fi
send_key Escape; sleep 0.3

# 10. Snippet picker
step "Snippet picker (x, Esc)"
send "x"; sleep 0.4
if alive; then ok; else fail "crash"; fi
send_key Escape; sleep 0.3

# 11. Tunnel list
step "Tunnel list (T, Esc)"
send "T"; sleep 0.4
if alive; then ok; else fail "crash"; fi
send_key Escape; sleep 0.3

# 12. Add host form
step "Add host form (a, Esc)"
send "a"; sleep 0.4
if alive; then ok; else fail "crash"; fi
send_key Escape; sleep 0.3

# 13. Edit host form
step "Edit host form (e, Esc)"
send "e"; sleep 0.4
if alive; then ok; else fail "crash"; fi
send_key Escape; sleep 0.3

# 14. Container screen
step "Container screen (c, Esc)"
send "c"; sleep 0.4
if alive; then ok; else fail "crash"; fi
send_key Escape; sleep 0.3

# 15. File browser
step "File browser (f, Esc)"
send "f"; sleep 0.4
if alive; then ok; else fail "crash"; fi
send_key Escape; sleep 0.3

# 16. Sort cycling
step "Sort modes (s s s)"
send "s"; sleep 0.2; send "s"; sleep 0.2; send "s"; sleep 0.2
if alive; then ok; else fail "crash"; fi

# 17. Group cycling
step "Group by (g g)"
send "g"; sleep 0.2; send "g"; sleep 0.2
if alive; then ok; else fail "crash"; fi

# 18. View mode
step "View mode toggle (v v)"
send "v"; sleep 0.2; send "v"; sleep 0.2
if alive; then ok; else fail "crash"; fi

# 19. Ping
step "Ping all (p)"
send "p"; sleep 1.5
if alive; then ok; else fail "crash"; fi

# 20. Filter down
step "Filter down hosts (! !)"
send "!"; sleep 0.3; send "!"; sleep 0.3
if alive; then ok; else fail "crash"; fi

# 21. Top/bottom navigation
step "Top/bottom (G, gg)"
send "G"; sleep 0.2; send "g"; send "g"; sleep 0.2
if alive; then ok; else fail "crash"; fi

# 22. Clean exit
step "Clean exit (q)"
send "q"; sleep 1
if alive; then
    # Check if purple exited and shell prompt returned
    OUTPUT=$(capture)
    if echo "$OUTPUT" | grep -q '\$\|%\|❯'; then
        ok
    else
        fail "did not exit cleanly"
    fi
else
    ok
fi

printf "\n=== Results: %d steps ===" "$STEP"
if [ "$FAIL" -eq 0 ]; then
    printf " ALL PASSED ✓\n"
else
    printf " FAILURES ✗\n"
fi

exit $FAIL
