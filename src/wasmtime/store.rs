use crate::{
    wasmtime::{types::map_anyhow_error, wasmtime_import_linker, WrappedContext},
    FuelConfig, ImportLinker, SyscallHandler, TrapCode, Value, F32, F64,
};
use std::sync::Arc;
use wasmtime::{
    AsContext, AsContextMut, Instance, InstancePre, Linker, Store, StoreLimitsBuilder, Val,
};

pub struct WasmtimeStore<T: 'static + Send + Sync> {
    pub linker: Linker<WrappedContext<T>>,
    pub store: Store<WrappedContext<T>>,
    pub instance_pre: InstancePre<WrappedContext<T>>,
    pub instance: Instance,
}

impl<T: 'static + Send + Sync> WasmtimeStore<T> {
    pub fn new(
        module: wasmtime::Module,
        import_linker: Arc<ImportLinker>,
        data: T,
        syscall_handler: SyscallHandler<T>,
        fuel_config: FuelConfig,
    ) -> Self {
        let resource_limiter = StoreLimitsBuilder::new()
            .instances(usize::MAX)
            .tables(usize::MAX)
            .memories(usize::MAX)
            .build();

        let context = WrappedContext {
            syscall_handler,
            fuel: None,
            resource_limiter,
            data,
        };
        let mut store = Store::<WrappedContext<T>>::new(module.engine(), context);
        store.limiter(|ctx| &mut ctx.resource_limiter);
        if let Some(fuel) = fuel_config.fuel_limit {
            if let Ok(_) = store.get_fuel() {
                store.set_fuel(fuel).expect("wasmtime: fuel is not enabled");
            } else {
                store.data_mut().fuel = Some(fuel);
            }
        }
        #[allow(unused_mut)]
        let mut linker = wasmtime_import_linker(module.engine(), import_linker);
        #[cfg(feature = "e2e")]
        Self::link_spectest_globals(&mut linker, &mut store);
        let instance_pre = linker
            .instantiate_pre(&module)
            .unwrap_or_else(|err| panic!("wasmtime: can't pre-instantiate module: {}", err));
        let instance = instance_pre
            .instantiate(store.as_context_mut())
            .unwrap_or_else(|err| panic!("wasmtime: can't instantiate module: {}", err));
        Self {
            linker,
            store,
            instance_pre,
            instance,
        }
    }

    #[cfg(feature = "e2e")]
    fn link_spectest_globals(
        linker: &mut Linker<WrappedContext<T>>,
        store: &mut Store<WrappedContext<T>>,
    ) {
        use wasmtime::{Extern, Global, GlobalType, Mutability, ValType};
        let global = Extern::Global(
            Global::new(
                store.as_context_mut(),
                GlobalType::new(ValType::I32, Mutability::Const),
                Val::I32(666),
            )
            .unwrap(),
        );
        linker
            .define(store.as_context_mut(), "spectest", "global_i32", global)
            .unwrap();
        let global = Extern::Global(
            Global::new(
                store.as_context_mut(),
                GlobalType::new(ValType::I64, Mutability::Const),
                Val::I64(666),
            )
            .unwrap(),
        );
        linker
            .define(store.as_context_mut(), "spectest", "global_i64", global)
            .unwrap();
    }

    pub fn execute(
        &mut self,
        func_name: &str,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let entrypoint = self
            .instance
            .get_func(self.store.as_context_mut(), func_name)
            .unwrap_or_else(|| unreachable!("wasmtime: missing entrypoint: {}", func_name));
        let mut buffer = Vec::<Val>::default();
        for (i, value) in params.iter().enumerate() {
            let value = match value {
                Value::I32(value) => Val::I32(*value),
                Value::I64(value) => Val::I64(*value),
                Value::F32(value) => Val::F32(value.to_bits()),
                Value::F64(value) => Val::F64(value.to_bits()),
                #[cfg(feature = "e2e")]
                Value::FuncRef(value) => Val::FuncRef(None),
                #[cfg(feature = "e2e")]
                Value::ExternRef(value) => {
                    let func_idx = value.0;
                    if func_idx == 0 {
                        Val::ExternRef(None)
                    } else {
                        Val::ExternRef(wasmtime::ExternRef::new(&mut self.store, func_idx).ok())
                    }
                }
                // this should never happen because rWasm rejects such binaries during compilation
                #[allow(unreachable_patterns)]
                _ => unreachable!("wasmtime: not supported type: {:?}", value),
            };
            buffer.push(value);
        }
        buffer.extend(std::iter::repeat(Val::I32(0)).take(result.len()));
        let (mapped_params, mapped_result) = buffer.split_at_mut(params.len());
        let exec_result = entrypoint
            .call(self.store.as_context_mut(), mapped_params, mapped_result)
            .map_err(map_anyhow_error)
            .or_else(|trap_code| {
                if trap_code == TrapCode::ExecutionHalted {
                    Ok(())
                } else {
                    Err(trap_code)
                }
            });
        if exec_result.is_err() {
            return exec_result;
        }
        for (i, x) in mapped_result.iter().cloned().enumerate() {
            result[i] = match x {
                Val::I32(value) => Value::I32(value),
                Val::I64(value) => Value::I64(value),
                Val::F32(value) => Value::F32(F32::from_bits(value)),
                Val::F64(value) => Value::F64(F64::from_bits(value)),
                #[cfg(feature = "e2e")]
                Val::FuncRef(value) => Value::FuncRef(crate::FuncRef::new(0)),
                #[cfg(feature = "e2e")]
                Val::ExternRef(value) => {
                    let value: Option<&u32> = value
                        .and_then(|ext_ref| ext_ref.data(&mut self.store).ok().flatten())
                        .and_then(|v| v.downcast_ref());
                    Value::ExternRef(crate::ExternRef::new(value.map(|v| *v).unwrap_or_default()))
                }
                _ => unreachable!("wasmtime: not supported type: {:?}", x),
            };
        }
        exec_result
    }

    pub fn resume(
        &mut self,
        interruption_result: Result<&[Value], TrapCode>,
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        unimplemented!("wasmtime: resume is not implemented yet");
    }

    fn with_store_mut<R, F: FnOnce(&mut Store<WrappedContext<T>>) -> R>(&mut self, f: F) -> R {
        f(&mut self.store)
    }

    fn with_store<R, F: FnOnce(&Store<WrappedContext<T>>) -> R>(&self, f: F) -> R {
        f(&self.store)
    }
}

