mod alu;
#[cfg(feature = "fpu")]
mod fpu;
mod memory;
mod opcodes;
mod stack;
mod table;

use crate::{
    types::{
        AddressOffset,
        GlobalIdx,
        Pages,
        RwasmModule,
        SignatureIdx,
        TableIdx,
        UntypedValue,
        N_DEFAULT_STACK_SIZE,
        N_MAX_DATA_SEGMENTS,
        N_MAX_STACK_SIZE,
    },
    vm::{
        config::ExecutorConfig,
        executor::opcodes::run_the_loop,
        handler::{always_failing_syscall_handler, SyscallHandler},
        instr_ptr::InstructionPtr,
        memory::GlobalMemory,
        table_entity::TableEntity,
        value_stack::{ValueStack, ValueStackPtr},
    },
    Caller,
    FuelCosts,
    Opcode,
    TrapCode,
    N_MAX_DATA_SEGMENTS_BITS,
    N_MAX_ELEM_SEGMENTS,
    N_MAX_ELEM_SEGMENTS_BITS,
};
use alloc::sync::Arc;
use bitvec::{array::BitArray, bitarr};
use hashbrown::HashMap;
use smallvec::{smallvec, SmallVec};

/// The `RwasmExecutor` struct represents the state and functionality required to execute
/// WebAssembly (WASM) instructions within an embedded WASM runtime environment.
/// It manages the
/// execution context, configuration, and other runtime parts necessary for the proper
/// execution and operation of WASM modules, particularly when leveraging the `rwasm` ecosystem.
///
/// # Generic Parameters
/// - `T`:
/// Custom execution context type to enable user-defined functionality during WASM execution.
///
/// # Fields
/// - `module`: A reference-counted pointer to the `RwasmModule` representing the compiled and
///   loaded WASM module.
/// - `config`: Configuration settings for the WASM executor, encapsulated in an `ExecutorConfig`.
/// - `consumed_fuel`: Tracks the total amount of fuel consumed, where fuel represents computational
///   resource usage.
/// - `refunded_fuel`: Tracks the amount of fuel refunded during execution, allowing optimizations
///   and reimbursements.
/// - `value_stack`: Stack for runtime values, used for computations and function calls in the WASM
///   runtime.
/// - `sp`: Pointer to the current position in the value stack.
/// - `global_memory`: Representation of the global memory accessible to the WASM module during
///   execution.
/// - `ip`: Instruction pointer used to track the next instruction to be executed.
/// - `context`: Custom execution context provided by the user, allowing external state to interact
///   with the executor.
/// - `tracer`: Optional field for an instance of `Tracer`,
/// used to trace or debug execution flow if
///   enabled.
/// - `fuel_costs`: Structure representing fuel consumption costs for various operations, enabling
///   fine-grained control of execution resources.
/// - `tables`: A map associating `TableIdx` (table index) to `TableEntity`, representing managed
///   tables in the WASM module.
/// - `call_stack`: A stack of instruction pointers,
/// used to manage nested function calls and return
///   points during execution.
/// - `last_signature`: Optionally stores the last used signature index, needed for validating
///   indirect function calls.
/// - `next_result`: Optionally stores the result of the next operation, either a valid result or an
///   error of type `TrapCode`.
/// - `stop_exec`: A boolean flag indicating whether the execution should halt prematurely.
/// - `syscall_handler`: A handler of type `SyscallHandler<T>` to execute host function calls or
///   system calls invoked by the WASM module.
/// - `default_elements_segment`: A vector of untyped values representing the default elements
///   segment used in `rwasm`'s modified execution context.
/// - `global_variables`: A map of global variable indices (`GlobalIdx`) to untyped values,
///   representing global variables in the WASM runtime.
/// - `empty_elements_segments`: A bit vector indicating which element segments are considered
///   empty.
/// - `empty_data_segments`: A bit vector indicating which data segments are considered empty.
///
/// # Usage
/// The `RwasmExecutor` is designed to be instantiated and used as the main driver for executing
/// WASM programs.
/// It maintains the state required for computation, controls the flow of execution,
/// and integrates user-defined functionality via the `T` generic execution context.
///
/// Note: This struct is intended as part of an internal runtime and may not expose all fields to
/// public interfaces.
pub struct RwasmExecutor<T> {
    // function segments
    pub(crate) module: Arc<RwasmModule>,
    pub(crate) config: ExecutorConfig,
    // execution context information
    pub(crate) consumed_fuel: u64,
    pub(crate) refunded_fuel: i64,
    pub(crate) value_stack: ValueStack,
    pub(crate) sp: ValueStackPtr,
    pub(crate) global_memory: GlobalMemory,
    pub(crate) ip: InstructionPtr,
    pub(crate) context: T,
    // the last used signature (needed for indirect calls type checks)
    pub(crate) last_signature: Option<SignatureIdx>,
    #[cfg(feature = "tracing")]
    pub(crate) tracer: Option<crate::vm::Tracer>,
    pub(crate) fuel_costs: FuelCosts,
    // rwasm modified segments
    pub(crate) tables: HashMap<TableIdx, TableEntity>,
    pub(crate) global_variables: HashMap<GlobalIdx, UntypedValue>,
    // elem/data emptiness flags
    pub(crate) empty_data_segments: BitArray<[usize; N_MAX_DATA_SEGMENTS_BITS]>,
    pub(crate) empty_elem_segments: BitArray<[usize; N_MAX_ELEM_SEGMENTS_BITS]>,
    // list of nested calls return pointers
    pub(crate) call_stack: SmallVec<[InstructionPtr; 128]>,
    pub(crate) syscall_handler: SyscallHandler<T>,
}

