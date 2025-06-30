use crate::{
    Caller,
    ImportLinker,
    Store,
    SyscallHandler,
    TrapCode,
    TypedCaller,
    UntypedValue,
    ValType,
    Value,
    F32,
    F64,
    N_MAX_STACK_SIZE,
};
use alloc::rc::Rc;
use directories::ProjectDirs;
use smallvec::{smallvec, SmallVec};
use std::{
    cell::RefCell,
    marker::PhantomData,
    panic,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, RwLock},
    thread,
    time::Instant,
};
use wasmtime::{AsContext, AsContextMut, Cache, CacheConfig};

pub type WasmtimeModule = wasmtime::Module;
pub type WasmtimeLinker<T> = wasmtime::Linker<T>;

struct WrappedContext<T: Send + Sync> {
    syscall_handler: SyscallHandler<T>,
    inner: Option<T>,
    message_channel: Option<mpsc::Sender<MessageResponse<T>>>,
    fuel: Option<u64>,
}

pub struct WasmtimeCaller<'a, T: Send + Sync> {
    caller: RefCell<wasmtime::Caller<'a, WrappedContext<T>>>,
}

enum MessageResponse<T: Send + Sync> {
    ExecutionResult {
        result: Result<SmallVec<[Value; 16]>, TrapCode>,
    },
    InterruptedCall {
        resp: mpsc::Sender<MessageInterruptionResult<T>>,
        stolen_context: StolenContext<T>,
    },
}

struct MessageRequest<T: Send + Sync> {
    func_name: &'static str,
    params: SmallVec<[Value; 16]>,
    num_result: usize,
    resp: mpsc::Sender<MessageResponse<T>>,
}

struct MessageInterruptionResult<T: Send + Sync> {
    stolen_context: StolenContext<T>,
    result: Result<SmallVec<[Value; 16]>, TrapCode>,
    memory_changes: Vec<(u32, Box<[u8]>)>,
}

struct StolenContext<T: Send + Sync> {
    context: T,
    fuel: Option<u64>,
}

pub struct WasmtimeWorker<T: 'static + Send + Sync> {
    sender: mpsc::Sender<MessageRequest<T>>,
    interrupt_channel: Option<mpsc::Sender<MessageInterruptionResult<T>>>,
    marker: PhantomData<T>,
    recv_channel: Option<mpsc::Receiver<MessageResponse<T>>>,
    store: Arc<RwLock<wasmtime::Store<WrappedContext<T>>>>,
    instance: wasmtime::Instance,
    stolen_context: Option<StolenContext<T>>,
}