impl<T: Send + Sync> crate::Store<T> for WasmtimeStore<T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        let instance = self.instance.clone();
        self.with_store_mut(|store| {
            let global_memory = instance
                .get_export(store.as_context_mut(), "memory")
                .unwrap_or_else(|| {
                    unreachable!("wasmtime: missing memory export, it's not possible")
                })
                .into_memory()
                .unwrap_or_else(|| {
                    unreachable!("wasmtime: missing memory export, it's not possible")
                });
            global_memory
                .read(store.as_context(), offset, buffer)
                .map_err(|_| TrapCode::MemoryOutOfBounds)
        })
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        let instance = self.instance.clone();
        self.with_store_mut(|store| {
            let global_memory = instance
                .get_export(store.as_context_mut(), "memory")
                .unwrap_or_else(|| {
                    unreachable!("wasmtime: missing memory export, it's not possible")
                })
                .into_memory()
                .unwrap_or_else(|| {
                    unreachable!("wasmtime: missing memory export, it's not possible")
                });
            global_memory
                .write(store.as_context_mut(), offset, &buffer)
                .map_err(|_| TrapCode::MemoryOutOfBounds)
        })
    }

    fn context_mut<R, F: FnOnce(&mut T) -> R>(&mut self, func: F) -> R {
        func(&mut self.store.data_mut().data)
    }

    fn context<R, F: FnOnce(&T) -> R>(&self, func: F) -> R {
        func(&self.store.data().data)
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        self.with_store_mut(|store| {
            if let Ok(remaining_fuel) = store.get_fuel() {
                let new_fuel = remaining_fuel
                    .checked_sub(delta)
                    .ok_or(TrapCode::OutOfFuel)?;
                store.set_fuel(new_fuel).unwrap_or_else(|_| {
                    unreachable!("wasmtime: fuel mode is disabled in wasmtime")
                });
            } else if let Some(fuel) = store.data_mut().fuel.as_mut() {
                *fuel = fuel.checked_sub(delta).ok_or(TrapCode::OutOfFuel)?;
            }
            Ok(())
        })
    }

    fn remaining_fuel(&self) -> Option<u64> {
        self.with_store(|store| {
            if let Ok(fuel) = store.get_fuel() {
                Some(fuel)
            } else if let Some(fuel) = store.data().fuel.as_ref() {
                Some(*fuel)
            } else {
                None
            }
        })
    }
}
