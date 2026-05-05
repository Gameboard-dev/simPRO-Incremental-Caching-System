#!/usr/bin/env bash

# Base URL for Postman API.
POSTMAN_URL="https://api.getpostman.com"

# Sends GET request to Postman API.
postman_get() {
  local url="$1"

  curl --silent --show-error \
    --request GET \
    --header "X-Api-Key: $POSTMAN_API_KEY" \
    "$url"
}

# Sends POST request to Postman API.
# * JSON is read from stdin so callers can pipe jq output 
#   directly into this function.
postman_post() {
  local url="$1"

  curl --silent --show-error \
    --request POST \
    --header "X-Api-Key: $POSTMAN_API_KEY" \
    --header "Content-Type: application/json" \
    --data-binary @- \
    "$url"
}