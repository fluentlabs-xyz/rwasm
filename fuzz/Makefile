.PHONY: fuzz
fuzz:
	cargo +nightly fuzz run differential -- -runs=10000 -max_len=1048576

.PHONY: extract-wasm
extract-wasm:
	cargo run --manifest-path ../fuzz/Cargo.toml --bin repro_artifact -- ../fuzz/artifacts/differential/crash-$(HASH) /tmp/$(HASH).wasm
	wasm-tools print /tmp/$(HASH).wasm