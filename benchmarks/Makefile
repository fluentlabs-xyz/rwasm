build:
	RUSTFLAGS="-C link-arg=-zstack-size=65536" cargo b --target=wasm32-unknown-unknown --release --no-default-features
	cp ./target/wasm32-unknown-unknown/release/fib.wasm ./lib.wasm
	wasm2wat ./lib.wasm > ./lib.wat