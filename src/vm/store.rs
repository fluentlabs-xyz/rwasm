use crate::{
    always_failing_syscall_handler,
    ExecutorConfig,
    FuelCosts,
    GlobalIdx,
    GlobalMemory,
    Pages,
    SignatureIdx,
    SyscallHandler,
    TableEntity,
    TableIdx,
    TrapCode,
    UntypedValue,
    N_MAX_DATA_SEGMENTS,
    N_MAX_DATA_SEGMENTS_BITS,
    N_MAX_ELEM_SEGMENTS,
    N_MAX_ELEM_SEGMENTS_BITS,
};
use bitvec::{array::BitArray, bitarr};
use hashbrown::HashMap;

pub struct Store<T> {
    pub(crate) consumed_fuel: u64,
    pub(crate) refunded_fuel: i64,
    pub(crate) global_memory: GlobalMemory,
    pub(crate) context: T,
    pub(crate) config: ExecutorConfig,
    // the last used signature (needed for indirect calls type checks)
    pub(crate) last_signature: Option<SignatureIdx>,
    #[cfg(feature = "tracing")]
    pub(crate) tracer: Option<crate::vm::Tracer>,
    // rwasm modified segments
    pub(crate) tables: HashMap<TableIdx, TableEntity>,
    pub(crate) global_variables: HashMap<GlobalIdx, UntypedValue>,
    // elem/data emptiness flags
    pub(crate) empty_data_segments: BitArray<[usize; N_MAX_DATA_SEGMENTS_BITS]>,
    pub(crate) empty_elem_segments: BitArray<[usize; N_MAX_ELEM_SEGMENTS_BITS]>,
    // list of nested calls return pointers
    pub(crate) syscall_handler: SyscallHandler<T>,
    pub(crate) fuel_costs: FuelCosts,
}

impl<T: Default> Default for Store<T> {
    fn default() -> Self {
        Self::new(ExecutorConfig::default(), T::default())
    }
}

impl<T> Store<T> {
    pub fn new(config: ExecutorConfig, context: T) -> Self {
        // create global memory
        let global_memory = GlobalMemory::new(Pages::default());

        let empty_data_segments = bitarr![0; N_MAX_DATA_SEGMENTS];
        let empty_elem_segments = bitarr![0; N_MAX_ELEM_SEGMENTS];

        #[cfg(feature = "tracing")]
        let tracer = if config.trace_enabled {
            Some(crate::vm::Tracer::default())
        } else {
            None
        };

        Self {
            consumed_fuel: 0,
            refunded_fuel: 0,
            global_memory,
            context,
            #[cfg(feature = "tracing")]
            tracer,
            global_variables: Default::default(),
            tables: Default::default(),
            last_signature: None,
            syscall_handler: always_failing_syscall_handler,
            empty_elem_segments,
            empty_data_segments,
            config,
            fuel_costs: Default::default(),
        }
    }

    /// Resets the state of the current execution context.
    ///
    /// # Parameters
    /// - `pc`: An optional program counter (`usize`) specifying the instruction pointer position to
    ///   reset to. If not provided, defaults to `0` (the entrypoint).
    /// - `keep_flags`: A boolean indicating whether to preserve the data and element segment flags
    ///   (`true` to keep the flags, `false` to reset them).
    ///
    /// # Behavior
    /// - Resets the instruction pointer (`ip`) to the specified `pc` or the default value of `0`.
    /// - Clears the consumed and refunded fuel counters by setting them to `0`.
    /// - Resets the value stack by clearing its contents and updating the stack pointer (`sp`).
    /// - Empties the call stack by setting its length to `0`.
    /// - Resets the data and element segment flags to `false` if `keep_flags` is `false`.
    /// - Clears the `last_signature` field, which can remain active after a trap.
    ///
    /// # Notes
    /// - The `value_stack` is completely cleared, and the stack pointer (`sp`) is re-initialized to
    ///   reflect the reset state.
    /// - The call stack is reset to zero directly through an unsafe operation for performance
    ///   optimization, avoiding a full drain.
    /// - Preserving the data and element flags with `keep_flags` is particularly useful for
    ///   end-to-end test cases that depend on unchanged segments.
    pub fn reset(&mut self, keep_flags: bool) {
        // reset consumed and refunded fuel to 0
        self.consumed_fuel = 0;
        self.refunded_fuel = 0;
        // we might want to keep data/elem flags between calls, it's required for e2e tests
        if !keep_flags {
            self.empty_data_segments.fill(false);
            self.empty_elem_segments.fill(false);
        }
        // in case of a trap we might have this flag remains active
        self.last_signature = None;
    }

    pub fn fuel_consumed(&self) -> u64 {
        self.consumed_fuel
    }

    pub fn fuel_refunded(&self) -> i64 {
        self.refunded_fuel
    }

    pub fn try_consume_fuel(&mut self, fuel: u64) -> Result<(), TrapCode> {
        let consumed_fuel = self.consumed_fuel.checked_add(fuel).unwrap_or(u64::MAX);
        if let Some(fuel_limit) = self.config.fuel_limit {
            if consumed_fuel > fuel_limit {
                return Err(TrapCode::OutOfFuel);
            }
        }
        self.consumed_fuel = consumed_fuel;
        Ok(())
    }

    pub fn refund_fuel(&mut self, fuel: i64) {
        self.refunded_fuel += fuel;
    }

    pub fn remaining_fuel(&self) -> Option<u64> {
        Some(self.config.fuel_limit? - self.consumed_fuel)
    }

    pub fn context(&self) -> &T {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut T {
        &mut self.context
    }

    pub fn set_syscall_handler(&mut self, handler: SyscallHandler<T>) {
        self.syscall_handler = handler;
    }
}
