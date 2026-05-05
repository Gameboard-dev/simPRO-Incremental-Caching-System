#!/usr/bin/env bash

INPUT_FILE="openapi.json"
TMP_FILE="$(mktemp)"

if ! command -v jq &> /dev/null; then
  echo "jq is not installed."
  exit 1
fi

jq 'del(.. | .tags?, .summary?, .example?, .examples?)' "$INPUT_FILE" > "$TMP_FILE" \
  && mv "$TMP_FILE" "$INPUT_FILE"

echo "Trimmed File: $INPUT_FILE"