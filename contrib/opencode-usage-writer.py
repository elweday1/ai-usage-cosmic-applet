#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
#
# opencode-usage-writer.py — feed ai-usage from OpenCode.
#
# OpenCode does not record rate-limit percentages, only per-session cost. This
# sums the cost of opencode-go sessions over a rolling 5 h and 7 d window and
# divides by your plan limits, writing the applet's JSONL format to
# ~/.local/share/opencode/usage-history.jsonl.
#
# Required:
#   OPENCODE_SESSION_LIMIT_USD  budget per 5 h window
#   OPENCODE_WEEKLY_LIMIT_USD   budget per 7 d window
# Optional:
#   OPENCODE_DB                 default ~/.local/share/opencode/opencode.db
#   OPENCODE_USAGE_HISTORY      default ~/.local/share/opencode/usage-history.jsonl
#   OPENCODE_PROVIDER           default opencode-go
#
# Run once, or with --watch to refresh every 60 s.

import json
import os
import sqlite3
import sys
import time

HOME = os.path.expanduser("~")
DB = os.environ.get("OPENCODE_DB", f"{HOME}/.local/share/opencode/opencode.db")
OUT = os.environ.get(
    "OPENCODE_USAGE_HISTORY", f"{HOME}/.local/share/opencode/usage-history.jsonl"
)
PROVIDER = os.environ.get("OPENCODE_PROVIDER", "opencode-go")
SESSION_SECS = 5 * 3600
WEEKLY_SECS = 7 * 86400


def window(conn, now, span):
    """Cost and earliest timestamp (unix s) of sessions updated inside the span."""
    since_ms = (now - span) * 1000
    row = conn.execute(
        """
        SELECT COALESCE(SUM(cost), 0), MIN(time_updated)
        FROM session_v2
        WHERE json_extract(model, '$.providerID') = ? AND time_updated >= ?
        """,
        (PROVIDER, since_ms),
    ).fetchone()
    cost, oldest_ms = row[0] or 0.0, row[1]
    oldest = (oldest_ms // 1000) if oldest_ms else now
    return cost, oldest


def sample():
    session_limit = float(os.environ["OPENCODE_SESSION_LIMIT_USD"])
    weekly_limit = float(os.environ["OPENCODE_WEEKLY_LIMIT_USD"])
    now = int(time.time())
    conn = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    try:
        s_cost, s_oldest = window(conn, now, SESSION_SECS)
        w_cost, w_oldest = window(conn, now, WEEKLY_SECS)
    finally:
        conn.close()
    return {
        "ts": now,
        "session": round(min(s_cost / session_limit, 1.0), 4),
        "weekly": round(min(w_cost / weekly_limit, 1.0), 4),
        "session_reset": s_oldest + SESSION_SECS,
        "weekly_reset": w_oldest + WEEKLY_SECS,
    }


def write_once():
    s = sample()
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    last = None
    try:
        with open(OUT) as f:
            lines = [l for l in f if l.strip()]
        last = json.loads(lines[-1]) if lines else None
    except (OSError, ValueError):
        pass
    # Skip identical samples, but heartbeat every 120 s so the applet stays fresh.
    if last and last["session"] == s["session"] and last["weekly"] == s["weekly"] \
            and s["ts"] - last["ts"] < 120:
        return s
    with open(OUT, "a") as f:
        f.write(json.dumps(s) + "\n")
    return s


if __name__ == "__main__":
    if "--watch" in sys.argv:
        while True:
            write_once()
            time.sleep(60)
    else:
        print(json.dumps(write_once()))