impl<T: 'static + Send + Sync> WasmtimeWorker<T> {
    pub fn new(
        module: Rc<wasmtime::Module>,
        import_linker: Rc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
        fuel: Option<u64>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let context = WrappedContext {
            syscall_handler,
            inner: Some(context),
            message_channel: None,
            fuel,
        };
        let store = Arc::new(RwLock::new(wasmtime::Store::<WrappedContext<T>>::new(
            module.engine(),
            context,
        )));
        let linker = wasmtime_import_linker(module.engine(), import_linker);
        let instance = linker
            .instantiate(store.write().unwrap().as_context_mut(), &module)
            .unwrap_or_else(|err| panic!("can't instantiate wasmtime: {}", err));
        let moved_store = store.clone();
        let moved_instance = instance.clone();
        thread::spawn(move || {
            let store = moved_store;
            let instance = moved_instance;
            while let Ok(message) = receiver.recv() {
                let mut store = store.write().unwrap();
                let MessageRequest {
                    func_name,
                    params,
                    num_result,
                    resp,
                } = message;
                let mut result: SmallVec<[Value; 16]> = smallvec![Value::I32(0); num_result];
                store.data_mut().message_channel = Some(resp);
                let result =
                    execute_wasmtime_module(instance, &mut store, func_name, &params, &mut result)
                        .map(|_| result);
                let resp = store.data_mut().message_channel.take().unwrap();
                resp.send(MessageResponse::ExecutionResult { result })
                    .unwrap();
            }
        });
        Self {
            sender,
            interrupt_channel: None,
            marker: Default::default(),
            recv_channel: None,
            store,
            stolen_context: None,
            instance,
        }
    }

    pub fn execute(
        &mut self,
        func_name: &'static str,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let (resp_tx, resp_rx) = mpsc::channel();
        debug_assert!(
            self.interrupt_channel.is_none(),
            "the resumable flag must not be set"
        );
        self.sender
            .send(MessageRequest {
                func_name,
                params: params.into(),
                num_result: result.len(),
                resp: resp_tx,
            })
            .unwrap();
        let response = resp_rx.recv().unwrap();
        match response {
            MessageResponse::ExecutionResult {
                result: exec_result,
            } => {
                for (i, x) in exec_result?.into_iter().enumerate() {
                    result[i] = x;
                }
                Ok(())
            }
            MessageResponse::InterruptedCall {
                resp,
                stolen_context,
            } => {
                self.interrupt_channel = Some(resp);
                self.recv_channel = Some(resp_rx);
                self.stolen_context = Some(stolen_context);
                Err(TrapCode::InterruptionCalled)
            }
        }
    }

    pub fn resume(
        &mut self,
        interruption_result: Result<&[Value], TrapCode>,
        result: &mut [Value],
        memory_changes: Vec<(u32, Box<[u8]>)>,
    ) -> Result<(), TrapCode> {
        let channel = self
            .interrupt_channel
            .take()
            .expect("the resumable flag must not be set");
        let interruption_result =
            interruption_result.map(|values| SmallVec::<[Value; 16]>::from(values));
        channel
            .send(MessageInterruptionResult {
                // stolen must be obtained during the execution
                stolen_context: self.stolen_context.take().unwrap(),
                result: interruption_result,
                memory_changes,
            })
            .unwrap();
        let response = self.recv_channel.as_ref().unwrap().recv().unwrap();
        match response {
            MessageResponse::ExecutionResult {
                result: exec_result,
            } => {
                for (i, x) in exec_result?.into_iter().enumerate() {
                    result[i] = x;
                }
                Ok(())
            }
            MessageResponse::InterruptedCall {
                resp,
                stolen_context,
            } => {
                self.interrupt_channel = Some(resp);
                // recv_channel is already assigned during execution
                self.stolen_context = Some(stolen_context);
                Err(TrapCode::InterruptionCalled)
            }
        }
    }
}

