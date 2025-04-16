all: test-specific-cases

.PHONY: test-specific-cases
test-specific-cases:
	cargo test --color=always --no-fail-fast --manifest-path rwasm/Cargo.toml
	cargo test --color=always --no-fail-fast --manifest-path e2e/Cargo.toml
