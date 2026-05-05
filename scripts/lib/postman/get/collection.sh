#!/usr/bin/env bash

# Looks up a Postman collection by name.
# Returns the collection UID if found.
postman_get_collection_uid() {
  local collection_name="$1"

  postman_get "$POSTMAN_URL/collections" \
  | jq -r --arg name "$collection_name" '
      (.collections // [])
      | map(select(.name == $name))
      | .[0].uid // empty
    '
}