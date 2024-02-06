.PHONY: all
all: solid_file

FEATURES =
TARGET=wasm32-unknown-unknown
PROFILE=release

RUSTFLAGS='-C target-feature=+bulk-memory -C relocation-model=pic'

.PHONY: solid_file
solid_file:
	RUSTFLAGS=${RUSTFLAGS} cargo b --profile ${PROFILE} --target=${TARGET} --features="${FEATURES}"
	mkdir -p tmp
	cp ../target/${TARGET}/${PROFILE}/rwasm_optimization.wasm ./tmp/$@.wasm
	wasm2wat ./tmp/$@.wasm > ./tmp/$@.wat
	cargo test translate_wasm2rwasm --features="std" --profile ${PROFILE} -- --nocapture
