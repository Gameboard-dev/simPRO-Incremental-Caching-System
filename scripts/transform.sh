#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

source "$SCRIPT_DIR/lib/env.sh"
source "$SCRIPT_DIR/lib/utils.sh"
source "$SCRIPT_DIR/lib/postman/http.sh"
source "$SCRIPT_DIR/lib/postman/get/workspace.sh"
source "$SCRIPT_DIR/lib/postman/get/collection.sh"
source "$SCRIPT_DIR/lib/postman/create/workspace.sh"
source "$SCRIPT_DIR/lib/postman/create/collection.sh"
source "$SCRIPT_DIR/lib/postman/transform.sh"

require curl
require jq

main() {
  # Transform the Swagger 2.0 Specification File
  # into OpenAPIv3 using Postman Collection Transform
  # This requires a valid POSTMAN_API_KEY in .env

  local swagger_file="swagger.json"
  local openapi_file="openapi.json"
  local workspace_name="simPRO_API_Workspace"
  local collection_name="simPRO_API_Collection"

  [[ -f "$swagger_file" ]] || {
    echo "Missing $swagger_file" >&2
    exit 1
  }

  [[ ! -f "$openapi_file" ]] || {
    echo "$openapi_file already exists."
    return 0
  }

  echo "Using $POSTMAN_API_KEY"

  local workspace_id="$(postman_get_workspace_id "$workspace_name")"
  if [[ -z "$workspace_id" ]]; then
    workspace_id="$(postman_create_workspace "$workspace_name")"
  fi

  local collection_uid="$(postman_get_collection_uid "$collection_name")"
  if [[ -z "$collection_uid" ]]; then
    collection_uid="$(
      postman_create_collection \
        "$swagger_file" \
        "$collection_name" \
        "$workspace_id"
    )"
  fi

  postman_transform_collection \
    "$collection_uid" \
    "$openapi_file"

  echo "OpenAPI written to $openapi_file"
}

main "$@"