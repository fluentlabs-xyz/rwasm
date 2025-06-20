mod resumable;

use crate::{
    Caller,
    ImportLinker,
    SyscallHandler,
    TrapCode,
    UntypedValue,
    ValType,
    Value,
    F32,
    F64,
};
use alloc::rc::Rc;
use smallvec::SmallVec;
use std::cell::{Ref, RefCell, RefMut};
use wasmtime::{AsContext, AsContextMut, Collector, Strategy, Trap, Val};

struct WrappedContext<T> {
    syscall_handler: SyscallHandler<T>,
    inner: T,
}

impl<T> WrappedContext<T> {
    pub fn new(syscall_handler: SyscallHandler<T>, inner: T) -> Self {
        Self {
            syscall_handler,
            inner,
        }
    }
}

struct WrappedCaller<'a, T> {
    caller: RefCell<wasmtime::Caller<'a, WrappedContext<T>>>,
}

impl<'a, T> Caller<T> for WrappedCaller<'a, T> {
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

    fn program_counter(&self) -> u32 {
        unimplemented!("not allowed im wasmtime mode")
    }

    fn sync_stack_ptr(&mut self) {
        // there is nothing to sync...
    }

    fn context_mut(&mut self) -> RefMut<T> {
        RefMut::map(self.caller.borrow_mut(), |c| &mut c.data_mut().inner)
    }

    fn context(&self) -> Ref<T> {
        Ref::map(self.caller.borrow(), |c| &c.data().inner)
    }

    fn stack_push(&mut self, _value: UntypedValue) {
        unimplemented!("not allowed in wasmtime mode")
    }
}

/// Wasmtime module is compiled from rWasm, it means that it can have only 1 entrypoint that is
/// function "main". Function "deploy" can't be called, because AOT can be applied only for
/// precompiled trusted binaries that are gas-free or use manual gas management. It doesn't work
/// for trustless arbitrary applications like Wasm or EVM.
const WASMTIME_DEFAULT_ENTRYPOINT_NAME: &'static str = "main";

pub struct CraneliftExecutor<T> {
    instance: wasmtime::Instance,
    store: wasmtime::Store<WrappedContext<T>>,
}

impl<T: 'static> CraneliftExecutor<T> {
    pub fn compile(
        wasm_binary: impl AsRef<[u8]>,
        import_linker: Rc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
    ) -> Self {
        let mut config = wasmtime::Config::new();
        // TODO(dmitry123): "make sure config is correct"
        config.strategy(Strategy::Cranelift);
        config.collector(Collector::Null);
        let engine = wasmtime::Engine::new(&config).unwrap();
        let module =
            wasmtime::Module::new(&engine, wasm_binary).expect("failed to compile wasmtime module");
        Self::new(module, import_linker, context, syscall_handler)
    }

    pub fn new(
        module: wasmtime::Module,
        import_linker: Rc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
    ) -> Self {
        let linker = map_import_linker(module.engine(), import_linker);
        let context = WrappedContext::new(syscall_handler, context);
        let mut store = wasmtime::Store::<WrappedContext<T>>::new(module.engine(), context);
        let instance = linker
            .instantiate(store.as_context_mut(), &module)
            .unwrap_or_else(|err| panic!("can't instantiate wasmtime: {}", err));
        Self { instance, store }
    }

    pub fn run(&mut self) -> Result<(), TrapCode> {
        // entrypoint doesn't have input/output params
        self.run_typed(WASMTIME_DEFAULT_ENTRYPOINT_NAME, &[], &mut [])
    }

    pub fn run_typed(
        &mut self,
        entrypoint: &'static str,
        params: &[Val],
        results: &mut [Val],
    ) -> Result<(), TrapCode> {
        let entrypoint = self
            .instance
            .get_func(self.store.as_context_mut(), entrypoint)
            .unwrap_or_else(|| unreachable!("missing a default main entrypoint"));
        entrypoint
            .call(self.store.as_context_mut(), params, results)
            .map_err(map_anyhow_error)?;
        Ok(())
    }

    pub fn resume(&mut self, results: &[Val]) -> Result<(), TrapCode> {
        unimplemented!("not implemented yet")
    }
}

fn wasmtime_syscall_handler<'a, T>(
    sys_func_idx: u32,
    caller: wasmtime::Caller<'a, WrappedContext<T>>,
    params: &[Val],
    result: &mut [Val],
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
    buffer.extend(std::iter::repeat(Value::I32(0)).take(result.len()));
    // resolve sys func idx using import linker
    // caller.data().import_linker.resolve_by_import_name();
    // caller adapter is required to provide operations for accessing memory and context
    let syscall_handler = caller.data().syscall_handler;
    let mut caller_adapter = WrappedCaller::<'a, T> {
        caller: RefCell::new(caller),
    };
    let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
    syscall_handler(
        &mut caller_adapter,
        sys_func_idx,
        mapped_params,
        mapped_result,
    )?;
    // after call map all values back to wasmtime format
    for (i, value) in mapped_result.iter().enumerate() {
        let value = match value {
            Value::I32(value) => Val::I32(*value),
            Value::I64(value) => Val::I64(*value),
            Value::F32(value) => Val::F32(value.to_bits()),
            Value::F64(value) => Val::F64(value.to_bits()),
            _ => unreachable!("not supported type: {:?}", value),
        };
        result[i] = value;
    }
    Ok(())
}

fn map_import_linker<T>(
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
    if let Some(trap) = err.downcast_ref::<Trap>() {
        // map wasmtime trap codes into our trap codes
        map_trap_code(trap)
    } else if let Some(trap) = err.downcast_ref::<TrapCode>() {
        // if our trap code is initiated, then just return the trap code
        *trap
    } else {
        // TODO(dmitry123): "what type of error to use here in case of unknown error?"
        TrapCode::IllegalOpcode
    }
}

fn map_trap_code(trap: &Trap) -> TrapCode {
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
