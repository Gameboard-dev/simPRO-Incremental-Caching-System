#!/usr/bin/env bash

# Looks up a Postman workspace by name
# and returns its ID if found.
postman_get_workspace_id() {
  local workspace_name="$1"

  postman_get "$POSTMAN_URL/workspaces" \
  | jq -r --arg name "$workspace_name" '
      (.workspaces // [])
      | map(select(.name == $name))
      | .[0].id // empty
    '
}