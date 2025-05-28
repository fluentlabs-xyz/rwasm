all: test-specific-cases

.PHONY: test-specific-cases
test-specific-cases:
	# build all binaries
	cd benchmarks && make
	cd wasm && make
	cd snippets && make
	# run tests
	cargo test --color=always --no-fail-fast --manifest-path Cargo.toml
	cargo test --color=always --no-fail-fast --manifest-path e2e/Cargo.toml
	cargo test --color=always --no-fail-fast --manifest-path snippets/Cargo.toml

.PHONY: coverage
coverage:
	# build all binaries
	cd benchmarks && make
	cd wasm && make
	cd snippets && make
	# run tests
	cargo llvm-cov --json --manifest-path=./snippets/Cargo.toml
	cargo llvm-cov --json --manifest-path=./Cargo.toml
	cargo llvm-cov --json --manifest-path=./e2e/Cargo.toml
	# merge all lcov files together
	grcov ./snippets/lcov.info ./lcov.info ./e2e/lcov.info > lcov.info
