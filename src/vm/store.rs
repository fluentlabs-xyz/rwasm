use crate::{
    CallStack, GlobalIdx, GlobalMemory, ImportLinker, InstructionPtr, Pages, RwasmModule,
    SignatureIdx, StoreTr, SyscallHandler, TableEntity, TableIdx, TrapCode, UntypedValue,
    ValueStack,
};
use alloc::sync::Arc;
use bitvec::{order::Lsb0, vec::BitVec};
use hashbrown::HashMap;

/// Host-side store that holds memory, tables, globals, and host context for a rwasm instance.
/// It also tracks fuel for metering and provides access to imported functions and syscalls.
/// The store is passed to host callbacks and persists across invocations of the same module.
pub struct RwasmStore<T: 'static> {
    /// Total amount of fuel consumed by the currently running instance.
    pub(crate) consumed_fuel: u64,
    /// The linear memory shared by the running module and the host.
    pub(crate) global_memory: GlobalMemory,
    /// User-defined context available to host functions and syscalls.
    pub(crate) data: T,
    /// The last used signature index used for validating indirect calls.
    pub(crate) last_signature: Option<SignatureIdx>,
    /// Runtime-managed tables (may differ from compile-time layout due to mutations).
    pub(crate) tables: HashMap<TableIdx, TableEntity>,
    /// Runtime values of mutable and immutable globals.
    pub(crate) global_variables: HashMap<GlobalIdx, UntypedValue>,
    /// Bitset tracking which data segments have been consumed/emptied.
    pub(crate) empty_data_segments: BitVec,
    /// Bitset tracking which element segments have been consumed/emptied.
    pub(crate) empty_elem_segments: BitVec,
    /// Dispatcher for system calls made by the guest.
    pub(crate) syscall_handler: SyscallHandler<T>,
    /// Linker that resolves imports to host functions/globals.
    pub(crate) import_linker: Arc<ImportLinker>,
    /// If set, contains the instruction/value-stack pointers to resume after a suspension.
    pub(crate) resumable_context: Option<ReusableContext>,
    /// A fuel config.
    pub(crate) fuel_limit: Option<u64>,
    /// Execution tracer used when the `tracing` feature is enabled.
    #[cfg(feature = "tracing")]
    pub tracer: crate::Tracer,
}

pub struct ReusableContext {
    pub module: RwasmModule,
    pub call_stack: CallStack,
    pub ip: InstructionPtr,
    pub value_stack: ValueStack,
}

#[cfg(feature = "std")]
impl<T: 'static + Default> Default for RwasmStore<T> {
    fn default() -> Self {
        Self::new(
            Arc::new(ImportLinker::default()),
            T::default(),
            crate::always_failing_syscall_handler,
            None,
        )
    }
}

impl<T: 'static> StoreTr<T> for RwasmStore<T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode> {
        self.global_memory.read(offset, buffer)?;
        Ok(())
    }

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode> {
        self.global_memory.write(offset, buffer)?;
        #[cfg(feature = "tracing")]
        self.tracer
            .memory_change(offset as u32, buffer.len() as u32, buffer);
        Ok(())
    }

    fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }

    fn data(&self) -> &T {
        &self.data
    }

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode> {
        let consumed_fuel = self.consumed_fuel.checked_add(delta).unwrap_or(u64::MAX);
        if let Some(fuel_limit) = self.fuel_limit {
            if consumed_fuel > fuel_limit {
                return Err(TrapCode::OutOfFuel);
            }
        }
        self.consumed_fuel = consumed_fuel;
        Ok(())
    }

    fn remaining_fuel(&self) -> Option<u64> {
        Some(self.fuel_limit? - self.consumed_fuel)
    }
}

impl<T: 'static> RwasmStore<T> {
    pub fn new(
        import_linker: Arc<ImportLinker>,
        context: T,
        syscall_handler: SyscallHandler<T>,
        fuel_limit: Option<u64>,
    ) -> Self {
        let global_memory = GlobalMemory::new(Pages::default());
        Self {
            consumed_fuel: 0,
            global_memory,
            data: context,
            #[cfg(feature = "tracing")]
            tracer: crate::Tracer::default(),
            global_variables: Default::default(),
            tables: Default::default(),
            last_signature: None,
            syscall_handler,
            empty_data_segments: BitVec::EMPTY,
            empty_elem_segments: BitVec::EMPTY,
            import_linker,
            resumable_context: None,
            fuel_limit,
        }
    }

    /// Resets the state of the current execution context.
    pub fn reset(&mut self, keep_flags: bool) {
        // reset consumed fuel to 0
        self.consumed_fuel = 0;
        // we might want to keep data/elem flags between calls, it's required for e2e tests
        if !keep_flags {
            // we don't do any assumptions regarding how data segments are used,
            // maybe there is a way to optimize reuse of bitset.
            if self.empty_data_segments.len() <= size_of::<usize>() {
                self.empty_data_segments.fill(false);
            } else {
                self.empty_data_segments = BitVec::<usize, Lsb0>::EMPTY;
            }
            // we don't do any assumptions regarding how tables are used inside the applications,
            // so keep it always empty, probably there is an optimization here.
            if self.empty_elem_segments.len() <= size_of::<usize>() {
                self.empty_elem_segments.fill(false);
            } else {
                self.empty_elem_segments = BitVec::<usize, Lsb0>::EMPTY;
            }
        }
        // in case of a trap, we might have this flag remains active
        self.last_signature = None;
    }

    pub fn set_fuel(&mut self, fuel: Option<u64>) {
        self.fuel_limit = fuel;
    }

    pub fn fuel_consumed(&self) -> u64 {
        self.consumed_fuel
    }

    pub fn set_syscall_handler(&mut self, handler: SyscallHandler<T>) {
        self.syscall_handler = handler;
    }
}
