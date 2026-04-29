#!/usr/bin/env bash

# Creates a personal Postman workspace.
# Outputs a newly-created workspace ID.
postman_create_workspace() {
  local workspace_name="$1"

  jq -n --arg name "$workspace_name" '{
    workspace: {
      name: $name,
      type: "personal",
      description: "Auto-created workspace for Swagger/OpenAPI transformation"
    }
  }' \
  | postman_post "$POSTMAN_URL/workspaces" \
  | jq -r '.workspace.id'
}