/// Type of the hint
#[derive(PartialEq, Clone, Debug)]
pub enum HintType {
    /// Hint contains input of Wasm bytecode
    WASM,
    /// Hint contains EVM bytecode (fallback)
    EVM,
}

const WASM_MAGIC_BYTES: [u8; 4] = [0x00, 0x61, 0x73, 0x6d];

impl HintType {
    pub fn from_ref<T: AsRef<[u8]>>(input: T) -> Self {
        if input.as_ref().starts_with(&WASM_MAGIC_BYTES) {
            HintType::WASM
        } else {
            HintType::EVM
        }
    }
}
