FROM rust:1.85-bookworm AS builder

# Pick a directory to work in
WORKDIR /usr/src/kitsune2

# Copy the workspace's source code into the container
ADD ["Cargo.toml", "Cargo.lock", "./"]
ADD crates ./crates

# Build the bootstrap server
RUN cargo install --path ./crates/bootstrap_srv --bin kitsune2-bootstrap-srv

FROM golang:1.24-bookworm AS lego-builder

# Install the lego tool
ENV GO111MODULE=on
RUN go install github.com/go-acme/lego/v4/cmd/lego@latest

FROM debian:bookworm-slim

EXPOSE 443

# Copy the built binary into the runtime container
COPY --from=builder /usr/local/cargo/bin/kitsune2-bootstrap-srv /usr/local/bin/
COPY --from=lego-builder /go/bin/lego /usr/local/bin/

# Create a directory to store the TLS certificate and key
RUN mkdir -p /etc/bootstrap_srv

# Run the bootstrap server with production settings
CMD ["kitsune2-bootstrap-srv", "--production", "--tls-cert", "/etc/bootstrap_srv/cert.pem", "--tls-key", "/etc/bootstrap_srv/key.pem"]
