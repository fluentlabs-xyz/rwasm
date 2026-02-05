mod compiled_expr;
mod config;
mod control_flow;
mod drop_keep;
mod error;
mod func_builder;
mod func_type_registry;
pub mod intrinsic;
mod labels;
mod locals_registry;
mod parser;
mod segment_builder;
mod snippets;
mod translator;
mod utils;
mod value_stack;

pub use self::{
    config::{CompilationConfig, StateRouterConfig},
    error::CompilationError,
    parser::ModuleParser,
};
use crate::RwasmModule;
use alloc::vec::Vec;

pub struct RwasmCompilationResult {
    pub rwasm_bytecode: Vec<u8>,
    pub constructor_params: Vec<u8>,
}

pub fn compile_wasm_to_rwasm(
    wasm_binary: &[u8],
    compilation_config: CompilationConfig,
) -> Result<RwasmCompilationResult, CompilationError> {
    let (module, params) = RwasmModule::compile(compilation_config, wasm_binary)?;
    Ok(RwasmCompilationResult {
        rwasm_bytecode: module.serialize(),
        constructor_params: params.into_vec(),
    })
}
