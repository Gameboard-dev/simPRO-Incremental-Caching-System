FROM rust:1-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libpq-dev \
        postgresql-client \
        ca-certificates \
    && cargo install diesel_cli --no-default-features --features postgres \
    && cargo install diesel_cli_ext \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*