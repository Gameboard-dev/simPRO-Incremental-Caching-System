

### Windows Dependencies

  WSL2:
  https://learn.microsoft.com/en-us/windows/wsl/install
  https://apps.microsoft.com/detail/9pdxgncfsczv?hl=en-US&gl=GB

  Docker:
  https://docs.docker.com/engine/install/ubuntu/

---

### SQL-Diesel Synchronization

The project can automatically regenerate `src/db.rs` whenever the schema (`init.sql`) changes.

```sh
cargo install cargo-watch
```

```sh
cargo watch --poll --delay 1 -w ./init.sql -x "run -p xtask -- generate-schema"
```

---

### OpenAPI-Progenator Synchronization

The project can automatically regenerate `src/api.rs` whenever the schema (`openapi.yaml`) changes.

```sh
cargo build
```