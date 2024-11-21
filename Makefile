# just a temporary Makefile so I can easily run the CI checks locally
# until we come up with better release tooling

.PHONY: all fmt clippy doc test

all: fmt clippy doc test

fmt:
	cargo fmt --all -- --check

clippy:
	cargo clippy --all-targets -- --deny warnings

doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

test:
	cargo test
