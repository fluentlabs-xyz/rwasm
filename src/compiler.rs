mod config;
mod control_flow;
mod drop_keep;
mod entrypoint;
mod error;
mod func_builder;
mod instr_loc;
mod labels;
mod locals_registry;
mod parser;
mod segment_builder;
#[cfg(test)]
mod tests;
mod translator;
mod utils;
mod value_stack;

pub use self::{
    config::{CompilationConfig, StateRouterConfig},
    error::CompilationError,
    parser::ModuleParser,
};
use alloc::vec::Vec;

pub struct RwasmCompilationResult {
    pub rwasm_bytecode: Vec<u8>,
    pub constructor_params: Vec<u8>,
}

pub fn compile_wasm_to_rwasm(
    wasm_binary: &[u8],
    compilation_config: CompilationConfig,
) -> Result<RwasmCompilationResult, CompilationError> {
    let mut parser = ModuleParser::new(compilation_config);
    parser.parse(wasm_binary)?;
    let (module, params) = parser.finalize()?;
    let rwasm_bytecode = bincode::encode_to_vec(&module, bincode::config::legacy()).unwrap();
    Ok(RwasmCompilationResult {
        rwasm_bytecode,
        constructor_params: params.into(),
    })
}
