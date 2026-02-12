.PHONY: build
build:
	# build all binaries
	cd benchmarks && make
	cargo build --manifest-path=./wasm/Cargo.toml
	cd snippets && make

.PHONY: test
test:
	# run tests
	cargo test --color=always --no-fail-fast --manifest-path Cargo.toml
	cargo test --color=always --no-fail-fast --manifest-path e2e/Cargo.toml
	cargo +nightly-2025-09-20 test --color=always --no-fail-fast --manifest-path snippets/Cargo.toml
	# run nitro test (with release flag)
	cargo test --release --package rwasm --test fluentbase test_nitro_verifier -- --ignored

.PHONY: coverage
coverage:
	# build all binaries
	cd benchmarks && make
	cargo build --manifest-path=./wasm/Cargo.toml
	cd snippets && make
	# run tests
	cargo llvm-cov --lcov --features=wasmtime --manifest-path=./Cargo.toml > lcov1.info
	cargo llvm-cov --lcov --manifest-path=./e2e/Cargo.toml > lcov2.info
	# merge all lcov files together
	grcov --llvm ./lcov1.info ./lcov2.info > lcov.info
	rm lcov1.info lcov2.info

.PHONY: clean
clean:
	# Delete all target folders
	find . -type d -name "target" -exec rm -rf {} +
	# Delete all Cargo.lock files except the root
	find . -name Cargo.lock ! -path './Cargo.lock' -type f -exec rm -f {} +

.PHONY: all
all: build test