use crate::{ExecutionEngine, RwasmModule, RwasmStore, TrapCode, Value};
use alloc::{boxed::Box, sync::Arc};

/// A handle to the current instance in a store.
///
/// A successful replacement invalidates earlier handles. Failed initialization restores the
/// previous instance; an interrupted initialization retains its state until completion or reset.
/// A replacement rejected because an execution is already parked leaves that execution intact.
pub struct RwasmInstance {
    engine: ExecutionEngine,
    module: RwasmModule,
    identity: Arc<()>,
    /// Name of the export compiled into the entrypoint, when the compiler selected one.
    ///
    /// The rwasm module has a single compile-time entrypoint (plus the optional state router), so
    /// a call that names a different export cannot be resolved the way the Wasmtime backend
    /// resolves it. Recording the name lets [`RwasmInstance::execute_named`] reject the mismatch
    /// instead of silently running the configured entrypoint.
    entrypoint_name: Option<Box<str>>,
}

impl RwasmInstance {
    /// Initializes a module transactionally, restoring the previous instance on a terminal trap.
    ///
    /// An interruption can be resumed through [`ExecutionEngine::resume`] or canceled with
    /// [`RwasmStore::reset`]. Initialization rollback preserves memory, tables, globals, and
    /// segment flags, but does not undo host callback side effects or consumed fuel.
    pub fn new<T>(
        store: &mut RwasmStore<T>,
        engine: ExecutionEngine,
        module: RwasmModule,
    ) -> Result<Self, TrapCode> {
        // Distinct allocations prevent handles from matching another store or a later instance
        // of the same module. The previous identity is restored if initialization fails.
        let identity = Arc::new(());
        store.begin_instantiation(identity.clone(), &module)?;
        // Legacy modules start at zero and have no separate initialization prologue.
        let outcome = if module.source_pc > 0 {
            engine.entrypoint(store, &module)
        } else {
            Ok(())
        };
        store.finish_instantiation(outcome)?;
        Ok(Self {
            engine,
            module,
            identity,
            entrypoint_name: None,
        })
    }

    /// Rejects handles whose instance state is no longer active in this store.
    fn check_store<T>(&self, store: &RwasmStore<T>) -> Result<(), TrapCode> {
        match &store.active_instance {
            Some(identity) if Arc::ptr_eq(identity, &self.identity) => Ok(()),
            _ => Err(TrapCode::IllegalOpcode),
        }
    }

    /// Records the export name the entrypoint was compiled from.
    pub fn with_entrypoint_name(mut self, entrypoint_name: Option<Box<str>>) -> Self {
        self.entrypoint_name = entrypoint_name;
        self
    }

    /// Executes the compiled entrypoint if this instance still owns the supplied store.
    ///
    /// `result` must have the entrypoint's result shape; see [`ExecutionEngine::execute`] for
    /// how a mismatch is reported.
    pub fn execute<T>(
        &self,
        store: &mut RwasmStore<T>,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        self.check_store(store)?;
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
        self.check_store(store)?;
        self.engine.resume(store, params, result)
    }
}
