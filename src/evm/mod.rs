use crate::{instruction_set, RwasmModule, RwasmModuleInner};
use alloc::vec;

pub fn compile_evm_to_rwasm<T: AsRef<[u8]>>(evm_bytecode: T) -> RwasmModule {
    let code_section = instruction_set! {
        // TODO(dmitry123): Yes, we don't have EVM compiler implemented right now
        Unreachable
    };
    RwasmModuleInner {
        code_section,
        data_section: vec![],
        elem_section: vec![],
        hint_section: evm_bytecode.as_ref().to_vec(),
    }
    .into()
}
