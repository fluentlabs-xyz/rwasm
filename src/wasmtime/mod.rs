mod engine;
mod store;

mod context;
mod import_linker;
mod syscall_handler;
#[cfg(test)]
mod tests;
mod types;

pub use self::{
    context::WasmtimeCaller, import_linker::wasmtime_import_linker, store::WasmtimeStore,
    syscall_handler::wasmtime_syscall_handler,
};
use crate::{
    wasmtime::{context::WrappedContext, engine::wasmtime_engine},
    CompilationConfig,
};
use std::{
    collections::HashMap,
    sync::{OnceLock, RwLock},
    time::Instant,
};

pub type WasmtimeModule = wasmtime::Module;
pub type WasmtimeLinker<T> = wasmtime::Linker<T>;

pub fn deserialize_wasmtime_module(
    compilation_config: CompilationConfig,
    wasmtime_binary: impl AsRef<[u8]>,
) -> anyhow::Result<WasmtimeModule> {
    print!("parsing wasmtime module... ");
    let start = Instant::now();
    let engine = wasmtime_engine(&compilation_config);
    let module = unsafe { wasmtime::Module::deserialize(&engine, wasmtime_binary) };
    println!("{:?}", start.elapsed());
    module
}

pub fn compile_wasmtime_module(
    compilation_config: CompilationConfig,
    wasm_binary: impl AsRef<[u8]>,
) -> anyhow::Result<WasmtimeModule> {
    print!("compiling wasmtime module... ");
    let start = Instant::now();
    let engine = wasmtime_engine(&compilation_config);
    let module = wasmtime::Module::new(&engine, wasm_binary);
    println!("{:?}", start.elapsed());
    module
}

pub fn compile_wasmtime_module_cached(
    compilation_config: CompilationConfig,
    wasm_binary: impl AsRef<[u8]>,
    caching_key: [u8; 32],
) -> anyhow::Result<WasmtimeModule> {
    static COMPILED_MODULES: OnceLock<RwLock<HashMap<[u8; 32], WasmtimeModule>>> = OnceLock::new();
    let compiled_modules = COMPILED_MODULES.get_or_init(|| RwLock::new(HashMap::new()));

    // Fast path: read lock lookup.
    {
        let guard = compiled_modules.read().unwrap();
        if let Some(module) = guard.get(&caching_key) {
            return Ok(module.clone());
        }
    }

    // Slow path: compile and insert under write lock (with re-check).
    let mut guard = compiled_modules.write().unwrap();
    if let Some(module) = guard.get(&caching_key) {
        return Ok(module.clone());
    }

    let module = compile_wasmtime_module(compilation_config, wasm_binary)?;
    guard.insert(caching_key, module.clone());
    Ok(module)
}
