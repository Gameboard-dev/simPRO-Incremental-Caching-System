#!/usr/bin/env bash

# Loads environment variables from .env.
#
# Required:
#   POSTMAN_API_KEY

ENV_FILE="$SCRIPT_DIR/.env"

if [[ -f "$ENV_FILE" ]]; then
  set -a
  source "$ENV_FILE"
  set +a
else
  echo "Missing $ENV_FILE" >&2
  exit 1
fi

[[ -n "${POSTMAN_API_KEY:-}" ]] || {
  echo "POSTMAN_API_KEY NOT SET" >&2
  exit 1
}