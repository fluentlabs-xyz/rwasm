.PHONY: build
build:
	cargo b --release --target=wasm32-unknown-unknown --no-default-features
	cp ./target/wasm32-unknown-unknown/release/wasm.wasm ./lib.wasm
	wasm2wat ./lib.wasm > ./lib.wat || true