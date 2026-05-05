

### Windows Dependencies

  WSL2:
  https://learn.microsoft.com/en-us/windows/wsl/install
  https://apps.microsoft.com/detail/9pdxgncfsczv?hl=en-US&gl=GB

  Docker:
  https://docs.docker.com/engine/install/ubuntu/

---

### API Preprocessing

```sh
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

The project can auto-generate diesel database accessors in `src/db.rs` 
whenever the schema (`init.sql`) changes.

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

