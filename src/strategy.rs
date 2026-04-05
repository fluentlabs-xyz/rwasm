use crate::{CompilationConfig, ExecutionEngine, RwasmModule};

mod module;
mod store;
mod syscall_handler;
mod types;

pub use module::*;
pub use store::*;
pub use syscall_handler::*;
pub use types::*;

pub fn for_each_strategy<R, F: FnMut(StrategyDefinition) -> Result<R, StrategyError>>(
    mut f: F,
    compilation_config: CompilationConfig,
    wasm_binary: &[u8],
) -> Result<Vec<R>, StrategyError> {
    let mut result = Vec::new();
    // rwasm case
    {
        let (module, _) = RwasmModule::compile(compilation_config.clone(), wasm_binary)?;
        result.push(f(StrategyDefinition::Rwasm {
            module,
            engine: ExecutionEngine::acquire_shared(),
        })?);
    }
    // wasmtime case
    #[cfg(feature = "wasmtime")]
    {
        let module =
            crate::wasmtime::compile_wasmtime_module(compilation_config.clone(), wasm_binary)
                .unwrap();
        result.push(f(StrategyDefinition::Wasmtime { module })?);
    }
    Ok(result)
}
