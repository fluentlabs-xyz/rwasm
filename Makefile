all: test-specific-cases

.PHONY: test-specific-cases
test-specific-cases:
	cd benchmarks && make
	cd wasm && make
	cd snippets && make
	cargo test --color=always --no-fail-fast --manifest-path Cargo.toml
	cargo test --color=always --no-fail-fast --manifest-path e2e/Cargo.toml
