use crate::bitvec_inlined::BitVecInlined as BV;
use crate::{
    FuelConfig, GlobalMemory, ImportLinker, InstructionPtr, Pages, SignatureIdx, Store,
    SyscallHandler, TableEntity, TrapCode, UntypedValue, ValueStackPtr,
};
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Host-side store that holds memory, tables, globals and host context for an rwasm instance.
/// It also tracks fuel for metering and provides access to imported functions and syscalls.
/// The store is passed to host callbacks and persists across invocations of the same module.
pub struct RwasmStore<T: 'static + Send + Sync> {
    /// Total amount of fuel consumed by the currently running instance.
    pub(crate) consumed_fuel: u64,
    /// The linear memory shared by the running module and the host.
    pub(crate) global_memory: Option<GlobalMemory>,
    /// User-defined context available to host functions and syscalls.
    pub(crate) context: T,
    /// The last used signature index used for validating indirect calls.
    pub(crate) last_signature: Option<SignatureIdx>,
    /// Runtime-managed tables (may differ from compile-time layout due to mutations).
    pub(crate) tables: Vec<TableEntity>,
    /// Runtime values of mutable and immutable globals.
    pub(crate) global_variables: Vec<UntypedValue>,
    /// Bitset tracking which data segments have been consumed/emptied.
    pub(crate) empty_data_segments: BV<2>,
    /// Bitset tracking which element segments have been consumed/emptied.
    pub(crate) empty_elem_segments: BV<2>,
    /// Dispatcher for system calls made by the guest.
    pub(crate) syscall_handler: SyscallHandler<T>,
    /// Linker that resolves imports to host functions/globals.
    pub(crate) import_linker: Arc<ImportLinker>,
    /// If set, contains the instruction/value-stack pointers to resume after a suspension.
    pub(crate) resumable_context: Option<(InstructionPtr, ValueStackPtr)>,
    /// A fuel config.
    pub(crate) fuel_config: FuelConfig,
    /// Execution tracer used when the `tracing` feature is enabled.
    #[cfg(feature = "tracing")]
    pub tracer: crate::Tracer,
}

#[cfg(feature = "std")]
impl<T: 'static + Send + Sync + Default> Default for RwasmStore<T> {
    fn default() -> Self {
        Self::new(
            Arc::new(ImportLinker::default()),
            T::default(),
            crate::always_failing_syscall_handler,
            FuelConfig::default(),
        )
    }
}

impl<T: 'static + Send + Sync> Store<T> for RwasmStore<T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        self.get_global_memory().read(offset, buffer)?;
        Ok(())
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        self.get_global_memory().write(offset, buffer)?;
        #[cfg(feature = "tracing")]
        self.tracer
            .memory_change(offset as u32, buffer.len() as u32, buffer);
        Ok(())
    }

    fn context_mut<R, F: FnOnce(&mut T) -> R>(&mut self, func: F) -> R {
        func(&mut self.context)
    }

    fn context<R, F: FnOnce(&T) -> R>(&self, func: F) -> R {
        func(&self.context)
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        let consumed_fuel = self.consumed_fuel.checked_add(delta).unwrap_or(u64::MAX);
        if let Some(fuel_limit) = self.fuel_config.fuel_limit {
            if consumed_fuel > fuel_limit {
                return Err(TrapCode::OutOfFuel);
            }
        }
        self.consumed_fuel = consumed_fuel;
        Ok(())
    }

    fn remaining_fuel(&self) -> Option<u64> {
        Some(self.fuel_config.fuel_limit? - self.consumed_fuel)
    }
}

impl<T: 'static + Send + Sync> RwasmStore<T> {
    pub fn new(
        import_linker: Arc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
        fuel_config: FuelConfig,
    ) -> Self {
        Self {
            consumed_fuel: 0,
            global_memory: None,
            context,
            #[cfg(feature = "tracing")]
            tracer: crate::Tracer::default(),
            global_variables: Default::default(),
            tables: Default::default(),
            last_signature: None,
            syscall_handler,
            empty_data_segments: BV::EMPTY,
            empty_elem_segments: BV::EMPTY,
            import_linker,
            resumable_context: None,
            fuel_config,
        }
    }

    pub fn get_global_memory(&mut self) -> &mut GlobalMemory {
        if self.global_memory.is_none() {
            self.global_memory = Some(GlobalMemory::new(Pages::default()))
        }
        self.global_memory.as_mut().unwrap()
    }

    /// Resets the state of the current execution context.
    pub fn reset(&mut self, keep_flags: bool) {
        // reset consumed fuel to 0
        self.consumed_fuel = 0;
        // we might want to keep data/elem flags between calls, it's required for e2e tests
        if !keep_flags {
            self.empty_data_segments = BV::EMPTY;
            self.empty_elem_segments = BV::EMPTY;
        }
        // in case of a trap, we might have this flag remains active
        self.last_signature = None;
    }

    pub fn fuel_consumed(&self) -> u64 {
        self.consumed_fuel
    }

    pub fn set_syscall_handler(&mut self, handler: SyscallHandler<T>) {
        self.syscall_handler = handler;
    }
}
