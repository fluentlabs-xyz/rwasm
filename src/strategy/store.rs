use crate::{CallerTr, RwasmCaller, RwasmStore, StoreTr, TrapCode};

pub enum TypedCaller<'a, T: 'static> {
    Rwasm(RwasmCaller<'a, T>),
    #[cfg(feature = "wasmtime")]
    Wasmtime(crate::wasmtime::WasmtimeCaller<'a, T>),
}

impl<'a, T> TypedCaller<'a, T> {
    pub fn as_rwasm_mut(&mut self) -> &mut RwasmCaller<'a, T> {
        match self {
            TypedCaller::Rwasm(store) => store,
            #[allow(unreachable_patterns)]
            _ => unreachable!(),
        }
    }

    pub fn as_rwasm_ref(&self) -> &RwasmCaller<'a, T> {
        match self {
            TypedCaller::Rwasm(store) => store,
            #[allow(unreachable_patterns)]
            _ => unreachable!(),
        }
    }

    pub fn into_rwasm(self) -> RwasmCaller<'a, T> {
        match self {
            TypedCaller::Rwasm(store) => store,
            #[allow(unreachable_patterns)]
            _ => unreachable!(),
        }
    }

    #[cfg(feature = "wasmtime")]
    pub fn as_wasmtime_mut(&mut self) -> &mut crate::wasmtime::WasmtimeCaller<'a, T> {
        match self {
            TypedCaller::Wasmtime(store) => store,
            _ => unreachable!(),
        }
    }

    #[cfg(feature = "wasmtime")]
    pub fn as_wasmtime_ref(&self) -> &crate::wasmtime::WasmtimeCaller<'a, T> {
        match self {
            TypedCaller::Wasmtime(store) => store,
            _ => unreachable!(),
        }
    }

    #[cfg(feature = "wasmtime")]
    pub fn into_wasmtime(self) -> crate::wasmtime::WasmtimeCaller<'a, T> {
        match self {
            TypedCaller::Wasmtime(store) => store,
            _ => unreachable!(),
        }
    }
}

impl<'a, T> StoreTr<T> for TypedCaller<'a, T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        match self {
            TypedCaller::Rwasm(store) => store.memory_read(offset, buffer),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(store) => store.memory_read(offset, buffer),
        }
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        match self {
            TypedCaller::Rwasm(store) => store.memory_write(offset, buffer),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(store) => store.memory_write(offset, buffer),
        }
    }

    fn data_mut(&mut self) -> &mut T {
        match self {
            TypedCaller::Rwasm(store) => store.data_mut(),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(store) => store.data_mut(),
        }
    }

    fn data(&self) -> &T {
        match self {
            TypedCaller::Rwasm(store) => store.data(),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(store) => store.data(),
        }
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        match self {
            TypedCaller::Rwasm(store) => store.try_consume_fuel(delta),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(store) => store.try_consume_fuel(delta),
        }
    }

    fn remaining_fuel(&self) -> Option<u64> {
        match self {
            TypedCaller::Rwasm(store) => store.remaining_fuel(),
            #[cfg(feature = "wasmtime")]
            TypedCaller::Wasmtime(store) => store.remaining_fuel(),
        }
    }
}

impl<'a, T> CallerTr<T> for TypedCaller<'a, T> {}

pub enum TypedStore<T: 'static> {
    Rwasm(RwasmStore<T>),
    #[cfg(feature = "wasmtime")]
    Wasmtime(crate::wasmtime::WasmtimeExecutor<T>),
}

impl<T> StoreTr<T> for TypedStore<T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        match self {
            TypedStore::Rwasm(store) => store.memory_read(offset, buffer),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(store) => store.memory_read(offset, buffer),
        }
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        match self {
            TypedStore::Rwasm(store) => store.memory_write(offset, buffer),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(store) => store.memory_write(offset, buffer),
        }
    }

    fn data_mut(&mut self) -> &mut T {
        match self {
            TypedStore::Rwasm(store) => store.data_mut(),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(store) => store.data_mut(),
        }
    }

    fn data(&self) -> &T {
        match self {
            TypedStore::Rwasm(store) => store.data(),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(store) => store.data(),
        }
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        match self {
            TypedStore::Rwasm(store) => store.try_consume_fuel(delta),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(store) => store.try_consume_fuel(delta),
        }
    }

    fn remaining_fuel(&self) -> Option<u64> {
        match self {
            TypedStore::Rwasm(store) => store.remaining_fuel(),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(store) => store.remaining_fuel(),
        }
    }
}

impl<T> TypedStore<T> {
    pub fn reset(&mut self, keep_flags: bool) {
        match self {
            TypedStore::Rwasm(store) => store.reset(keep_flags),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(_) => {}
        }
    }

    pub fn set_fuel(&mut self, fuel: u64) {
        match self {
            TypedStore::Rwasm(store) => store.set_fuel(Some(fuel)),
            #[cfg(feature = "wasmtime")]
            TypedStore::Wasmtime(store) => store.store.set_fuel(fuel).unwrap(),
        }
    }
}
