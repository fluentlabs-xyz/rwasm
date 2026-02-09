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
use std::time::Instant;

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
