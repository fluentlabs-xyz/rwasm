mod engine;
mod instance;

mod context;
mod import_linker;
mod syscall_handler;
#[cfg(test)]
mod tests;
mod types;

pub use self::{
    context::WasmtimeCaller, import_linker::wasmtime_import_linker, instance::WasmtimeExecutor,
    syscall_handler::wasmtime_syscall_handler,
};
use crate::{
    wasmtime::{context::WrappedContext, engine::wasmtime_engine},
    CompilationConfig, CompilationError,
};
use lru::LruCache;
use std::{
    num::NonZeroUsize,
    sync::{Mutex, OnceLock},
    time::Instant,
};

pub type WasmtimeModule = wasmtime::Module;
pub type WasmtimeLinker<T> = wasmtime::Linker<WrappedContext<T>>;

pub fn deserialize_wasmtime_module(
    compilation_config: CompilationConfig,
    wasmtime_binary: impl AsRef<[u8]>,
) -> wasmtime::Result<WasmtimeModule> {
    #[cfg(feature = "debug-print")]
    print!("parsing wasmtime module... ");
    let start = Instant::now();
    let engine = wasmtime_engine(&compilation_config);
    let module = unsafe { wasmtime::Module::deserialize(&engine, wasmtime_binary) };
    #[cfg(feature = "debug-print")]
    println!("{:?}", start.elapsed());
    module
}

/// Compiles a wasm binary with the Wasmtime engine configured by `compilation_config`.
///
/// This applies Wasmtime's own validation only. The rwasm strategy accepts a strict subset of
/// what Wasmtime accepts (see [`crate::RwasmModule::compile`]); callers that need both strategies
/// to agree on the accepted language go through [`crate::StrategyDefinition::new_as_wasmtime`],
/// which runs the rwasm front end first.
pub fn compile_wasmtime_module(
    compilation_config: CompilationConfig,
    wasm_binary: impl AsRef<[u8]>,
) -> Result<WasmtimeModule, CompilationError> {
    #[cfg(feature = "debug-print")]
    print!("compiling wasmtime module... ");
    let start = Instant::now();
    let engine = wasmtime_engine(&compilation_config);
    let module = wasmtime::Module::new(&engine, wasm_binary)
        .map_err(CompilationError::WasmtimeCompilationFailed);
    #[cfg(feature = "debug-print")]
    println!("{:?}", start.elapsed());
    module
}

const MAX_CACHED_COMPILED_MODULES: usize = 10_000;

/// Like [`compile_wasmtime_module`], but memoizes the compiled module in a process-wide LRU cache
/// under `module_caching_key`.
pub fn compile_wasmtime_module_cached(
    compilation_config: CompilationConfig,
    wasm_binary: impl AsRef<[u8]>,
    module_caching_key: [u8; 32],
) -> Result<WasmtimeModule, CompilationError> {
    compile_wasmtime_module_cached_with(compilation_config, module_caching_key, |config| {
        compile_wasmtime_module(config, wasm_binary)
    })
}

/// Returns the module cached under `module_caching_key`, or compiles it with `compile` and caches
/// the result. The cache lock is held across `compile`, so concurrent callers with the same key
/// compile once.
pub(crate) fn compile_wasmtime_module_cached_with<E>(
    compilation_config: CompilationConfig,
    module_caching_key: [u8; 32],
    compile: impl FnOnce(CompilationConfig) -> Result<WasmtimeModule, E>,
) -> Result<WasmtimeModule, E> {
    static COMPILED_MODULES: OnceLock<Mutex<LruCache<[u8; 32], WasmtimeModule>>> = OnceLock::new();
    let compiled_modules = COMPILED_MODULES.get_or_init(|| {
        Mutex::new(LruCache::new(
            NonZeroUsize::new(MAX_CACHED_COMPILED_MODULES).unwrap(),
        ))
    });
    // P.S: We don't check config hash here for performance reasons, assuming it's handled by an external caching key
    let mut guard = compiled_modules.lock().unwrap();
    if let Some(module) = guard.get(&module_caching_key) {
        return Ok(module.clone());
    }
    let module = compile(compilation_config)?;
    guard.push(module_caching_key, module.clone());
    Ok(module)
}
