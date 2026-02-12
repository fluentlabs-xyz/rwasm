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
    CompilationConfig,
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
) -> anyhow::Result<WasmtimeModule> {
    #[cfg(feature = "debug-print")]
    print!("parsing wasmtime module... ");
    let start = Instant::now();
    let engine = wasmtime_engine(&compilation_config);
    let module = unsafe { wasmtime::Module::deserialize(&engine, wasmtime_binary) };
    #[cfg(feature = "debug-print")]
    println!("{:?}", start.elapsed());
    module
}

pub fn compile_wasmtime_module(
    compilation_config: CompilationConfig,
    wasm_binary: impl AsRef<[u8]>,
) -> anyhow::Result<WasmtimeModule> {
    #[cfg(feature = "debug-print")]
    print!("compiling wasmtime module... ");
    let start = Instant::now();
    let engine = wasmtime_engine(&compilation_config);
    let module = wasmtime::Module::new(&engine, wasm_binary);
    #[cfg(feature = "debug-print")]
    println!("{:?}", start.elapsed());
    module
}

const MAX_CACHED_COMPILED_MODULES: usize = 10_000;

pub fn compile_wasmtime_module_cached(
    compilation_config: CompilationConfig,
    wasm_binary: impl AsRef<[u8]>,
    module_caching_key: [u8; 32],
) -> anyhow::Result<WasmtimeModule> {
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
    let module = compile_wasmtime_module(compilation_config, wasm_binary)?;
    guard.push(module_caching_key, module.clone());
    Ok(module)
}
