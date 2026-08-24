.PHONY: ensure-wasm-targets
ensure-wasm-targets:
	rustup target add wasm32-unknown-unknown
	rustup +nightly-2025-09-20 target add wasm32-unknown-unknown

.PHONY: ensure-submodules
ensure-submodules:
	git submodule update --init --recursive

.PHONY: build
build: ensure-wasm-targets ensure-submodules
	# check rwasm for errors
	cargo check
	# build all binaries
	cargo build --manifest-path=./examples/fib/Cargo.toml
	cargo build --manifest-path=./examples/wasm/Cargo.toml
	# build snippets
	cd snippets && make

.PHONY: test
test: build
	# run tests
	cargo test --color=always --no-fail-fast --manifest-path Cargo.toml
	cargo test --color=always --no-fail-fast --manifest-path e2e/Cargo.toml
	cargo +nightly-2025-09-20 test --color=always --no-fail-fast --manifest-path snippets/Cargo.toml
	# run nitro test (with release flag)
	cargo test --release --package rwasm --test fluentbase test_nitro_verifier -- --ignored

.PHONY: clippy
clippy: ensure-wasm-targets
	cargo clippy --all-targets --all-features
	cargo clippy --manifest-path=./e2e/Cargo.toml --all-targets --all-features
	cargo +nightly-2025-09-20 clippy --manifest-path=./snippets/Cargo.toml --lib --all-features

.PHONY: coverage
coverage: build
	# Run the unit and integration suites with production-code coverage.
	cargo llvm-cov --lcov --features=wasmtime --manifest-path=./Cargo.toml > lcov1.info
	# Instrument rwasm as an e2e dependency, then export both crates from that run.
	cargo llvm-cov --no-report --manifest-path=./e2e/Cargo.toml --dep-coverage rwasm
	cd e2e && cargo llvm-cov report --lcov > ../lcov2.info
	# Merge all LCOV files and enforce coverage for production sources.
	grcov --llvm ./lcov1.info ./lcov2.info > lcov.info
	rm lcov1.info lcov2.info
	awk -F: '/^SF:/{include=$$0 !~ /\/e2e\//} include && /^LF:/{lines+=$$2} include && /^LH:/{hits+=$$2} END{coverage=100*hits/lines; printf "rwasm line coverage: %.2f%% (%d/%d)\n", coverage, hits, lines; if (coverage < 90) exit 1}' lcov.info

.PHONY: clean
clean:
	# Delete all target folders
	find . -type d -name "target" -exec rm -rf {} +
	# Delete all Cargo.lock files except the root
	find . -name Cargo.lock ! -path './Cargo.lock' -type f -exec rm -f {} +

.PHONY: all
all: build test
