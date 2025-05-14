.PHONY: lib.wasm
lib.wasm: Cargo.toml lib.rs
	cargo b --release --target=wasm32-unknown-unknown --no-default-features
	cp ./target/wasm32-unknown-unknown/release/snippets.wasm lib.wasm
	wasm2wat lib.wasm > lib.wat