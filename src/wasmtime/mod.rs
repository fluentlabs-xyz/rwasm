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
use wasmparser::{Parser, Payload};

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

/// Applies the rwasm compile-time resource caps to a wasm binary before Wasmtime compiles it.
///
/// `RwasmModule::compile` rejects a module whose declared initial memory exceeds
/// `config.max_allowed_memory_pages`. Wasmtime has no compile-time equivalent: its store limiter
/// acts at instantiation, against a cap the runtime picks independently of the compiler. Without
/// this check a deployment whose runtime cap exceeds the compile cap accepts a module on the
/// Wasmtime strategy that the rwasm strategy rejects at compile time.
///
/// Only the section headers up to the code section are read, so this costs a fraction of the
/// compilation itself.
fn check_compile_limits(
    config: &CompilationConfig,
    wasm_binary: &[u8],
) -> Result<(), CompilationError> {
    let mut total_pages: u32 = 0;
    for payload in Parser::new(0).parse_all(wasm_binary) {
        match payload? {
            Payload::MemorySection(section) => {
                for memory_type in section.into_iter() {
                    let initial_pages = u32::try_from(memory_type?.initial)
                        .map_err(|_| CompilationError::MaxReadonlyDataReached)?;
                    total_pages = total_pages.saturating_add(initial_pages);
                    // inclusive, like `SegmentBuilder::add_memory_pages`
                    if total_pages > config.max_allowed_memory_pages {
                        return Err(CompilationError::MaxReadonlyDataReached);
                    }
                }
            }
            // every section this check cares about precedes the code section
            Payload::CodeSectionStart { .. } | Payload::End(_) => break,
            _ => {}
        }
    }
    Ok(())
}

/// Compiles a wasm binary with the Wasmtime engine configured by `compilation_config`.
///
/// Beyond Wasmtime's own validation this only enforces the rwasm compile-time resource caps
/// (see [`check_compile_limits`]). The rwasm strategy accepts a strict subset of what Wasmtime
/// accepts (see [`crate::RwasmModule::compile`]); callers that need both strategies to agree on
/// the accepted language go through [`crate::StrategyDefinition::new_as_wasmtime`], which runs
/// the rwasm front end first.
pub fn compile_wasmtime_module(
    compilation_config: CompilationConfig,
    wasm_binary: impl AsRef<[u8]>,
) -> Result<WasmtimeModule, CompilationError> {
    let wasm_binary = wasm_binary.as_ref();
    check_compile_limits(&compilation_config, wasm_binary)?;
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
