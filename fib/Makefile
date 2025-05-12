RUSTFLAGS="-C link-arg=-zstack-size=1024"

build:
	RUSTFLAGS="-C link-arg=-zstack-size=0" cargo b --target=wasm32-unknown-unknown --release --no-default-features
	cp ./target/wasm32-unknown-unknown/release/fib.wasm ./lib.wasm
	wasm2wat ./lib.wasm > ./lib.wat