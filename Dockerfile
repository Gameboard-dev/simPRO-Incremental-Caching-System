FROM rust:1-bookworm
WORKDIR /app
COPY . .
EXPOSE 3000

# cargo watch could be used here 
# for incremental rebuilds during 
# development