#![cfg_attr(not(feature = "std"), no_std)]
#![warn(unused_crate_dependencies)]
#![allow(unused_variables, dead_code)]
#![recursion_limit = "750"]

extern crate alloc;
extern crate core;

mod compiler;
mod isa;
mod module;
mod strategy;
mod types;
mod vm;
#[cfg(feature = "wasmtime")]
pub mod wasmtime;

pub use compiler::*;
pub use isa::*;
pub use module::*;
pub use rwasm_fuel_policy::*;
pub use strategy::*;
pub use types::*;
pub use vm::*;
pub use wasmparser::{FuncType, ValType};
#[cfg(feature = "wasmtime")]
pub use wasmtime::{
    compile_wasmtime_module, compile_wasmtime_module_cached, WasmtimeCaller, WasmtimeLinker,
    WasmtimeModule, WasmtimeStore,
};

#[cfg(feature = "std")]
pub fn for_each_strategy<F: FnMut(Strategy) -> Result<(), StrategyError>>(
    mut f: F,
    compilation_config: CompilationConfig,
    wasm_binary: &[u8],
) -> Result<(), StrategyError> {
    // rwasm case
    {
        let (module, _) = RwasmModule::compile(compilation_config.clone(), wasm_binary)?;
        f(Strategy::Rwasm {
            module,
            engine: ExecutionEngine::acquire_shared(),
        })?;
    }
    // wasmtime case
    #[cfg(feature = "wasmtime")]
    {
        let module = compile_wasmtime_module(compilation_config.clone(), wasm_binary).unwrap();
        f(Strategy::Wasmtime { module })?;
    }
    Ok(())
}

#[cfg(test)]
use hex_literal as _;
#[cfg(test)]
use wat as _;
