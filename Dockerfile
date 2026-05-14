FROM rust:1-bookworm
WORKDIR /app
COPY . .
EXPOSE 3000
