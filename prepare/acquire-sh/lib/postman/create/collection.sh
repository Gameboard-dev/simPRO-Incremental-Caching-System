#!/usr/bin/env bash

# Imports a Swagger/OpenAPI JSON file into Postman as a collection.
#
# Arguments:
#   $1 - Swagger/OpenAPI JSON file path.
#   $2 - Collection name to assign inside Postman.
#   $3 - Postman workspace ID.
#
# Outputs:
#   Created collection UID.
postman_create_collection() {
  local swagger_file="$1"
  local collection_name="$2"
  local workspace_id="$3"

  local response
  response="$(
    jq -n \
      --arg name "$collection_name" \
      --slurpfile file "$swagger_file" \
      '{type:"json", input: ($file[0] | .info.title=$name)}' \
    | postman_post "$POSTMAN_URL/import/openapi?workspace=$workspace_id"
  )"

  local collection_uid
  collection_uid="$(jq -r '.collections[]?.uid // empty' <<<"$response")"

  [[ -n "$collection_uid" ]] || {
    echo "Collection creation failed:" >&2
    jq . <<<"$response" >&2
    exit 1
  }

  echo "$collection_uid"
}