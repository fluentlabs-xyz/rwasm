.PHONY: lib.wasm
lib.wasm: Cargo.toml lib.rs
	cargo +nightly-2025-09-20 b --release --target-dir=./target --target=wasm32-unknown-unknown --no-default-features
	cp ./target/wasm32-unknown-unknown/release/snippets.wasm lib.wasm
	wasm2wat lib.wasm > lib.wat || true