use crate::{ExecutionEngine, RwasmModule, RwasmStore, TrapCode, Value};

pub struct RwasmInstance {
    engine: ExecutionEngine,
    module: RwasmModule,
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
        Ok(Self { engine, module })
    }

    pub fn execute<T>(
        &self,
        store: &mut RwasmStore<T>,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        self.engine.execute(store, &self.module, params, result)
    }

    pub fn resume<T>(
        &self,
        store: &mut RwasmStore<T>,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        self.engine.resume(store, params, result)
    }
}
