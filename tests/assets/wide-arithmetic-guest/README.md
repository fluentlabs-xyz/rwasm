# wide-arithmetic-guest

The guest behind `tests/wide_arithmetic.rs`: plain Rust `u128` arithmetic that LLVM lowers to the
wide-arithmetic instructions (`i64.mul_wide_u`, `i64.mul_wide_s`, `i64.add128`) when the target
feature is on. `src/mont.rs` is included by the test as well, so the native reference and the
guest run the same code.

Rebuild `../wide-arithmetic-guest.wasm` with (the target feature is not stabilized, so `rustc`
warns about it and the build still succeeds):

```bash
RUSTFLAGS="-C target-feature=+wide-arithmetic" \
  cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/wide_arithmetic_guest.wasm ../wide-arithmetic-guest.wasm
```

`tests/wide_arithmetic.rs` asserts that the checked-in binary contains the instructions, so a
build without the feature fails the test instead of silently testing the old lowering.
