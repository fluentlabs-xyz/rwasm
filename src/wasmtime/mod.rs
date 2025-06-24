use crate::{
    Caller,
    ImportLinker,
    Store,
    SyscallHandler,
    TrapCode,
    UntypedValue,
    ValType,
    Value,
    F32,
    F64,
};
use smallvec::{smallvec, SmallVec};
use std::{
    cell::{Ref, RefCell, RefMut},
    marker::PhantomData,
    sync::{mpsc, Arc},
    thread,
};
use wasmtime::{AsContext, AsContextMut, Collector, Strategy, Trap, Val};

pub type WasmtimeModule = wasmtime::Module;
pub type WasmtimeLinker<T> = wasmtime::Linker<T>;

struct WrappedContext<T: Send> {
    syscall_handler: SyscallHandler<T>,
    inner: T,
    message_channel: Option<mpsc::Sender<MessageResponse>>,
}

struct WrappedCaller<'a, T: Send> {
    caller: RefCell<wasmtime::Caller<'a, WrappedContext<T>>>,
}

enum MessageResponse {
    ExecutionResult {
        result: Result<SmallVec<[Value; 16]>, TrapCode>,
    },
    InterruptedCall {
        resp: mpsc::Sender<Result<SmallVec<[Value; 16]>, TrapCode>>,
    },
}

enum MessageRequest {
    ExecuteFunc {
        func_name: &'static str,
        params: SmallVec<[Value; 16]>,
        num_result: usize,
        resp: mpsc::Sender<MessageResponse>,
    },
    MemoryRead {
        offset: usize,
        buffer_size: usize,
        resp: mpsc::Sender<Result<Vec<u8>, TrapCode>>,
    },
    MemoryWrite {
        offset: usize,
        buffer: Vec<u8>,
        resp: mpsc::Sender<Result<(), TrapCode>>,
    },
    TryConsumeFuel {
        delta: u64,
        resp: mpsc::Sender<Result<(), TrapCode>>,
    },
    RemainingFuel {
        resp: mpsc::Sender<Option<u64>>,
    },
}

pub struct WasmtimeWorker<T: 'static + Send> {
    sender: mpsc::Sender<MessageRequest>,
    interrupt_channel: Option<mpsc::Sender<Result<SmallVec<[Value; 16]>, TrapCode>>>,
    marker: PhantomData<T>,
    recv_channel: Option<mpsc::Receiver<MessageResponse>>,
}

impl<T: 'static + Send> WasmtimeWorker<T> {
    pub fn new(
        module: Arc<wasmtime::Module>,
        import_linker: Arc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let context = WrappedContext {
                syscall_handler,
                inner: context,
                message_channel: None,
            };
            let mut store = wasmtime::Store::<WrappedContext<T>>::new(module.engine(), context);
            let linker = wasmtime_import_linker(module.engine(), import_linker);
            let instance = linker
                .instantiate(store.as_context_mut(), &module)
                .unwrap_or_else(|err| panic!("can't instantiate wasmtime: {}", err));
            while let Ok(message) = receiver.recv() {
                match message {
                    MessageRequest::ExecuteFunc {
                        func_name,
                        params,
                        num_result,
                        resp,
                    } => {
                        let mut result: SmallVec<[Value; 16]> =
                            smallvec![Value::I32(0); num_result];
                        store.data_mut().message_channel = Some(resp);
                        let result = execute_wasmtime_module_inner(
                            instance,
                            &mut store,
                            func_name,
                            &params,
                            &mut result,
                        )
                        .map(|_| result);
                        let resp = store.data_mut().message_channel.take().unwrap();
                        resp.send(MessageResponse::ExecutionResult { result })
                            .unwrap();
                    }
                    MessageRequest::MemoryRead {
                        offset,
                        buffer_size,
                        resp,
                    } => {
                        let global_memory = instance
                            .get_export(store.as_context_mut(), "memory")
                            .unwrap_or_else(|| {
                                unreachable!("missing memory export, it's not possible")
                            })
                            .into_memory()
                            .unwrap_or_else(|| {
                                unreachable!("missing memory export, it's not possible")
                            });
                        let mut buffer = vec![0; buffer_size];
                        let result = global_memory
                            .read(store.as_context(), offset, &mut buffer)
                            .map_err(|_| TrapCode::MemoryOutOfBounds)
                            .map(|_| buffer);
                        resp.send(result).unwrap();
                    }
                    MessageRequest::MemoryWrite {
                        offset,
                        buffer,
                        resp,
                    } => {
                        let global_memory = instance
                            .get_export(store.as_context_mut(), "memory")
                            .unwrap_or_else(|| {
                                unreachable!("missing memory export, it's not possible")
                            })
                            .into_memory()
                            .unwrap_or_else(|| {
                                unreachable!("missing memory export, it's not possible")
                            });
                        let result = global_memory
                            .write(store.as_context_mut(), offset, &buffer)
                            .map_err(|_| TrapCode::MemoryOutOfBounds);
                        resp.send(result).unwrap();
                    }
                    MessageRequest::TryConsumeFuel { delta, resp } => {
                        let remaining_fuel = store
                            .get_fuel()
                            .unwrap_or_else(|_| unreachable!("fuel mode is disabled in wasmtime"));
                        let result = if let Some(new_fuel) = remaining_fuel.checked_sub(delta) {
                            store.set_fuel(new_fuel).unwrap_or_else(|_| {
                                unreachable!("fuel mode is disabled in wasmtime")
                            });
                            Ok(())
                        } else {
                            Err(TrapCode::OutOfFuel)
                        };
                        resp.send(result).unwrap();
                    }
                    MessageRequest::RemainingFuel { resp } => {
                        let fuel_remaining = store.get_fuel().ok();
                        resp.send(fuel_remaining).unwrap();
                    }
                }
            }
        });
        Self {
            sender,
            interrupt_channel: None,
            marker: Default::default(),
            recv_channel: None,
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
            "resumable flag must not be set"
        );
        self.sender
            .send(MessageRequest::ExecuteFunc {
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
            MessageResponse::InterruptedCall { resp } => {
                self.interrupt_channel = Some(resp);
                self.recv_channel = Some(resp_rx);
                Err(TrapCode::InterruptionCalled)
            }
        }
    }

    pub fn resume(
        &mut self,
        interruption_result: Result<&[Value], TrapCode>,
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let channel = self
            .interrupt_channel
            .take()
            .expect("resumable flag must not be set");
        let interruption_result =
            interruption_result.map(|values| SmallVec::<[Value; 16]>::from(values));
        channel.send(interruption_result).unwrap();
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
            MessageResponse::InterruptedCall { resp } => {
                self.interrupt_channel = Some(resp);
                Err(TrapCode::InterruptionCalled)
            }
        }
    }
}

