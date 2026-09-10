use crate::{CompilationConfig, ExecutionEngine, RwasmModule};
use alloc::vec::Vec;

mod module;
mod store;
mod syscall_handler;
mod types;

pub use module::*;
pub use store::*;
pub use syscall_handler::*;
pub use types::*;

/// Compiles `wasm_binary` once per available strategy and runs `f` on each definition, in the
/// order rwasm, then Wasmtime (when the feature is enabled).
///
/// The config must be strategy compatible (see
/// [`CompilationConfig::default_strategy_compatible`]); otherwise the strategies would charge
/// different fuel for the same module and the comparison would be meaningless, so such a config
/// is rejected with [`crate::CompilationError::StrategyIncompatibleConfig`].
pub fn for_each_strategy<R, F: FnMut(StrategyDefinition) -> Result<R, StrategyError>>(
    mut f: F,
    compilation_config: CompilationConfig,
    wasm_binary: &[u8],
) -> Result<Vec<R>, StrategyError> {
    StrategyDefinition::ensure_strategy_compatible(&compilation_config)?;
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
            crate::wasmtime::compile_wasmtime_module(compilation_config.clone(), wasm_binary)?;
        result.push(f(StrategyDefinition::Wasmtime { module })?);
    }
    Ok(result)
}
