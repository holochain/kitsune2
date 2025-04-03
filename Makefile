# A temporary Makefile to hold a single source of truth for running CI
# tasks both in automation and locally until we figure out better
# release automation tools.

.PHONY: all static-toml fmt clippy doc build test proto publish-all bump

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

publish-all:
	cargo publish -p kitsune2_api
	cargo publish -p kitsune2_bootstrap_srv
	cargo publish -p kitsune2_bootstrap_client
	cargo publish -p kitsune2_core
	cargo publish -p kitsune2_transport_tx5
	cargo publish -p kitsune2_dht
	cargo publish -p kitsune2_gossip
	cargo publish -p kitsune2
	cargo publish -p kitsune2_showcase
	export VER="v$$(grep version ./Cargo.toml | head -1 | cut -d ' ' -f 3 | cut -d \" -f 2)" && \
		git tag -a "$${VER}" -m "$${VER}" && \
		git push --tags;

bump:
	@if [ "$(ver)x" = "x" ]; then \
		echo "USAGE: make bump ver=0.0.1-alpha8"; \
		exit 1; \
	fi
	sed -i 's/^\(kitsune2[^=]*= { version = "\|version = "\)[^"]*"/\1$(ver)"/g' ./Cargo.toml
