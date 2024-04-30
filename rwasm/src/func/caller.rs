use super::super::{AsContext, AsContextMut, StoreContext, StoreContextMut};
use crate::{
    core::{Trap, TrapCode},
    store::FuelError,
    Engine,
    Extern,
    Instance,
    Memory,
};

/// Represents the caller’s context when creating a host function via [`Func::wrap`].
///
/// [`Func::wrap`]: struct.Func.html#method.wrap
pub struct Caller<'a, T> {
    ctx: StoreContextMut<'a, T>,
    /// The module instance associated to the call.
    /// This is `Some` if the host function was called from a Wasm function
    /// since all Wasm function are associated to a module instance.
    /// This usually is `None` if the host function was called from the host side.
    instance: Option<Instance>,
}

impl<'a, T> Caller<'a, T> {
    /// Creates a new [`Caller`] from the given store context and [`Instance`] handle.
    pub fn new<C>(ctx: &'a mut C, instance: Option<&Instance>) -> Self
    where
        C: AsContextMut<UserState = T>,
    {
        Self {
            ctx: ctx.as_context_mut(),
            instance: instance.copied(),
        }
    }

    /// Queries the caller for an exported definition identifier by `name`.
    ///
    /// Returns `None` if there is no associated [`Instance`] of the caller
    /// or if the caller does not provide an export under the name `name`.
    pub fn get_export(&self, name: &str) -> Option<Extern> {
        self.instance
            .and_then(|instance| instance.get_export(self, name))
    }

    /// Returns a shared reference to the user provided host data.
    pub fn data(&self) -> &T {
        self.ctx.store.data()
    }

    /// Returns an exclusive reference to the user provided host data.
    pub fn data_mut(&mut self) -> &mut T {
        self.ctx.store.data_mut()
    }

    /// Returns a shared reference to the used [`Engine`].
    pub fn engine(&self) -> &Engine {
        self.ctx.store.engine()
    }

    pub fn exported_memory(&self) -> Memory {
        self.get_export("memory")
            .and_then(|memory| match memory {
                Extern::Memory(memory) => Some(memory),
                _ => None,
            })
            .unwrap_or_else(|| unreachable!("there is no memory export inside"))
    }

    pub fn read_memory(&self, offset: u32, len: u32) -> Result<&[u8], Trap> {
        let buffer = self.exported_memory().data(self);
        let buffer = buffer
            .get((offset as usize)..(offset as usize + len as usize))
            .ok_or::<Trap>(TrapCode::MemoryOutOfBounds.into())?;
        Ok(buffer)
    }

    pub fn write_memory(&mut self, address: u32, data: &[u8]) -> Result<(), Trap> {
        let address = address as usize;
        let memory = self
            .exported_memory()
            .data_mut(self.as_context_mut())
            .get_mut(address..(address + data.len()))
            .ok_or::<Trap>(TrapCode::MemoryOutOfBounds.into())?;
        memory.clone_from_slice(data);
        self.ctx
            .store
            .tracer_mut()
            .memory_change(address as u32, data.len() as u32, data);
        Ok(())
    }

    /// Adds `delta` quantity of fuel to the remaining fuel.
    ///
    /// # Panics
    ///
    /// If this overflows the remaining fuel counter.
    ///
    /// # Errors
    ///
    /// If fuel metering is disabled.
    pub fn add_fuel(&mut self, delta: u64) -> Result<(), FuelError> {
        self.ctx.store.add_fuel(delta)
    }

    /// Returns the amount of fuel consumed by executions of the [`Store`](crate::Store) so far.
    ///
    /// Returns `None` if fuel metering is disabled.
    pub fn fuel_consumed(&self) -> Option<u64> {
        self.ctx.store.fuel_consumed()
    }

    /// Synthetically consumes an amount of fuel for the [`Store`](crate::Store).
    ///
    /// Returns the remaining amount of fuel after this operation.
    ///
    /// # Panics
    ///
    /// If this overflows the consumed fuel counter.
    ///
    /// # Errors
    ///
    /// - If fuel metering is disabled.
    /// - If more fuel is consumed than available.
    pub fn consume_fuel(&mut self, delta: u64) -> Result<u64, FuelError> {
        self.ctx.store.consume_fuel(delta)
    }
}

impl<T> AsContext for Caller<'_, T> {
    type UserState = T;

    #[inline]
    fn as_context(&self) -> StoreContext<'_, Self::UserState> {
        self.ctx.as_context()
    }
}

impl<T> AsContextMut for Caller<'_, T> {
    #[inline]
    fn as_context_mut(&mut self) -> StoreContextMut<'_, Self::UserState> {
        self.ctx.as_context_mut()
    }
}

impl<'a, T: AsContextMut> From<&'a mut T> for Caller<'a, T::UserState> {
    #[inline]
    fn from(ctx: &'a mut T) -> Self {
        Self {
            ctx: ctx.as_context_mut(),
            instance: None,
        }
    }
}
