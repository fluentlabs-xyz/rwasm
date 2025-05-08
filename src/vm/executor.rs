use crate::{
    types::{
        AddressOffset,
        DropKeep,
        FuelCosts,
        GlobalIdx,
        OpcodeData,
        Pages,
        RwasmError,
        RwasmModule,
        SignatureIdx,
        TableIdx,
        UntypedValue,
        FUNC_REF_OFFSET,
        N_DEFAULT_STACK_SIZE,
        N_MAX_DATA_SEGMENTS,
        N_MAX_RECURSION_DEPTH,
        N_MAX_STACK_SIZE,
        N_MAX_TABLE_SIZE,
    },
    vm::{
        config::ExecutorConfig,
        handler::{always_failing_syscall_handler, SyscallHandler},
        instr_ptr::InstructionPtr,
        memory::GlobalMemory,
        opcodes::run_the_loop,
        table_entity::TableEntity,
        tracer::Tracer,
        value_stack::{ValueStack, ValueStackPtr},
    },
};
use alloc::sync::Arc;
use bitvec::{bitvec, prelude::BitVec};
use hashbrown::HashMap;

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
///   error of type `RwasmError`.
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
    pub(crate) tracer: Option<Tracer>,
    pub(crate) fuel_costs: FuelCosts,
    // rwasm modified segments
    pub(crate) tables: HashMap<TableIdx, TableEntity>,
    pub(crate) default_elements_segment: Vec<UntypedValue>,
    pub(crate) global_variables: HashMap<GlobalIdx, UntypedValue>,
    pub(crate) empty_elements_segments: BitVec,
    pub(crate) empty_data_segments: BitVec,
    // list of nested calls return pointers
    pub(crate) call_stack: Vec<InstructionPtr>,
    // the last used signature (needed for indirect calls type checks)
    pub(crate) last_signature: Option<SignatureIdx>,
    pub(crate) next_result: Option<Result<i32, RwasmError>>,
    pub(crate) stop_exec: bool,
    pub(crate) syscall_handler: SyscallHandler<T>,
}

impl<T> RwasmExecutor<T> {
    pub fn parse(
        rwasm_bytecode: &[u8],
        config: ExecutorConfig,
        context: T,
    ) -> Result<Self, RwasmError> {
        Ok(Self::new(
            Arc::new(RwasmModule::new(rwasm_bytecode)),
            config,
            context,
        ))
    }

