#![cfg_attr(not(feature = "std"), no_std)]
#![warn(unused_crate_dependencies)]
#![allow(unused_variables)]
#![recursion_limit = "750"]

mod compiler;
mod strategy;
mod types;
mod vm;
mod wasmi;
#[cfg(feature = "wasmtime")]
mod wasmtime;

extern crate alloc;
extern crate core;

pub use compiler::*;
use libm as _;
pub use strategy::*;
pub use types::*;
pub use vm::*;
pub use wasmi::*;
pub use wasmparser::{FuncType, ValType};
#[cfg(feature = "wasmtime")]
pub use wasmtime::*;

#[cfg(feature = "std")]
pub fn for_each_strategy<F: FnMut(Strategy) -> Result<(), StrategyError>>(
    mut f: F,
    compilation_config: CompilationConfig,
    wasm_binary: &[u8],
) -> Result<(), StrategyError> {
    use std::rc::Rc;
    // rwasm case
    {
        let (rwasm_module, _) = RwasmModule::compile(compilation_config.clone(), wasm_binary)?;
        f(Strategy::Rwasm {
            module: Rc::new(rwasm_module),
            engine: ExecutionEngine::acquire_shared(),
        })?;
    }
    // wasmtime case
    #[cfg(feature = "wasmtime")]
    {
        let wasmtime_module =
            compile_wasmtime_module(compilation_config.clone(), wasm_binary).unwrap();
        f(Strategy::Wasmtime {
            module: Rc::new(wasmtime_module),
        })?;
    }
    // wasmi case
    {
        let wasmi_module = compile_wasmi_module(compilation_config.clone(), wasm_binary).unwrap();
        f(Strategy::Wasmi {
            module: Rc::new(wasmi_module),
        })?;
    }
    Ok(())
}
