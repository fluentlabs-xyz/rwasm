use crate::{
    Caller, CompilationConfig, ImportLinker, Store, SyscallHandler, TrapCode, TypedCaller,
    UntypedValue, ValType, Value, F32, F64, N_MAX_STACK_SIZE,
};
use alloc::rc::Rc;
use smallvec::SmallVec;
use std::{cell::RefCell, time::Instant};
use wasmtime::{AsContext, AsContextMut};

pub type WasmtimeModule = wasmtime::Module;
pub type WasmtimeLinker<T> = wasmtime::Linker<T>;

struct WrappedContext<T: Send + Sync + 'static> {
    syscall_handler: SyscallHandler<T>,
    inner: T,
    fuel: Option<u64>,
}

pub struct WasmtimeCaller<'a, T: Send + Sync + 'static> {
    caller: RefCell<wasmtime::Caller<'a, WrappedContext<T>>>,
}

pub struct WasmtimeWorker<T: 'static + Send + Sync> {
    store: RefCell<wasmtime::Store<WrappedContext<T>>>,
    instance: wasmtime::Instance,
    execution_handle: Option<wasmtime::ExecutionHandle>,
}

impl<T: 'static + Send + Sync> WasmtimeWorker<T> {
    pub fn new(
        module: Rc<wasmtime::Module>,
        import_linker: Rc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
        fuel: Option<u64>,
    ) -> Self {
        let wrapped_context = WrappedContext {
            syscall_handler,
            inner: context,
            fuel,
        };
        let mut store = wasmtime::Store::new(module.engine(), wrapped_context);
        if let Some(fuel) = fuel {
            store.set_fuel(fuel).expect("fuel should be supported");
        }
        let linker = wasmtime_import_linker(module.engine(), import_linker);
        let instance = linker
            .instantiate(&mut store, &module)
            .unwrap_or_else(|err| panic!("can't instantiate wasmtime: {}", err));

        Self {
            store: RefCell::new(store),
            instance,
            execution_handle: None,
        }
    }

    pub fn execute_not_resumable(
        &mut self,
        func_name: &'static str,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let mut store = self.store.borrow_mut();
        execute_wasmtime_module(self.instance, &mut *store, func_name, params, result)
    }

    pub fn execute(
        &mut self,
        func_name: &'static str,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        assert!(
            self.execution_handle.is_none(),
            "execution already in progress"
        );
        let mut store = self.store.borrow_mut();
        match execute_wasmtime_module(self.instance, &mut *store, func_name, params, result) {
            Ok(()) => Ok(()),
            Err(TrapCode::InterruptionCalled) => {
                self.execution_handle = Some(
                    self.instance
                        .get_execution_handle(&mut *store)
                        .expect("execution should be paused"),
                );
                Err(TrapCode::InterruptionCalled)
            }
            Err(other) => Err(other),
        }
    }

    pub fn resume(
        &mut self,
        interruption_result: Result<&[Value], TrapCode>,
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let handle = self
            .execution_handle
            .take()
            .expect("no execution to resume");
        let mut store = self.store.borrow_mut();
        match handle.resume(&mut *store) {
            Ok(wasmtime_results) => {
                for (i, val) in wasmtime_results.into_iter().enumerate() {
                    if i < result.len() {
                        result[i] = match val {
                            wasmtime::Val::I32(x) => Value::I32(x),
                            wasmtime::Val::I64(x) => Value::I64(x),
                            wasmtime::Val::F32(x) => Value::F32(F32::from_bits(x)),
                            wasmtime::Val::F64(x) => Value::F64(F64::from_bits(x)),
                            _ => unreachable!("unsupported value type"),
                        };
                    }
                }
                Ok(())
            }
            Err(trap) => {
                let trap_code = map_anyhow_error(trap.into());
                if trap_code == TrapCode::InterruptionCalled {
                    self.execution_handle = Some(
                        self.instance
                            .get_execution_handle(&mut *store)
                            .expect("execution should be paused"),
                    );
                    Err(TrapCode::InterruptionCalled)
                } else {
                    Err(trap_code)
                }
            }
        }
    }
}

