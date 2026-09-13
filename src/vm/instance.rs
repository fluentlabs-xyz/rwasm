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
        // A parked execution still owns the store's memory and tables. Reject replacement
        // before clearing either so it can be resumed, or explicitly cancelled with reset.
        if store.resumable_context.is_some() {
            return Err(TrapCode::IllegalOpcode);
        }
        // The data/element drop state lives in the store but belongs to the instance: a module
        // instantiated on a store that already hosted another module has to start with all of its
        // segments live, or `memory.init`/`table.init` traps because the previous module dropped
        // the same segment index. The flag has to be cleared before the entrypoint runs, because
        // that code copies the module's active segments.
        store.clear_segment_flags();
        // The linear memory belongs to the instance as well. The entrypoint grows it from zero to
        // the size the module declares, so releasing the previous instance's pages here is what
        // keeps `memory.size`, the data-segment copies and every load/store relative to this
        // module instead of the one that ran before it.
        store.reset_memory();
        // Tables are per-instance too: the entrypoint grows each declared table and fills it with
        // nulls, so an entry the previous instance wrote must not survive into this one. Dropping
        // the tables here keeps `call_indirect` from dispatching into the previous module's code
        // and lets `table.size`/`table.get` report this module's table.
        store.reset_tables();
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
