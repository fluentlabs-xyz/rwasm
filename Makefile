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
	cargo llvm-cov --lcov --manifest-path=./snippets/Cargo.toml > lcov1.info
	cargo llvm-cov --lcov --manifest-path=./Cargo.toml > lcov2.info
	cargo llvm-cov --lcov --manifest-path=./e2e/Cargo.toml > lcov3.info
	# merge all lcov files together
	grcov --llvm ./lcov1.info ./lcov2.info ./lcov3.info > lcov.info

all: test-specific-cases