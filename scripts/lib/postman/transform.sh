#!/usr/bin/env bash

# Exports a Postman collection transformation as OpenAPI JSON.
#
# Postman stores generated transformations for a collection.
# This function retrieves the transformation output 
# and writes the OpenAPI JSON to disk.
#
# Arguments:
#   $1 - Postman collection UID.
#   $2 - Output file path, for example openapi.json.
#
# Link:
# * https://blog.postman.com/creating-an-openapi-definition-from-a-collection-with-the-postman-api/
#
postman_transform_collection() {
  local collection_uid="$1"
  local output_file="$2"

  postman_get "$POSTMAN_URL/collections/$collection_uid/transformations" \
  | jq -r '.output | fromjson' \
  > "$output_file"
}