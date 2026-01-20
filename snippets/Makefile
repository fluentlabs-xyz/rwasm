.PHONY: lib.wasm
lib.wasm: Cargo.toml lib.rs
	cargo +nightly-2025-09-20 b --release --target=wasm32-unknown-unknown --no-default-features
	cp ./target/wasm32-unknown-unknown/release/snippets.wasm lib.wasm
	wasm2wat lib.wasm > lib.wat || true
	cargo test --color=always --package snippets --lib extractor::test_extract_rwasm_bytecode --profile test --no-fail-fast -- --nocapture
