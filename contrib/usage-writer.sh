#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
#
# usage-writer.sh — feed ai-usage.
#
# The applet only *reads* ~/.claude/usage-history.jsonl; something has to
# *write* it. Claude Code exposes the 5-hour and 7-day rate-limit figures only
# on the JSON it pipes to a statusLine command's stdin, so this script is a
# statusLine that persists those figures for the applet.
#
# Two ways to use it (see the project README, "Feeding the applet"):
#
#   1. No statusLine yet — point Claude Code straight at this script:
#        // ~/.claude/settings.json
#        "statusLine": { "type": "command",
#                        "command": "bash /path/to/usage-writer.sh" }
#      It prints a compact "session N% · weekly N%" line for the terminal.
#
#   2. Already have a statusLine — either paste the "persist" block below into
#      your own script, or tee the render JSON through this one:
#        input=$(cat)
#        printf '%s' "$input" | bash /path/to/usage-writer.sh >/dev/null
#        printf '%s' "$input" | your-existing-statusline
#
# LC_ALL=C: some locales use a comma decimal separator, which makes awk/printf
# mishandle "0.42". Force C so the numbers are well-formed JSON.
export LC_ALL=C

HISTORY="${CLAUDE_USAGE_HISTORY:-$HOME/.claude/usage-history.jsonl}"
HEARTBEAT="${CLAUDE_USAGE_HEARTBEAT:-120}"   # re-write an unchanged sample after N seconds

input=$(cat)

# rate_limits appears only for Claude.ai (Pro/Max) accounts, and only after the
# first API response of a session; absent otherwise -> nothing to persist.
sess=$(printf '%s' "$input" | jq -r '.rate_limits.five_hour.used_percentage // empty')
week=$(printf '%s' "$input" | jq -r '.rate_limits.seven_day.used_percentage // empty')

if [ -n "$sess" ] && [ -n "$week" ]; then
  # session/weekly are stored as 0-1 fractions; %g drops trailing zeros so the
  # lines match the applet's fixture convention (0.45, not 0.4500).
  ns=$(awk "BEGIN{printf \"%g\", $sess/100}")
  nw=$(awk "BEGIN{printf \"%g\", $week/100}")
  now_ts=$(date +%s)

  # Renders fire in bursts. Skip a sample identical to the last recorded one,
  # but always write at least every $HEARTBEAT seconds so the applet's
  # staleness clock stays current.
  last=$(tail -1 "$HISTORY" 2>/dev/null)
  last_sw=$(printf '%s' "$last" | jq -r '"\(.session) \(.weekly)"' 2>/dev/null)
  last_ts=$(printf '%s' "$last" | jq -r '.ts // 0' 2>/dev/null | cut -d. -f1)

  if [ "$last_sw" != "$ns $nw" ] || [ $(( now_ts - ${last_ts:-0} )) -ge "$HEARTBEAT" ]; then
    # resets_at is unix seconds today, but tolerate an ISO-8601 string too.
    _norm_ts() { case "$1" in ''|null) echo 0;; *[!0-9]*) date -d "$1" +%s 2>/dev/null || echo 0;; *) echo "$1";; esac; }
    s_reset=$(printf '%s' "$input" | jq -r '.rate_limits.five_hour.resets_at // empty')
    w_reset=$(printf '%s' "$input" | jq -r '.rate_limits.seven_day.resets_at // empty')
    mkdir -p "$(dirname "$HISTORY")"
    printf '{"ts": %s, "session": %s, "weekly": %s, "session_reset": %s, "weekly_reset": %s}\n' \
      "$now_ts" "$ns" "$nw" "$(_norm_ts "$s_reset")" "$(_norm_ts "$w_reset")" >> "$HISTORY"
  fi

  # Compact status line for standalone use (harmless when piped to /dev/null).
  printf 'session %s%% · weekly %s%%\n' "$(printf '%.0f' "$sess")" "$(printf '%.0f' "$week")"
fi
