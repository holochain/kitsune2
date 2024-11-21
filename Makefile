# just a temporary Makefile so I can easily run the CI checks locally
# until we come up with better release tooling

.PHONY: all fmt clippy test

all: fmt clippy test

fmt:
	cargo fmt --all -- --check

clippy:
	cargo clippy --all-targets -- --deny warnings

test:
	cargo test
