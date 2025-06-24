use crate::{
    ExecutionEngine,
    ExecutorConfig,
    ImportLinker,
    RwasmModule,
    RwasmStore,
    TrapCode,
    UntypedValue,
    Value,
};
#[cfg(feature = "wasmtime")]
use crate::{WasmtimeModule, WasmtimeWorker};
use alloc::{rc::Rc, sync::Arc};
use core::cell::{Ref, RefCell, RefMut};

pub trait Store<T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode>;

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode>;

    fn context_mut(&mut self) -> RefMut<'_, T>;

    fn context(&self) -> Ref<'_, T>;

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode>;

    fn remaining_fuel(&mut self) -> Option<u64>;
}

pub trait Caller<T>: Store<T> {
    #[deprecated(note = "only for e2e testing suite will be removed soon")]
    fn program_counter(&self) -> u32;

    #[deprecated(note = "only for e2e testing suite will be removed soon")]
    fn sync_stack_ptr(&mut self);

    #[deprecated(note = "only for e2e testing suite will be removed soon")]
    fn stack_push(&mut self, value: UntypedValue);
}

pub type SyscallHandler<T> =
    fn(&mut dyn Caller<T>, u32, &[Value], &mut [Value]) -> Result<(), TrapCode>;

pub fn always_failing_syscall_handler<T>(
    _caller: &mut dyn Caller<T>,
    _func_idx: u32,
    _params: &[Value],
    _result: &mut [Value],
) -> Result<(), TrapCode> {
    Err(TrapCode::UnknownExternalFunction)
}

pub enum Strategy {
    Rwasm {
        module: Arc<RwasmModule>,
        engine: Rc<RefCell<ExecutionEngine>>,
    },
    #[cfg(feature = "wasmtime")]
    Wasmtime { module: Arc<WasmtimeModule> },
}

pub enum TypedStore<T: Send + 'static> {
    Rwasm(RwasmStore<T>),
    #[cfg(feature = "wasmtime")]
    Wasmtime(WasmtimeWorker<T>),
}

impl Strategy {
    pub fn create_store<T: 'static + Send>(
        &self,
        config: ExecutorConfig,
        import_linker: Arc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
    ) -> TypedStore<T> {
        match self {
            Strategy::Rwasm { .. } => TypedStore::Rwasm(RwasmStore::new(
                config,
                import_linker,
                context,
                syscall_handler,
            )),
            #[cfg(feature = "wasmtime")]
            Strategy::Wasmtime { module } => TypedStore::Wasmtime(WasmtimeWorker::new(
                module.clone(),
                import_linker,
                context,
                syscall_handler,
            )),
        }
    }

    pub fn execute<'a, T: 'static + Send>(
        &'a mut self,
        store: &mut TypedStore<T>,
        func_name: &'static str,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        match self {
            Strategy::Rwasm { module, engine } => {
                let store = match store {
                    TypedStore::Rwasm(store) => store,
                    #[allow(unreachable_patterns)]
                    _ => unreachable!(),
                };
                let mut ctx = engine.borrow_mut();
                let mut executor = ctx.create_callable_executor(store, &module);
                executor.run()
            }
            #[cfg(feature = "wasmtime")]
            Strategy::Wasmtime { .. } => {
                let store = match store {
                    TypedStore::Wasmtime(store) => store,
                    _ => unreachable!(),
                };
                store.execute(func_name, params, result)
            }
        }
    }

    pub fn resume<'a, T: 'static + Send>(
        &'a mut self,
        store: &mut TypedStore<T>,
        interruption_result: Result<&[Value], TrapCode>,
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        match self {
            Strategy::Rwasm { module, engine } => {
                let store = match store {
                    TypedStore::Rwasm(store) => store,
                    #[allow(unreachable_patterns)]
                    _ => unreachable!(),
                };
                let mut ctx = engine.borrow_mut();
                let mut executor = ctx.create_resumable_executor(store, &module);
                executor.run()
            }
            #[cfg(feature = "wasmtime")]
            Strategy::Wasmtime { .. } => {
                let store = match store {
                    TypedStore::Wasmtime(store) => store,
                    _ => unreachable!(),
                };
                store.resume(interruption_result, result)
            }
        }
    }
}
