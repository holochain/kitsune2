# A temporary Makefile to hold a single source of truth for running CI
# tasks both in automation and locally until we figure out better
# release automation tools.

.PHONY: all static-toml fmt clippy doc build test

all: static-toml fmt clippy doc build test

static-toml:
	taplo format --check

fmt:
	cargo fmt --all -- --check

clippy:
	cargo clippy --all-targets -- --deny warnings

doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

build:
	cargo build

test:
	cargo test

proto:
	cd crates/tool_proto_build && cargo run