impl<T: Send> Store<T> for WasmtimeWorker<T> {
    fn memory_read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let (resp_tx, resp_rx) = mpsc::channel();
        self.sender
            .send(MessageRequest::MemoryRead {
                offset,
                buffer_size: buffer.len(),
                resp: resp_tx,
            })
            .unwrap();
        let result = resp_rx.recv().unwrap()?;
        debug_assert_eq!(result.len(), buffer.len());
        buffer.copy_from_slice(result.as_slice());
        Ok(())
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let (resp_tx, resp_rx) = mpsc::channel();
        self.sender
            .send(MessageRequest::MemoryWrite {
                offset,
                buffer: buffer.to_vec(),
                resp: resp_tx,
            })
            .unwrap();
        resp_rx.recv().unwrap()
    }

    fn context_mut(&mut self) -> RefMut<'_, T> {
        unimplemented!("not implemented for wasmtime resumable mode")
    }

    fn context(&self) -> Ref<'_, T> {
        unimplemented!("not implemented for wasmtime resumable mode")
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        let (resp_tx, resp_rx) = mpsc::channel();
        self.sender
            .send(MessageRequest::TryConsumeFuel {
                delta,
                resp: resp_tx,
            })
            .unwrap();
        resp_rx.recv().unwrap()
    }

    fn remaining_fuel(&mut self) -> Option<u64> {
        let (resp_tx, resp_rx) = mpsc::channel();
        self.sender
            .send(MessageRequest::RemainingFuel { resp: resp_tx })
            .unwrap();
        resp_rx.recv().unwrap()
    }
}

impl<'a, T: Send> Store<T> for WrappedCaller<'a, T> {
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

    fn context_mut(&mut self) -> RefMut<'_, T> {
        RefMut::map(self.caller.borrow_mut(), |c| &mut c.data_mut().inner)
    }

    fn context(&self) -> Ref<'_, T> {
        Ref::map(self.caller.borrow(), |c| &c.data().inner)
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        let ctx = self.caller.borrow_mut();
        let remaining_fuel = ctx
            .get_fuel()
            .unwrap_or_else(|_| unreachable!("fuel mode is disabled in wasmtime"));
        let new_fuel = remaining_fuel
            .checked_sub(delta)
            .ok_or(TrapCode::OutOfFuel)?;
        self.caller
            .borrow_mut()
            .set_fuel(new_fuel)
            .unwrap_or_else(|_| unreachable!("fuel mode is disabled in wasmtime"));
        Ok(())
    }

    fn remaining_fuel(&mut self) -> Option<u64> {
        self.caller.borrow().get_fuel().ok()
    }
}

impl<'a, T: Send> Caller<T> for WrappedCaller<'a, T> {
    fn program_counter(&self) -> u32 {
        unimplemented!("not allowed im wasmtime mode")
    }

    fn sync_stack_ptr(&mut self) {
        // there is nothing to sync...
    }

    fn stack_push(&mut self, _value: UntypedValue) {
        unimplemented!("not allowed in wasmtime mode")
    }
}