impl<T: Send + Sync> Store<T> for WasmtimeWorker<T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let mut store = self.store.borrow_mut();
        let memory = self
            .instance
            .get_export(&mut *store, "memory")
            .unwrap_or_else(|| unreachable!("missing memory export"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("missing memory export"));
        memory
            .read(&*store, offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let mut store = self.store.borrow_mut();
        let memory = self
            .instance
            .get_export(&mut *store, "memory")
            .unwrap_or_else(|| unreachable!("missing memory export"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("missing memory export"));
        memory
            .write(&mut *store, offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn context_mut<R, F: FnMut(&mut T) -> R>(&mut self, mut func: F) -> R {
        let mut store = self.store.borrow_mut();
        func(&mut store.data_mut().inner)
    }

    fn context<R, F: Fn(&T) -> R>(&self, func: F) -> R {
        let store = self.store.borrow();
        func(&store.data().inner)
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        let mut store = self.store.borrow_mut();
        if let Ok(remaining_fuel) = store.get_fuel() {
            let new_fuel = remaining_fuel
                .checked_sub(delta)
                .ok_or(TrapCode::OutOfFuel)?;
            store
                .set_fuel(new_fuel)
                .unwrap_or_else(|_| unreachable!("fuel mode should be enabled"));
        } else if let Some(fuel) = store.data_mut().fuel.as_mut() {
            *fuel = fuel.checked_sub(delta).ok_or(TrapCode::OutOfFuel)?;
        }
        Ok(())
    }

    fn remaining_fuel(&mut self) -> Option<u64> {
        let store = self.store.borrow();
        if let Ok(fuel) = store.get_fuel() {
            Some(fuel)
        } else {
            store.data().fuel
        }
    }
}

impl<'a, T: Send + Sync> Store<T> for WasmtimeCaller<'a, T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let memory = self
            .caller
            .borrow_mut()
            .get_export("memory")
            .unwrap_or_else(|| unreachable!("missing memory export"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("missing memory export"));
        memory
            .read(self.caller.borrow().as_context(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let memory = self
            .caller
            .borrow_mut()
            .get_export("memory")
            .unwrap_or_else(|| unreachable!("missing memory export"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("missing memory export"));
        memory
            .write(self.caller.borrow_mut().as_context_mut(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn context_mut<R, F: FnMut(&mut T) -> R>(&mut self, mut func: F) -> R {
        func(&mut self.caller.borrow_mut().data_mut().inner)
    }

    fn context<R, F: Fn(&T) -> R>(&self, func: F) -> R {
        func(&self.caller.borrow().data().inner)
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        let mut ctx = self.caller.borrow_mut();
        if let Ok(remaining_fuel) = ctx.get_fuel() {
            let new_fuel = remaining_fuel
                .checked_sub(delta)
                .ok_or(TrapCode::OutOfFuel)?;
            ctx.set_fuel(new_fuel)
                .unwrap_or_else(|_| unreachable!("fuel mode should be enabled"));
        } else if let Some(fuel) = ctx.data_mut().fuel.as_mut() {
            *fuel = fuel.checked_sub(delta).ok_or(TrapCode::OutOfFuel)?;
        }
        Ok(())
    }

    fn remaining_fuel(&mut self) -> Option<u64> {
        let ctx = self.caller.borrow();
        if let Ok(fuel) = ctx.get_fuel() {
            Some(fuel)
        } else {
            ctx.data().fuel
        }
    }
}

impl<'a, T: Send + Sync> Caller<T> for WasmtimeCaller<'a, T> {
    fn program_counter(&self) -> u32 {
        unimplemented!("not allowed in wasmtime mode")
    }

    fn stack_push(&mut self, _value: UntypedValue) {
        unimplemented!("not allowed in wasmtime mode")
    }
}

fn wasmtime_config() -> anyhow::Result<wasmtime::Config> {
    let mut config = wasmtime::Config::new();
    // TODO(dmitry123): "make sure config is correct"
    config.strategy(wasmtime::Strategy::Cranelift);
    config.collector(wasmtime::Collector::Null);
    config.max_wasm_stack(N_MAX_STACK_SIZE * size_of::<u32>());
    // use caching for artifacts
    #[cfg(feature = "cache-compiled-artifacts")]
    {
        use directories::ProjectDirs;
        use std::path::{Path, PathBuf};
        use wasmtime::{Cache, CacheConfig};
        let project_dirs = ProjectDirs::from("com", "bytecodealliance", "wasmtime").unwrap();
        let cache_dir = project_dirs.cache_dir();
        std::fs::create_dir_all(cache_dir)?;
        let mut cache_config = CacheConfig::default();
        cache_config.with_directory(PathBuf::from(cache_dir));
        config.cache(Some(Cache::new(cache_config)?));
        // make sure the caching dir exists
        std::fs::create_dir_all(Path::new(cache_dir))?;
    }
    Ok(config)
}

pub fn deserialize_wasmtime_module(
    wasmtime_binary: impl AsRef<[u8]>,
) -> anyhow::Result<WasmtimeModule> {
    print!("parsing wasmtime module... ");
    let start = Instant::now();
    let engine = wasmtime::Engine::new(&wasmtime_config()?)?;
    let module = unsafe { wasmtime::Module::deserialize(&engine, wasmtime_binary) };
    println!("{:?}", start.elapsed());
    module
}

pub fn compile_wasmtime_module(
    _compilation_config: CompilationConfig,
    wasm_binary: impl AsRef<[u8]>,
) -> anyhow::Result<WasmtimeModule> {
    print!("compiling wasmtime module... ");
    let start = Instant::now();
    let engine = wasmtime::Engine::new(&wasmtime_config()?)?;
    let module = wasmtime::Module::new(&engine, wasm_binary);
    println!("{:?}", start.elapsed());
    module
}

fn execute_wasmtime_module<T: Send + Sync>(
    instance: wasmtime::Instance,
    store: &mut wasmtime::Store<WrappedContext<T>>,
    func_name: &'static str,
    params: &[Value],
    result: &mut [Value],
) -> Result<(), TrapCode> {
    let entrypoint = instance
        .get_func(store.as_context_mut(), func_name)
        .unwrap_or_else(|| unreachable!("missing entrypoint: {}", func_name));
    let mut buffer = SmallVec::<[wasmtime::Val; 128]>::new();
    for (i, value) in params.iter().enumerate() {
        let value = match value {
            Value::I32(value) => wasmtime::Val::I32(*value),
            Value::I64(value) => wasmtime::Val::I64(*value),
            Value::F32(value) => wasmtime::Val::F32(value.to_bits()),
            Value::F64(value) => wasmtime::Val::F64(value.to_bits()),
            _ => unreachable!("not supported type: {:?}", value),
        };
        buffer.push(value);
    }
    buffer.extend(std::iter::repeat(wasmtime::Val::I32(0)).take(result.len()));
    let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
    entrypoint
        .call(store.as_context_mut(), &mapped_params, mapped_result)
        .map_err(map_anyhow_error)
        .or_else(|trap_code| {
            if trap_code == TrapCode::ExecutionHalted {
                Ok(())
            } else {
                Err(trap_code)
            }
        })?;
    for (i, x) in mapped_result.iter().enumerate() {
        result[i] = match x {
            wasmtime::Val::I32(value) => Value::I32(*value),
            wasmtime::Val::I64(value) => Value::I64(*value),
            wasmtime::Val::F32(value) => Value::F32(F32::from_bits(*value)),
            wasmtime::Val::F64(value) => Value::F64(F64::from_bits(*value)),
            _ => unreachable!("not supported type: {:?}", x),
        };
    }
    Ok(())
}

fn wasmtime_syscall_handler<'a, T: Send + Sync + 'static>(
    sys_func_idx: u32,
    caller: wasmtime::Caller<'a, WrappedContext<T>>,
    params: &[wasmtime::Val],
    results: &mut [wasmtime::Val],
) -> anyhow::Result<()> {
    // Convert input values from wasmtime format into rwasm format
    let mut buffer = SmallVec::<[Value; 128]>::new();
    buffer.extend(params.iter().map(|x| match x {
        wasmtime::Val::I32(value) => Value::I32(*value),
        wasmtime::Val::I64(value) => Value::I64(*value),
        wasmtime::Val::F32(value) => Value::F32(F32::from_bits(*value)),
        wasmtime::Val::F64(value) => Value::F64(F64::from_bits(*value)),
        _ => unreachable!("not supported type: {:?}", x),
    }));
    buffer.extend(std::iter::repeat(Value::I32(0)).take(results.len()));

    // Create caller adapter for accessing memory and context
    let syscall_handler = caller.data().syscall_handler;
    let mut caller_adapter = TypedCaller::Wasmtime(WasmtimeCaller::<'a, T> {
        caller: RefCell::new(caller),
    });
    let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());

    // Execute the syscall
    match syscall_handler(
        &mut caller_adapter,
        sys_func_idx,
        mapped_params,
        mapped_result,
    ) {
        Ok(_) => {
            // Convert results back to wasmtime format
            for (i, value) in mapped_result.iter().enumerate() {
                results[i] = match value {
                    Value::I32(value) => wasmtime::Val::I32(*value),
                    Value::I64(value) => wasmtime::Val::I64(*value),
                    Value::F32(value) => wasmtime::Val::F32(value.to_bits()),
                    Value::F64(value) => wasmtime::Val::F64(value.to_bits()),
                    _ => unreachable!("not supported type: {:?}", value),
                };
            }
            Ok(())
        }
        Err(TrapCode::InterruptionCalled) => {
            caller_adapter
                .as_wasmtime_mut()
                .caller
                .borrow_mut()
                .pause_execution()?;
            Ok(())
        }
        Err(TrapCode::ExecutionHalted) => Err(TrapCode::ExecutionHalted.into()),
        Err(trap_code) => Err(trap_code.into()),
    }
}

fn wasmtime_import_linker<T: Send + Sync + 'static>(
    engine: &wasmtime::Engine,
    import_linker: Rc<ImportLinker>,
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

fn map_anyhow_error(err: anyhow::Error) -> TrapCode {
    if let Some(trap) = err.downcast_ref::<wasmtime::Trap>() {
        // map wasmtime trap codes into our trap codes
        use wasmtime::Trap;
        match trap {
            Trap::StackOverflow => TrapCode::StackOverflow,
            Trap::MemoryOutOfBounds => TrapCode::MemoryOutOfBounds,
            Trap::HeapMisaligned => TrapCode::MemoryOutOfBounds,
            Trap::TableOutOfBounds => TrapCode::TableOutOfBounds,
            Trap::IndirectCallToNull => TrapCode::IndirectCallToNull,
            Trap::BadSignature => TrapCode::BadSignature,
            Trap::IntegerOverflow => TrapCode::IntegerOverflow,
            Trap::IntegerDivisionByZero => TrapCode::IntegerDivisionByZero,
            Trap::BadConversionToInteger => TrapCode::BadConversionToInteger,
            Trap::UnreachableCodeReached => TrapCode::UnreachableCodeReached,
            Trap::Interrupt => unreachable!("interrupt is not supported"),
            Trap::AlwaysTrapAdapter => unreachable!("component-model is not supported"),
            Trap::OutOfFuel => TrapCode::OutOfFuel,
            Trap::AtomicWaitNonSharedMemory => {
                unreachable!("atomic extension is not supported")
            }
            Trap::NullReference => TrapCode::IndirectCallToNull,
            Trap::ArrayOutOfBounds | Trap::AllocationTooLarge => {
                unreachable!("gc is not supported")
            }
            Trap::CastFailure => TrapCode::BadConversionToInteger,
            Trap::CannotEnterComponent => unreachable!("component-model is not supported"),
            Trap::NoAsyncResult => unreachable!("async mode must be disabled"),
            Trap::PauseExecution => TrapCode::InterruptionCalled,
            _ => unreachable!("unknown trap wasmtime code"),
        }
    } else if let Some(trap) = err.downcast_ref::<TrapCode>() {
        // if our trap code is initiated, then just return the trap code
        *trap
    } else {
        // TODO(dmitry123): "what type of error to use here in case of unknown error?"
        TrapCode::IllegalOpcode
    }
}

fn map_val_type(val_type: ValType) -> wasmtime::ValType {
    match val_type {
        ValType::I32 => wasmtime::ValType::I32,
        ValType::I64 => wasmtime::ValType::I64,
        ValType::F32 => wasmtime::ValType::F32,
        ValType::F64 => wasmtime::ValType::F64,
        _ => unreachable!("not supported type: {:?}", val_type),
    }
}
