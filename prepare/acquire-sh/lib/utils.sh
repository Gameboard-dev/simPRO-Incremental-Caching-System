#!/usr/bin/env bash

# Ensures a required command is available on PATH.
require() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Missing dependency: $1" >&2
    exit 1
  }
}

# Determines whether a timestamp is older than a specified number of days.
#
# Args:
#   $1 - Timestamp string e.g. "ISO 8601: 2024-01-01T12:00:00Z"
#   $2 - Maximum allowed age in days.
#
# Return:
#   0 (true)  - Timestamp is stale
#   1 (false) - Timestamp is fresh
#
is_stale() {
  local timestamp="$1"
  local max_days="$2"

  [[ -z "$timestamp" || "$timestamp" == "null" ]] && return 0

  local now ts
  now="$(date -u +%s)"
  ts="$(date -u -d "$timestamp" +%s 2>/dev/null)" || return 0

  (( (now - ts) > max_days * 86400 ))
}