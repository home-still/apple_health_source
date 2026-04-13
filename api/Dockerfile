FROM rust:1.94-bookworm AS builder
WORKDIR /app
# Copy workspace root files
COPY ../Cargo.toml ../Cargo.lock ./
# Copy the api crate
COPY . ./api/
# Build
RUN cargo build --release -p apple-health-api

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/apple-health-api /usr/local/bin/
EXPOSE 3000
CMD ["apple-health-api"]
