use alloc::{vec, vec::Vec};
use rwasm_legacy::{
    engine::RwasmConfig,
    rwasm::{BinaryFormat, BinaryFormatWriter, RwasmModule},
    Error,
};

pub struct RwasmCompilationResult {
    pub rwasm_bytecode: Vec<u8>,
    pub constructor_params: Vec<u8>,
}

pub fn compile_wasm_to_rwasm(
    wasm_binary: &[u8],
    rwasm_config: RwasmConfig,
) -> Result<RwasmCompilationResult, Error> {
    let mut config = RwasmModule::default_config(None);
    config.rwasm_config(rwasm_config);
    let (rwasm_module, constructor_params) =
        RwasmModule::compile_and_retrieve_input(wasm_binary, &config)?;
    let length = rwasm_module.encoded_length();
    let mut rwasm_bytecode = vec![0u8; length];
    let mut binary_format_writer = BinaryFormatWriter::new(&mut rwasm_bytecode);
    rwasm_module
        .write_binary(&mut binary_format_writer)
        .expect("failed to encode rwasm bytecode");
    Ok(RwasmCompilationResult {
        rwasm_bytecode,
        constructor_params,
    })
}
