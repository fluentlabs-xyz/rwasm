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
        // Invoke an entrypoint before (it triggers first init for memory, data, tables, etc. and also calls start section)
        engine.entrypoint(store, &module)?;
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
