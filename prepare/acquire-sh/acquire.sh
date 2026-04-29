download_protected_file() {
  # Downloads a Cloudflare-protected file with `curl-impersonate`.
  # * https://curl.se/docs/manpage.html
  # * https://docs.docker.com/reference/cli/docker/container/run/
  # * https://github.com/lwthiker/curl-impersonate
  # * https://developer.simprogroup.com/apidoc/swagger.zip
  # * https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Status
  local download_url="$1"
  local downloaded_file="$2"
  local expected_contents="$3"

  local status_code
  status_code=$(
    docker run --rm \
      --volume "$PWD:/data" \
      --workdir /data \
      lwthiker/curl-impersonate:0.6-ff \
      curl_ff109 \
        --silent \
        --show-error \
        --location \
        --output "$downloaded_file" \
        --write-out "%{http_code}" \
        "$download_url"
  )

  [[ "$status_code" == "200" ]] || {
    rm -f "$downloaded_file"
    echo "Download Failed (HTTP $status_code)" >&2
    return 1
  }

  unzip -o "$downloaded_file" >/dev/null
  rm -f "$downloaded_file"

  [[ -f "$expected_contents" ]] || {
    echo "Expected '$expected_contents' not found after unzip" >&2
    return 1
  }
}

download_protected_file \
    "https://developer.simprogroup.com/apidoc/swagger.zip" \
    "swagger.zip" \
    "swagger.json"