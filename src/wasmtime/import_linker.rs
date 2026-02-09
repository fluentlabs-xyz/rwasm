use crate::{
    wasmtime::{context::WrappedContext, types::map_val_type, wasmtime_syscall_handler},
    ImportLinker,
};
use std::sync::Arc;
use wasmtime::{Engine, Linker};

/// Creates a Wasmtime linker from an rWasm `ImportLinker`.
///
/// Each imported function becomes a Wasmtime host function that:
/// - maps Wasmtime values to rWasm values,
/// - invokes `invoke_runtime_handler`,
/// - maps rWasm results back to Wasmtime values,
/// - converts certain trap codes into controlled termination (`ExecutionHalted`).
pub fn wasmtime_import_linker<T: 'static + Send + Sync>(
    engine: &Engine,
    import_linker: Arc<ImportLinker>,
) -> Linker<WrappedContext<T>> {
    let mut linker = Linker::<WrappedContext<T>>::new(engine);

    for (import_name, import_entity) in import_linker.iter() {
        let params = import_entity
            .params
            .iter()
            .copied()
            .map(map_val_type)
            .collect::<Vec<_>>();
        let result = import_entity
            .result
            .iter()
            .copied()
            .map(map_val_type)
            .collect::<Vec<_>>();

        let func_type = wasmtime::FuncType::new(engine, params, result);

        linker
            .func_new(
                import_name.module(),
                import_name.name(),
                func_type,
                move |caller, params, result| {
                    wasmtime_syscall_handler(import_entity.sys_func_idx, caller, params, result)
                },
            )
            .unwrap_or_else(|_| panic!("function import collision: {}", import_name));
    }

    linker
}
