.PHONY: fuzz
fuzz:
	cargo +nightly fuzz run differential -- -runs=3000

HASH=?

.PHONY: extract-wasm
extract-wasm:
	cargo run --manifest-path ../fuzz/Cargo.toml --bin repro_artifact -- ../fuzz/artifacts/differential/crash-$(HASH) /tmp/$(HASH).wasm
	wasm-tools print /tmp/$(HASH).wasm