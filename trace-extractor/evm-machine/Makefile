.PHONY: build
build:
	cargo b --target-dir=./target --target=wasm32-unknown-unknown --release --no-default-features
	cp ./target/wasm32-unknown-unknown/release/evm_machine.wasm ./lib.wasm
	wasm2wat ./lib.wasm > ./lib.wat || true