impl<T> RwasmExecutor<T> {
    pub fn new(module: Arc<RwasmModule>, config: ExecutorConfig, context: T) -> Self {
        // create a stack with sp
        let mut value_stack = ValueStack::new(N_DEFAULT_STACK_SIZE, N_MAX_STACK_SIZE);
        let sp = value_stack.stack_ptr();

        // assign sp to the position inside a code section
        let mut ip = InstructionPtr::new(module.code_section.instr.as_ptr());
        ip.add(config.default_pc.unwrap_or(0));

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
            module,
            config,
            consumed_fuel: 0,
            refunded_fuel: 0,
            value_stack,
            sp,
            global_memory,
            ip,
            context,
            #[cfg(feature = "tracing")]
            tracer,
            fuel_costs: Default::default(),
            global_variables: Default::default(),
            tables: Default::default(),
            call_stack: smallvec![],
            last_signature: None,
            syscall_handler: always_failing_syscall_handler,
            empty_elem_segments,
            empty_data_segments,
        }
    }

    pub fn caller(&mut self) -> Caller<T> {
        Caller::new(self)
    }

    pub fn set_syscall_handler(&mut self, handler: SyscallHandler<T>) {
        self.syscall_handler = handler;
    }

    pub fn program_counter(&self) -> u32 {
        let diff = self.ip.ptr as u32 - self.module.code_section.instr.as_ptr() as u32;
        diff / size_of::<Opcode>() as u32
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
    pub fn reset(&mut self, pc: Option<usize>, keep_flags: bool) {
        // if pc is not specified, then fallback to 0 (an entrypoint)
        self.ip = {
            let mut ip = InstructionPtr::new(self.module.code_section.instr.as_ptr());
            ip.add(pc.unwrap_or(0));
            ip
        };
        // reset consumed and refunded fuel to 0
        self.consumed_fuel = 0;
        self.refunded_fuel = 0;
        // reset stack pointer to zero (stack must be cleared)
        self.value_stack.reset();
        self.sp = self.value_stack.stack_ptr();
        // reset the call stack to 0 if it's not, don't drain for performance reasons
        unsafe {
            self.call_stack.set_len(0);
        }
        // we might want to keep data/elem flags between calls, it's required for e2e tests
        if !keep_flags {
            self.empty_data_segments.fill(false);
            self.empty_elem_segments.fill(false);
        }
        // in case of a trap we might have this flag remains active
        self.last_signature = None;
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

    pub fn adjust_fuel_limit(&mut self) -> u64 {
        let consumed_fuel = self.consumed_fuel;
        if let Some(fuel_limit) = self.config.fuel_limit.as_mut() {
            *fuel_limit -= self.consumed_fuel;
        }
        self.consumed_fuel = 0;
        consumed_fuel
    }

    pub fn remaining_fuel(&self) -> Option<u64> {
        Some(self.config.fuel_limit? - self.consumed_fuel)
    }

    pub fn fuel_consumed(&self) -> u64 {
        self.consumed_fuel
    }

    pub fn fuel_refunded(&self) -> i64 {
        self.refunded_fuel
    }

    #[cfg(feature = "tracing")]
    pub fn tracer(&self) -> Option<&crate::vm::Tracer> {
        self.tracer.as_ref()
    }

    #[cfg(feature = "tracing")]
    pub fn tracer_mut(&mut self) -> Option<&mut crate::vm::Tracer> {
        self.tracer.as_mut()
    }

    pub fn context(&self) -> &T {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut T {
        &mut self.context
    }

    pub fn run(&mut self) -> Result<(), TrapCode> {
        match run_the_loop(self) {
            Ok(_) => Ok(()),
            Err(err) => match err {
                TrapCode::ExecutionHalted => Ok(()),
                _ => Err(err),
            },
        }
    }

    pub(crate) fn fetch_table_index(&self, offset: usize) -> TableIdx {
        let mut addr: InstructionPtr = self.ip;
        addr.add(offset);
        match addr.get() {
            Opcode::TableGet(table_idx) => table_idx,
            _ => unreachable!("rwasm: can't extract table index"),
        }
    }

    #[inline(always)]
    pub(crate) fn execute_load_extend(
        &mut self,
        offset: AddressOffset,
        load_extend: fn(
            memory: &[u8],
            address: UntypedValue,
            offset: u32,
        ) -> Result<UntypedValue, TrapCode>,
    ) -> Result<(), TrapCode> {
        self.sp.try_eval_top(|address| {
            let memory = self.global_memory.data();
            let value = load_extend(memory, address, offset)?;
            Ok(value)
        })?;
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn execute_store_wrap(
        &mut self,
        offset: AddressOffset,
        store_wrap: fn(
            memory: &mut [u8],
            address: UntypedValue,
            offset: u32,
            value: UntypedValue,
        ) -> Result<(), TrapCode>,
        #[allow(unused_variables)] len: u32,
    ) -> Result<(), TrapCode> {
        let (address, value) = self.sp.pop2();
        let memory = self.global_memory.data_mut();
        store_wrap(memory, address, offset, value)?;
        #[cfg(feature = "tracing")]
        if let Some(tracer) = self.tracer.as_mut() {
            let base_address = offset + u32::from(address);
            tracer.memory_change(
                base_address,
                len,
                &memory[base_address as usize..(base_address + len) as usize],
            );
        }
        self.ip.add(1);
        Ok(())
    }
}