    pub fn new(module: Arc<RwasmModule>, config: ExecutorConfig, context: T) -> Self {
        // create a stack with sp
        let mut value_stack = ValueStack::new(N_DEFAULT_STACK_SIZE, N_MAX_STACK_SIZE);
        let sp = value_stack.stack_ptr();

        // assign sp to the position inside a code section
        let mut ip = InstructionPtr::new(module.code_section.instr.as_ptr());
        ip.add(module.source_pc as usize);

        // create global memory
        let global_memory = GlobalMemory::new(Pages::default());

        let dropped_elements = bitvec![0; N_MAX_TABLE_SIZE];
        let empty_data_segments = bitvec![0; N_MAX_DATA_SEGMENTS];

        let tracer = if config.trace_enabled {
            Some(Tracer::default())
        } else {
            None
        };

        let module_elements_section = module
            .element_section
            .iter()
            .copied()
            .map(|v| UntypedValue::from(v + FUNC_REF_OFFSET))
            .collect::<Vec<_>>();

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
            tracer,
            fuel_costs: Default::default(),
            global_variables: Default::default(),
            tables: Default::default(),
            call_stack: vec![],
            last_signature: None,
            next_result: None,
            stop_exec: false,
            syscall_handler: always_failing_syscall_handler,
            default_elements_segment: module_elements_section,
            empty_elements_segments: dropped_elements,
            empty_data_segments,
        }
    }

    pub fn set_syscall_handler(&mut self, handler: SyscallHandler<T>) {
        self.syscall_handler = handler;
    }

    pub fn program_counter(&self) -> u32 {
        self.ip.pc()
    }

    pub fn reset(&mut self, pc: Option<usize>) {
        let mut ip = InstructionPtr::new(self.module.code_section.instr.as_ptr());
        ip.add(pc.unwrap_or(self.module.source_pc as usize));
        self.ip = ip;
        self.consumed_fuel = 0;
        self.value_stack.drain();
        self.sp = self.value_stack.stack_ptr();
        self.call_stack.clear();
        self.last_signature = None;
    }

    pub fn reset_last_signature(&mut self) {
        self.last_signature = None;
    }

    pub fn try_consume_fuel(&mut self, fuel: u64) -> Result<(), RwasmError> {
        let consumed_fuel = self.consumed_fuel.checked_add(fuel).unwrap_or(u64::MAX);
        if let Some(fuel_limit) = self.config.fuel_limit {
            if consumed_fuel > fuel_limit {
                return Err(RwasmError::OutOfFuel);
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

    pub fn tracer(&self) -> Option<&Tracer> {
        self.tracer.as_ref()
    }

    pub fn tracer_mut(&mut self) -> Option<&mut Tracer> {
        self.tracer.as_mut()
    }

    pub fn context(&self) -> &T {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut T {
        &mut self.context
    }

    pub fn run(&mut self) -> Result<i32, RwasmError> {
        match run_the_loop(self) {
            Ok(exit_code) => Ok(exit_code),
            Err(err) => match err {
                RwasmError::ExecutionHalted(exit_code) => Ok(exit_code),
                _ => Err(err),
            },
        }
    }

    pub(crate) fn fetch_drop_keep(&self, offset: usize) -> DropKeep {
        let mut addr: InstructionPtr = self.ip;
        addr.add(offset);
        match addr.data() {
            OpcodeData::DropKeep(drop_keep) => *drop_keep,
            _ => unreachable!("rwasm: can't extract drop keep"),
        }
    }

    pub(crate) fn fetch_table_index(&self, offset: usize) -> TableIdx {
        let mut addr: InstructionPtr = self.ip;
        addr.add(offset);
        match addr.data() {
            OpcodeData::TableIdx(table_idx) => *table_idx,
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
        ) -> Result<UntypedValue, RwasmError>,
    ) -> Result<(), RwasmError> {
        self.sp.try_eval_top(|address| {
            let memory = self.global_memory.data();
            let value = load_extend(memory, address, offset.into_inner())?;
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
        ) -> Result<(), RwasmError>,
        len: u32,
    ) -> Result<(), RwasmError> {
        let (address, value) = self.sp.pop2();
        let memory = self.global_memory.data_mut();
        store_wrap(memory, address, offset.into_inner(), value)?;
        self.ip.offset(0);
        let address = u32::from(address);
        let base_address = offset.into_inner() + address;
        if let Some(tracer) = self.tracer.as_mut() {
            tracer.memory_change(
                base_address,
                len,
                &memory[base_address as usize..(base_address + len) as usize],
            );
        }
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn execute_unary(&mut self, f: fn(UntypedValue) -> UntypedValue) {
        self.sp.eval_top(f);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn execute_binary(&mut self, f: fn(UntypedValue, UntypedValue) -> UntypedValue) {
        self.sp.eval_top2(f);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn try_execute_unary(
        &mut self,
        f: fn(UntypedValue) -> Result<UntypedValue, RwasmError>,
    ) -> Result<(), RwasmError> {
        self.sp.try_eval_top(f)?;
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn try_execute_binary(
        &mut self,
        f: fn(UntypedValue, UntypedValue) -> Result<UntypedValue, RwasmError>,
    ) -> Result<(), RwasmError> {
        self.sp.try_eval_top2(f)?;
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn execute_call_internal(
        &mut self,
        is_nested_call: bool,
        skip: usize,
        func_idx: u32,
    ) -> Result<(), RwasmError> {
        self.ip.add(skip);
        self.value_stack.sync_stack_ptr(self.sp);
        if is_nested_call {
            if self.call_stack.len() > N_MAX_RECURSION_DEPTH {
                return Err(RwasmError::StackOverflow);
            }
            self.call_stack.push(self.ip);
        }
        let instr_ref = self
            .module
            .func_section
            .get(func_idx as usize)
            .copied()
            .expect("rwasm: unknown internal function");
        self.sp = self.value_stack.stack_ptr();
        self.ip = InstructionPtr::new(self.module.code_section.instr.as_ptr());
        self.ip.add(instr_ref as usize);
        Ok(())
    }
}
