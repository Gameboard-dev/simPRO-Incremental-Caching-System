

### Windows Dependencies

  WSL2:
  https://learn.microsoft.com/en-us/windows/wsl/install
  https://apps.microsoft.com/detail/9pdxgncfsczv?hl=en-US&gl=GB

  Docker:
  https://docs.docker.com/engine/install/ubuntu/

---

### API Preprocessing

```sh
# Open the current directory in Ubuntu (WSL2)
wsl -d Ubuntu

# Acquire and unzip the official simPRO `swagger.json` (API)
bash scripts/acquire.sh

# Transform into OpenAPIv3 compliant format using Postman
bash scripts/transform.sh

# Remove .tags .summary .example .examples keys using `jq`
bash scripts/trim.sh

# Deduplicate inline parameters and schemas into `components/`
cargo run -p xtask -- dedupe-openapi openapi.json openapi.deduped.yaml
```

### OpenAPI Validation

https://openapi-generator.tech/docs/usage/

```bash
docker run --rm \
  -v "$PWD:/local" \
  openapitools/openapi-generator-cli validate \
  -i /local/openapi.deduped.yaml
```

### SQL (Database Accessor) Synchronization

The project can regenerate diesel database accessors in `src/db.rs` 
whenever the schema (`init.sql`) changes.

Make sure these commands run in an environment with Docker available.
To enter WSL2 Ubuntu to run these commands on Windows use `wsl -d Ubuntu`.

```sh
cargo install cargo-watch
```

```sh
cargo watch --poll --delay 1 -w ./init.sql -x "run -p xtask -- generate-schema"
```
---

### API (Client Wrapper) Synchronization

The project can automatically regenerate the API client wrapper `src/api.rs` 
whenever the specification (`openapi.yaml`) changes.

```sh
cargo build
```

### PostgreSQL Binaries

Diesel’s PostgreSQL backend depends on the native PostgreSQL client library (libpq).

In Docker environments this is handled automatically, but on Windows you need to install PostgreSQL binaries and expose them on LIB and PATH.

### Windows

Binaries can be obtained from:
https://www.enterprisedb.com/download-postgresql-binaries

Then add the following environment variables to your path:

```sh
$env:LIB="<PATH_TO>\lib;$env:LIB"
$env:PATH="<PATH_TO>\bin;$env:PATH"
```

### Windows WSL2 Ubuntu

In Ubuntu WSL on Windows run the following command to install PostgreSQL's client library, `libpq`:

```bash
sudo apt install libpq-dev pkg-config build-essential
```

## Environment Variables

#### SIMPRO_API_KEY

API key used to authenticate requests to the simPRO API.

Create an API application and generate an API key from the simPRO API settings:

[simPRO Authentication Documentation](https://developer.simprogroup.com/apidoc/?page=3366d2ea7906f693b27d57ed9cca3acb#tag/How-to-authenticate)

#### SIMPRO_DOMAIN

Your simPRO tenant domain. Example:

```yaml
SIMPRO_DOMAIN=grainconnect.simprosuite.com
```

#### SIMPRO_WEBHOOK_SECRET

Shared secret used to verify webhook signatures from simPRO.

This value must match the webhook subscription secret configured in simPRO webhook settings.

[simPRO Webhook Documentation](https://developer.simprogroup.com/apidoc/?page=cd8682773ab1b07fdc9661984e281ce3#tag/Web-Hooks)


