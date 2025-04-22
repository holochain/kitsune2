# A Makefile for bumping versions and publishing all crates.

.PHONY: publish-all bump

publish-all:
	cargo publish -p kitsune2_api
	cargo publish -p kitsune2_test_utils
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
