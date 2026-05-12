FROM rust:1-bookworm
WORKDIR /app
COPY . .
RUN cargo build
EXPOSE 3000
CMD ["cargo", "run"]