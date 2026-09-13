use crate::{ExecutionEngine, RwasmModule, RwasmStore, TrapCode, Value};
use alloc::boxed::Box;

pub struct RwasmInstance {
    engine: ExecutionEngine,
    module: RwasmModule,
    /// Name of the export compiled into the entrypoint, when the compiler selected one.
    ///
    /// The rwasm module has a single compile-time entrypoint (plus the optional state router), so
    /// a call that names a different export cannot be resolved the way the Wasmtime backend
    /// resolves it. Recording the name lets [`RwasmInstance::execute_named`] reject the mismatch
    /// instead of silently running the configured entrypoint.
    entrypoint_name: Option<Box<str>>,
}

impl RwasmInstance {
    pub fn new<T>(
        store: &mut RwasmStore<T>,
        engine: ExecutionEngine,
        module: RwasmModule,
    ) -> Result<Self, TrapCode> {
        // Invoke an entrypoint before (it triggers first init for memory, data, tables, etc. and also calls a start section).
        // We call entrypoint only if source PC is greater than 0, it means that the module has a start section and it's not legacy module.
        if module.source_pc > 0 {
            engine.entrypoint(store, &module)?;
        }
        Ok(Self {
            engine,
            module,
            entrypoint_name: None,
        })
    }

    /// Records the export name the entrypoint was compiled from.
    pub fn with_entrypoint_name(mut self, entrypoint_name: Option<Box<str>>) -> Self {
        self.entrypoint_name = entrypoint_name;
        self
    }

    pub fn execute<T>(
        &self,
        store: &mut RwasmStore<T>,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        self.engine.execute(store, &self.module, params, result)
    }

    /// Executes the entrypoint, checking `func_name` against the configured entrypoint name.
    ///
    /// # Errors
    ///
    /// With [`TrapCode::UnknownExternalFunction`] if the module was compiled with
    /// [`crate::CompilationConfig::entrypoint_name`] and `func_name` is a different export: the
    /// compiled entrypoint is fixed, so running it anyway would execute other code than the
    /// caller asked for (the Wasmtime backend resolves the requested export instead). Modules
    /// routed by state carry no entrypoint name and accept any name.
    pub fn execute_named<T>(
        &self,
        store: &mut RwasmStore<T>,
        func_name: &str,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        if let Some(expected) = self.entrypoint_name.as_deref() {
            if expected != func_name {
                return Err(TrapCode::UnknownExternalFunction);
            }
        }
        self.execute(store, params, result)
    }

    /// Resumes an interrupted execution; see [`ExecutionEngine::resume`] for the error when the
    /// store holds none.
    pub fn resume<T>(
        &self,
        store: &mut RwasmStore<T>,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        self.engine.resume(store, params, result)
    }
}