impl<T: Send + Sync> Store<T> for WasmtimeWorker<T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let mut store = self.store.write().unwrap();
        let global_memory = self
            .instance
            .get_export(store.as_context_mut(), "memory")
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"));
        global_memory
            .read(store.as_context(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let mut store = self.store.write().unwrap();
        let global_memory = self
            .instance
            .get_export(store.as_context_mut(), "memory")
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"));
        global_memory
            .write(store.as_context_mut(), offset, &buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn context_mut<R, F: FnMut(&mut T) -> R>(&mut self, mut func: F) -> R {
        if let Some(stolen_context) = self.stolen_context.as_mut() {
            return func(&mut stolen_context.context);
        }
        let mut store = self.store.write().unwrap();
        func(store.data_mut().inner.as_mut().unwrap())
    }

    fn context<R, F: Fn(&T) -> R>(&self, func: F) -> R {
        if let Some(stolen_context) = self.stolen_context.as_ref() {
            return func(&stolen_context.context);
        }
        let store = self.store.read().unwrap();
        func(store.data().inner.as_ref().unwrap())
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        if let Some(stolen_context) = self.stolen_context.as_mut() {
            if let Some(fuel) = stolen_context.fuel {
                let new_fuel = fuel.checked_sub(delta).ok_or(TrapCode::OutOfFuel)?;
                stolen_context.fuel = Some(new_fuel);
            }
            return Ok(());
        }
        let mut store = self.store.write().unwrap();
        if let Ok(remaining_fuel) = store.get_fuel() {
            let new_fuel = remaining_fuel
                .checked_sub(delta)
                .ok_or(TrapCode::OutOfFuel)?;
            store
                .set_fuel(new_fuel)
                .unwrap_or_else(|_| unreachable!("fuel mode is disabled in wasmtime"));
        } else if let Some(fuel) = store.data_mut().fuel.as_mut() {
            *fuel = fuel.checked_sub(delta).ok_or(TrapCode::OutOfFuel)?;
        }
        Ok(())
    }

    fn remaining_fuel(&mut self) -> Option<u64> {
        if let Some(stolen_context) = self.stolen_context.as_ref() {
            return stolen_context.fuel;
        }
        let store = self.store.read().unwrap();
        if let Ok(fuel) = store.get_fuel() {
            Some(fuel)
        } else if let Some(fuel) = store.data().fuel.as_ref() {
            Some(*fuel)
        } else {
            None
        }
    }
}

impl<'a, T: Send + Sync> Store<T> for WasmtimeCaller<'a, T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let global_memory = self
            .caller
            .borrow_mut()
            .get_export("memory")
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"));
        global_memory
            .read(self.caller.borrow().as_context(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let global_memory = self
            .caller
            .borrow_mut()
            .get_export("memory")
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"))
            .into_memory()
            .unwrap_or_else(|| unreachable!("missing memory export, it's not possible"));
        global_memory
            .write(self.caller.borrow_mut().as_context_mut(), offset, buffer)
            .map_err(|_| TrapCode::MemoryOutOfBounds)
    }

    fn context_mut<R, F: FnMut(&mut T) -> R>(&mut self, mut func: F) -> R {
        func(self.caller.borrow_mut().data_mut().inner.as_mut().unwrap())
    }

    fn context<R, F: Fn(&T) -> R>(&self, func: F) -> R {
        func(self.caller.borrow_mut().data().inner.as_ref().unwrap())
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        let mut ctx = self.caller.borrow_mut();
        if let Ok(remaining_fuel) = ctx.get_fuel() {
            let new_fuel = remaining_fuel
                .checked_sub(delta)
                .ok_or(TrapCode::OutOfFuel)?;
            ctx.set_fuel(new_fuel)
                .unwrap_or_else(|_| unreachable!("fuel mode is disabled in wasmtime"));
        } else if let Some(fuel) = ctx.data_mut().fuel.as_mut() {
            *fuel = fuel.checked_sub(delta).ok_or(TrapCode::OutOfFuel)?;
        }
        Ok(())
    }

    fn remaining_fuel(&mut self) -> Option<u64> {
        let ctx = self.caller.borrow();
        // TODO(dmitry123): "do we want to deal with wasmtime's fuel?"
        if let Ok(fuel) = ctx.get_fuel() {
            Some(fuel)
        } else if let Some(fuel) = ctx.data().fuel.as_ref() {
            Some(*fuel)
        } else {
            None
        }
    }
}

impl<'a, T: Send + Sync> Caller<T> for WasmtimeCaller<'a, T> {
    fn program_counter(&self) -> u32 {
        unimplemented!("not allowed im wasmtime mode")
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
    let project_dirs = ProjectDirs::from("com", "bytecodealliance", "wasmtime").unwrap();
    let cache_dir = project_dirs.cache_dir();
    std::fs::create_dir_all(cache_dir)?;
    let mut cache_config = CacheConfig::default();
    cache_config.with_directory(PathBuf::from(cache_dir));
    config.cache(Some(Cache::new(cache_config)?));
    // make sure the caching dir exists
    std::fs::create_dir_all(Path::new(cache_dir))?;
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

pub fn compile_wasmtime_module(wasm_binary: impl AsRef<[u8]>) -> anyhow::Result<WasmtimeModule> {
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
    // convert input values from wasmtime format into rwasm format
    let mut buffer = SmallVec::<[Value; 128]>::new();
    buffer.extend(params.iter().map(|x| match x {
        wasmtime::Val::I32(value) => Value::I32(*value),
        wasmtime::Val::I64(value) => Value::I64(*value),
        wasmtime::Val::F32(value) => Value::F32(F32::from_bits(*value)),
        wasmtime::Val::F64(value) => Value::F64(F64::from_bits(*value)),
        _ => unreachable!("not supported type: {:?}", x),
    }));
    buffer.extend(std::iter::repeat(Value::I32(0)).take(results.len()));
    // caller adapter is required to provide operations for accessing memory and context
    let syscall_handler = caller.data().syscall_handler;
    let mut caller_adapter = TypedCaller::Wasmtime(WasmtimeCaller::<'a, T> {
        caller: RefCell::new(caller),
    });
    let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
    let syscall_result = syscall_handler(
        &mut caller_adapter,
        sys_func_idx,
        mapped_params,
        mapped_result,
    );
    if let Some(TrapCode::InterruptionCalled) = syscall_result.err() {
        let (resp_tx, resp_rx) = mpsc::channel();
        let mut caller_ctx = caller_adapter.as_wasmtime_mut().caller.borrow_mut();
        let stolen_context = StolenContext {
            context: caller_ctx.data_mut().inner.take().unwrap(),
            fuel: caller_ctx
                .get_fuel()
                .ok()
                .or_else(|| caller_ctx.data_mut().fuel),
        };
        caller_ctx
            .data()
            .message_channel
            .as_ref()
            .unwrap()
            .send(MessageResponse::InterruptedCall {
                resp: resp_tx,
                stolen_context,
            })
            .expect("failed to send a message to the host thread");
        let interruption_result = resp_rx.recv().expect("failed to receive a response");
        let StolenContext {
            context: stolen_context,
            fuel: stolen_fuel,
        } = interruption_result.stolen_context;
        if let Some(stolen_fuel) = stolen_fuel {
            caller_ctx.set_fuel(stolen_fuel).unwrap_or_else(|_| {
                // we don't care about the trap here because we have a fallback
                caller_ctx.data_mut().fuel = Some(stolen_fuel);
            });
        }
        caller_ctx.data_mut().inner.replace(stolen_context);
        drop(caller_ctx);
        for (i, value) in interruption_result.result?.iter().enumerate() {
            results[i] = match value {
                Value::I32(value) => wasmtime::Val::I32(*value),
                Value::I64(value) => wasmtime::Val::I64(*value),
                Value::F32(value) => wasmtime::Val::F32(value.to_bits()),
                Value::F64(value) => wasmtime::Val::F64(value.to_bits()),
                _ => unreachable!("not supported type: {:?}", value),
            };
        }
        for (addr, buf) in interruption_result.memory_changes {
            caller_adapter.memory_write(addr as usize, &buf)?
        }
    } else {
        // make sure a syscall result is successful
        let should_terminate = syscall_result.map(|_| false).or_else(|trap_code| {
            // if syscall returns execution halted then don't return this trap code since it's a
            // successful error code
            if trap_code == TrapCode::ExecutionHalted {
                Ok(true)
            } else {
                Err(trap_code)
            }
        })?;
        // after call map all values back to wasmtime format
        for (i, value) in mapped_result.iter().enumerate() {
            results[i] = match value {
                Value::I32(value) => wasmtime::Val::I32(*value),
                Value::I64(value) => wasmtime::Val::I64(*value),
                Value::F32(value) => wasmtime::Val::F32(value.to_bits()),
                Value::F64(value) => wasmtime::Val::F64(value.to_bits()),
                _ => unreachable!("not supported type: {:?}", value),
            };
        }
        // terminate execution if required
        if should_terminate {
            return Err(TrapCode::ExecutionHalted.into());
        }
    }
    Ok(())
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
