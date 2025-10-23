.PHONY: test-specific-cases
test-specific-cases:
	# build all binaries
	cd benchmarks && make
	cd wasm && make
	cd snippets && make
	# run tests
	cargo test --color=always --no-fail-fast --manifest-path Cargo.toml --no-default-features --features=std,wasmtime
	cargo test --color=always --no-fail-fast --manifest-path Cargo.toml --no-default-features --features=std,wasmtime,unix-memory
	cargo test --color=always --no-fail-fast --manifest-path e2e/Cargo.toml --no-default-features --features=std,wasmtime
	cargo test --color=always --no-fail-fast --manifest-path e2e/Cargo.toml --no-default-features --features=std,wasmtime,unix-memory
	cargo +nightly-2025-09-20 test --color=always --no-fail-fast --manifest-path snippets/Cargo.toml
	# run nitro test (with release flag)
	cargo test --release --package fluent-rwasm --test nitro-verifier test_nitro_verifier --no-default-features --features=std,wasmtime -- --ignored
	cargo test --release --package fluent-rwasm --test nitro-verifier test_nitro_verifier --no-default-features --features=std,wasmtime,unix-memory -- --ignored

.PHONY: coverage
coverage:
	# build all binaries
	cd benchmarks && make
	cd wasm && make
	cd snippets && make
	# run tests
	cargo +nightly-2025-09-20 llvm-cov --lcov --manifest-path=./snippets/Cargo.toml > lcov1.info
	cargo llvm-cov --lcov --manifest-path=./Cargo.toml > lcov2.info
	cargo llvm-cov --lcov --manifest-path=./e2e/Cargo.toml > lcov3.info
	# merge all lcov files together
	grcov --llvm ./lcov1.info ./lcov2.info ./lcov3.info > lcov.info

.PHONY: clean
clean:
	# Delete all target folders
	find . -type d -name "target" -exec rm -rf {} +
	# Delete all Cargo.lock files except the root
	find . -name Cargo.lock ! -path './Cargo.lock' -type f -exec rm -f {} +

.PHONY: test
test:
	cargo test

all: test-specific-cases
