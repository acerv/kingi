#!/usr/bin/env python3
"""Antigravity PreToolUse safety guard for run_command in kingi."""

import json
import re
import sys

BLOCKED_PATTERNS = [
    (
        re.compile(r"\bgit\s+push\b.*(\s--force\b|\s-f\b|--force-with-lease\b)"),
        "Force-pushing is blocked by repository policy.",
    ),
    (
        re.compile(r"\bgit\s+push\b.*\borigin\s+master\b"),
        "Direct pushes to master are blocked; use a feature branch and PR.",
    ),
    (
        re.compile(r"\bgit\s+reset\s+--hard\b"),
        "git reset --hard is blocked to prevent discarding uncommitted agent work.",
    ),
    (
        re.compile(r"\brm\s+-[a-zA-Z]*r[a-zA-Z]*f[a-zA-Z]*\s+(/|~|\$HOME)\b"),
        "Destructive recursive deletion targeting root or home directory is blocked.",
    ),
]


def main() -> int:
    try:
        payload = json.load(sys.stdin)
    except Exception:
        print(json.dumps({"decision": "ask"}))
        return 0

    tool_call = payload.get("toolCall") or {}
    args = tool_call.get("args") or {}
    cmd = str(args.get("CommandLine") or "")

    for pattern, reason in BLOCKED_PATTERNS:
        if pattern.search(cmd):
            print(json.dumps({"decision": "deny", "reason": reason}))
            return 0

    print(json.dumps({"decision": "ask"}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
