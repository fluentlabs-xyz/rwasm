all: test-specific-cases

.PHONY: test-specific-cases
test-specific-cases:
	cd benchmarks && make build
	cargo test --color=always --no-fail-fast --manifest-path Cargo.toml
	cargo test --color=always --no-fail-fast --manifest-path e2e/Cargo.toml
