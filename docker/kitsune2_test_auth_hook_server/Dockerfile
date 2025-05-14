FROM rust:1.85-bookworm AS builder

# Pick a directory to work in
WORKDIR /usr/src/kitsune2

# Copy the workspace's source code into the container
ADD ["Cargo.toml", "Cargo.lock", "./"]
ADD crates ./crates

# Build the bootstrap server
RUN cargo install --path ./crates/bootstrap_srv --example test-auth-hook-server

FROM debian:bookworm-slim

EXPOSE 80

# Copy the built binary into the runtime container
COPY --from=builder /usr/local/cargo/bin/test-auth-hook-server /usr/local/bin/

# Create a directory to store the TLS certificate and key
RUN mkdir -p /etc/bootstrap_srv

# Run the bootstrap server with production settings
CMD ["test-auth-hook-server", "0.0.0.0:80"]
