use crate::{ExecutionEngine, RwasmModule, RwasmStore, TrapCode, Value};
use alloc::{boxed::Box, sync::Arc};
use smallvec::SmallVec;
use wasmparser::FuncType;

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
    /// Present when compiled through the typed strategy API; raw bytecode has no signature.
    entrypoint_type: Option<FuncType>,
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
            entrypoint_type: None,
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

    pub(crate) fn with_entrypoint_type(mut self, entrypoint_type: Option<FuncType>) -> Self {
        self.entrypoint_type = entrypoint_type;
        self
    }

    /// The typed API treats result values as placeholders, like Wasmtime. Run with the declared
    /// types and publish them only on success, leaving the caller's buffer intact on a trap.
    fn with_typed_results(
        &self,
        result: &mut [Value],
        run: impl FnOnce(&mut [Value]) -> Result<(), TrapCode>,
    ) -> Result<(), TrapCode> {
        let Some(signature) = &self.entrypoint_type else {
            return run(result);
        };
        if result.len() != signature.results().len() {
            return Err(TrapCode::IllegalOpcode);
        }
        let mut typed: SmallVec<[Value; 8]> = signature
            .results()
            .iter()
            .copied()
            .map(Value::default)
            .collect();
        run(&mut typed)?;
        result.clone_from_slice(&typed);
        Ok(())
    }

    /// Executes the compiled entrypoint if this instance still owns the supplied store.
    ///
    /// Strategy-compiled named entrypoints validate parameter types and result count before
    /// execution, and overwrite result placeholders with the declared types. Raw instances use
    /// the stack-slot calling convention described by [`ExecutionEngine::execute`].
    pub fn execute<T>(
        &self,
        store: &mut RwasmStore<T>,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        self.check_store(store)?;
        if let Some(signature) = &self.entrypoint_type {
            if params.len() != signature.params().len()
                || params
                    .iter()
                    .zip(signature.params())
                    .any(|(value, ty)| value.ty() != *ty)
            {
                return Err(TrapCode::IllegalOpcode);
            }
        }
        self.with_typed_results(result, |result| {
            self.engine.execute(store, &self.module, params, result)
        })
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
        self.with_typed_results(result, |result| self.engine.resume(store, params, result))
    }
}
