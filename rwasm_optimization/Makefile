.PHONY: all
all: solid_file

FEATURES=
TARGET=wasm32-unknown-unknown
PROFILE=release

RUSTFLAGS='-C target-feature=+bulk-memory'
#RUSTFLAGS='-C target-feature=+bulk-memory -C relocation-model=pic' # slightly increases number of instructions

.PHONY: solid_file
solid_file:
	clear
	RUSTFLAGS=${RUSTFLAGS} cargo b --profile ${PROFILE} --target=${TARGET} --features="${FEATURES}"
	mkdir -p tmp
	cp ../target/${TARGET}/${PROFILE}/rwasm_optimization.wasm ./tmp/$@.wasm
	wasm2wat ./tmp/$@.wasm > ./tmp/$@.wat
	cargo test translate_wasm2rwasm --features=std --profile ${PROFILE} -- --nocapture

.PHONY: clean
clean:
	cargo clean