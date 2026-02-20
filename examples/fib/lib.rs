#[cfg(not(target_arch = "wasm32"))]
pub const FIB_WASM: &[u8] = include_bytes!(env!("OUTPUT_WASM_PATH"));