pub fn compile_wasmtime_module(wasm_binary: impl AsRef<[u8]>) -> anyhow::Result<WasmtimeModule> {
    let mut config = wasmtime::Config::new();
    // TODO(dmitry123): "make sure config is correct"
    config.strategy(Strategy::Cranelift);
    config.collector(Collector::Null);
    let engine = wasmtime::Engine::new(&config)?;
    wasmtime::Module::new(&engine, wasm_binary)
}

fn execute_wasmtime_module_inner<T: Send>(
    instance: wasmtime::Instance,
    store: &mut wasmtime::Store<WrappedContext<T>>,
    func_name: &'static str,
    params: &[Value],
    result: &mut [Value],
) -> Result<(), TrapCode> {
    let entrypoint = instance
        .get_func(store.as_context_mut(), func_name)
        .unwrap_or_else(|| unreachable!("missing entrypoint: {}", func_name));
    let mut buffer = SmallVec::<[Val; 128]>::new();
    for (i, value) in params.iter().enumerate() {
        let value = match value {
            Value::I32(value) => Val::I32(*value),
            Value::I64(value) => Val::I64(*value),
            Value::F32(value) => Val::F32(value.to_bits()),
            Value::F64(value) => Val::F64(value.to_bits()),
            _ => unreachable!("not supported type: {:?}", value),
        };
        buffer.push(value);
    }
    buffer.extend(std::iter::repeat(Val::I32(0)).take(result.len()));
    let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
    entrypoint
        .call(store.as_context_mut(), &mapped_params, mapped_result)
        .map_err(map_anyhow_error)?;
    for (i, x) in mapped_result.iter().enumerate() {
        result[i] = match x {
            Val::I32(value) => Value::I32(*value),
            Val::I64(value) => Value::I64(*value),
            Val::F32(value) => Value::F32(F32::from_bits(*value)),
            Val::F64(value) => Value::F64(F64::from_bits(*value)),
            _ => unreachable!("not supported type: {:?}", x),
        };
    }
    Ok(())
}

fn wasmtime_syscall_handler<'a, T: Send + 'static>(
    sys_func_idx: u32,
    caller: wasmtime::Caller<'a, WrappedContext<T>>,
    params: &[Val],
    results: &mut [Val],
) -> anyhow::Result<()> {
    // convert input values from wasmtime format into rwasm format
    let mut buffer = SmallVec::<[Value; 128]>::new();
    buffer.extend(params.iter().map(|x| match x {
        Val::I32(value) => Value::I32(*value),
        Val::I64(value) => Value::I64(*value),
        Val::F32(value) => Value::F32(F32::from_bits(*value)),
        Val::F64(value) => Value::F64(F64::from_bits(*value)),
        _ => unreachable!("not supported type: {:?}", x),
    }));
    buffer.extend(std::iter::repeat(Value::I32(0)).take(results.len()));
    // caller adapter is required to provide operations for accessing memory and context
    let syscall_handler = caller.data().syscall_handler;
    let mut caller_adapter = WrappedCaller::<'a, T> {
        caller: RefCell::new(caller),
    };
    let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
    let syscall_result = syscall_handler(
        &mut caller_adapter,
        sys_func_idx,
        mapped_params,
        mapped_result,
    );
    if let Some(TrapCode::InterruptionCalled) = syscall_result.err() {
        let caller = caller_adapter.caller.borrow();
        let (resp_tx, resp_rx) = mpsc::channel();
        caller
            .data()
            .message_channel
            .as_ref()
            .unwrap()
            .send(MessageResponse::InterruptedCall { resp: resp_tx })
            .expect("failed to send message to host thread");
        let interruption_result = resp_rx.recv().expect("failed to receive response");
        for (i, value) in interruption_result?.into_iter().enumerate() {
            let value = match value {
                Value::I32(value) => Val::I32(value),
                Value::I64(value) => Val::I64(value),
                Value::F32(value) => Val::F32(value.to_bits()),
                Value::F64(value) => Val::F64(value.to_bits()),
                _ => unreachable!("not supported type: {:?}", value),
            };
            results[i] = value;
        }
    } else {
        // make sure syscall result is successful
        syscall_result?;
        // after call map all values back to wasmtime format
        for (i, value) in mapped_result.iter().enumerate() {
            let value = match value {
                Value::I32(value) => Val::I32(*value),
                Value::I64(value) => Val::I64(*value),
                Value::F32(value) => Val::F32(value.to_bits()),
                Value::F64(value) => Val::F64(value.to_bits()),
                _ => unreachable!("not supported type: {:?}", value),
            };
            results[i] = value;
        }
    }
    Ok(())
}

fn wasmtime_import_linker<T: Send + 'static>(
    engine: &wasmtime::Engine,
    import_linker: Arc<ImportLinker>,
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
    if let Some(trap) = err.downcast_ref::<Trap>() {
        // map wasmtime trap codes into our trap codes
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
