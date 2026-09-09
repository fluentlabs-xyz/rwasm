use crate::{
    wasmtime::{
        context::WrappedContext, syscall_handler::wasmtime_syscall_handler_raw,
        types::map_val_type, wasmtime_syscall_handler,
    },
    ImportLinker,
};
use std::sync::Arc;
use wasmparser::ValType;

/// Whether a signature can go through the raw trampoline, which handles numeric values only.
fn is_numeric_signature(params: &[ValType], result: &[ValType]) -> bool {
    params.iter().chain(result).all(|ty| {
        matches!(
            ty,
            ValType::I32 | ValType::I64 | ValType::F32 | ValType::F64
        )
    })
}

/// Creates a Wasmtime linker from an rWasm `ImportLinker`.
///
/// Each imported function becomes a Wasmtime host function that:
/// - maps Wasmtime values to rWasm values,
/// - invokes `invoke_runtime_handler`,
/// - maps rWasm results back to Wasmtime values,
/// - converts certain trap codes into controlled termination (`ExecutionHalted`).
pub fn wasmtime_import_linker<T: 'static>(
    engine: &wasmtime::Engine,
    import_linker: &Arc<ImportLinker>,
) -> wasmtime::Linker<WrappedContext<T>> {
    let mut linker = wasmtime::Linker::<WrappedContext<T>>::new(engine);

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

        let linked = if is_numeric_signature(import_entity.params, import_entity.result) {
            let sys_func_idx = import_entity.sys_func_idx;
            let (param_types, result_types) = (import_entity.params, import_entity.result);
            // SAFETY: `func_type` was built from exactly `param_types` and `result_types`,
            // both numeric only, and the raw trampoline reads and writes the value slots
            // according to those same slices.
            unsafe {
                linker.func_new_unchecked(
                    import_name.module(),
                    import_name.name(),
                    func_type,
                    move |caller, slots| {
                        wasmtime_syscall_handler_raw(
                            sys_func_idx,
                            param_types,
                            result_types,
                            caller,
                            slots,
                        )
                    },
                )
            }
        } else {
            linker.func_new(
                import_name.module(),
                import_name.name(),
                func_type,
                move |caller, params, result| {
                    wasmtime_syscall_handler(import_entity.sys_func_idx, caller, params, result)
                },
            )
        };
        linked.unwrap_or_else(|_| panic!("function import collision: {}", import_name));
    }

    linker
}
