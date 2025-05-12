mod alu;
mod control_flow;
// mod fpu;
mod memory;
mod stack;
mod system;

use crate::{
    types::{
        DropKeep,
        FuelCosts,
        GlobalIdx,
        Instruction,
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
    },
    vm::{
        config::ExecutorConfig,
        executor::{
            alu::{
                exec_arith_signed_opcode,
                exec_arith_unsigned_opcode,
                exec_bitwise_binary_opcode,
                exec_bitwise_unary_opcode,
                exec_compare_binary_opcode,
                exec_compare_unary_opcode,
                exec_convert_unary_opcode,
            },
            control_flow::exec_control_flow_opcode,
            memory::{exec_memory_load_opcode, exec_memory_store_opcode},
            stack::exec_stack_opcode,
            system::exec_system_opcode,
        },
        instr_ptr::InstructionPtr,
        memory::GlobalMemory,
        syscall::{always_failing_syscall_handler, SyscallHandler},
        table::TableEntity,
        value_stack::{ValueStack, ValueStackPtr},
    },
    Opcode,
    TrapCode,
    N_MAX_ELEMENT_SEGMENTS,
};
use alloc::{sync::Arc, vec, vec::Vec};
use bitvec::{bitarr, prelude::BitArray};
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
/// - `tracer`: Optional field for an instance of `Tracer`, used to trace or debug execution flow if
///   enabled.
/// - `fuel_costs`: Structure representing fuel consumption costs for various operations, enabling
///   fine-grained control of execution resources.
/// - `tables`: A map associating `TableIdx` (table index) to `TableEntity`, representing managed
///   tables in the WASM module.
/// - `call_stack`: A stack of instruction pointers, used to manage nested function calls and return
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
    #[cfg(feature = "tracing")]
    pub(crate) tracer: Option<crate::Tracer>,
    pub(crate) fuel_costs: FuelCosts,
    // rwasm modified segments
    pub(crate) tables: HashMap<TableIdx, TableEntity>,
    pub(crate) default_elements_segment: Vec<UntypedValue>,
    pub(crate) global_variables: HashMap<GlobalIdx, UntypedValue>,
    pub(crate) empty_elements_segments:
        BitArray<[usize; bitvec::mem::elts::<usize>(N_MAX_ELEMENT_SEGMENTS)]>,
    pub(crate) empty_data_segments:
        BitArray<[usize; ::bitvec::mem::elts::<usize>(N_MAX_DATA_SEGMENTS)]>,
    // list of nested calls return pointers
    pub(crate) call_stack: Vec<InstructionPtr>,
    // the last used signature (needed for indirect calls type checks)
    pub(crate) last_signature: Option<SignatureIdx>,
    pub(crate) syscall_handler: SyscallHandler<T>,
}

macro_rules! time {
    () => {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
    };
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

        let empty_elements_segments = bitarr![0; N_MAX_ELEMENT_SEGMENTS];
        let empty_data_segments = bitarr![0; N_MAX_DATA_SEGMENTS];

        let module_elements_section = module
            .element_section
            .iter()
            .copied()
            .map(|v| UntypedValue::from(v + FUNC_REF_OFFSET))
            .collect::<Vec<_>>();

        #[cfg(feature = "tracing")]
        let tracer = if config.trace_enabled {
            Some(crate::Tracer::default())
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
            call_stack: vec![],
            last_signature: None,
            syscall_handler: always_failing_syscall_handler,
            default_elements_segment: module_elements_section,
            empty_elements_segments,
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

    pub fn reset_pc(&mut self) {
        let mut ip = InstructionPtr::new(self.module.code_section.instr.as_ptr());
        ip.add(self.module.source_pc as usize);
        self.ip = ip;
        self.consumed_fuel = 0;
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
    pub fn tracer(&self) -> Option<&crate::Tracer> {
        self.tracer.as_ref()
    }

    #[cfg(feature = "tracing")]
    pub fn tracer_mut(&mut self) -> Option<&mut crate::Tracer> {
        self.tracer.as_mut()
    }

    pub fn context(&self) -> &T {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut T {
        &mut self.context
    }

    pub fn run(&mut self) -> Result<i32, RwasmError> {
        // let mut opcode_cost: HashMap<Opcode, u128> = HashMap::new();
        let result = match self.run_the_loop(/*&mut opcode_cost*/) {
            Ok(_) => Ok(0),
            Err(err) => match err {
                TrapCode::ExecutionHalted => Ok(0),
                err => Err(RwasmError::TrapCode(err)),
            },
        };
        // let mut opcode_cost = opcode_cost.iter().collect::<Vec<_>>();
        // opcode_cost.sort_by(|a, b| b.1.cmp(a.1));
        // for (k, v) in opcode_cost {
        //     println!(" - {}={}", k, v);
        // }
        result
    }

    #[cfg(feature = "debug-print")]
    fn debug_trace_opcode(&self, opcode: &crate::Opcode, _data: &crate::Instruction) {
        let stack = self.value_stack.dump_stack(self.sp);
        println!(
            "{}:\t {:?} \tstack({}):{:?}",
            self.ip.pc(),
            opcode,
            stack.len(),
            stack
                .iter()
                .rev()
                .take(10)
                .map(|v| v.as_usize())
                .collect::<Vec<_>>()
        );
    }

    // #[cfg(feature = "tracer")]
    // if exec.tracer.is_some() {
    //     use rwasm::engine::bytecode::InstrMeta;
    //     let memory_size: u32 = exec.global_memory.current_pages().into();
    //     let consumed_fuel = exec.fuel_consumed();
    //     let stack = exec.value_stack.dump_stack(exec.sp);
    //     exec.tracer.as_mut().unwrap().pre_opcode_state(
    //         exec.ip.pc(),
    //         instr,
    //         stack,
    //         &InstrMeta::new(0, 0, 0),
    //         memory_size,
    //         consumed_fuel,
    //     );
    // }

    fn run_the_loop(
        &mut self, /* opcode_cost: &mut HashMap<Opcode, u128> */
    ) -> Result<(), TrapCode> {
        loop {
            // let time = time!();
            let instr = self.ip.get();

            // #[cfg(feature = "debug-print")]
            // self.debug_trace_opcode(&opcode, &data);

            let opcode = instr.opcode();
            if opcode.is_stack_opcode() {
                exec_stack_opcode(self, instr)?;
            } else if opcode.is_arith_unsigned_opcode() {
                exec_arith_unsigned_opcode(self, opcode)?;
            } else if opcode.is_arith_signed_opcode() {
                exec_arith_signed_opcode(self, opcode)?;
            } else if opcode.is_memory_load_opcode() {
                exec_memory_load_opcode(self, instr)?;
            } else if opcode.is_memory_store_opcode() {
                exec_memory_store_opcode(self, instr)?;
            } else if opcode.is_compare_unary_opcode() {
                exec_compare_unary_opcode(self, opcode)?;
            } else if opcode.is_compare_binary_opcode() {
                exec_compare_binary_opcode(self, opcode)?;
            } else if opcode.is_control_flow_opcode() {
                exec_control_flow_opcode(self, instr)?;
            } else if opcode.is_bitwise_unary_opcode() {
                exec_bitwise_unary_opcode(self, opcode)?;
            } else if opcode.is_bitwise_binary_opcode() {
                exec_bitwise_binary_opcode(self, opcode)?;
            } else if opcode.is_convert_opcode() {
                exec_convert_unary_opcode(self, opcode)?;
            } else if opcode.is_system_opcode() {
                exec_system_opcode(self, instr)?;
            } else if opcode.is_float_opcode() {
                // exec_fpu_opcode(self, instr)?;
                unreachable!()
            } else {
                unreachable!()
            }

            // let time = (time!() - time).as_nanos();
            // if !opcode_cost.contains_key(&opcode) {
            //     opcode_cost.insert(opcode, time);
            // } else {
            //     *opcode_cost.get_mut(&opcode).unwrap() += time;
            // }
        }
    }

    pub(crate) fn fetch_drop_keep(&self, offset: usize) -> DropKeep {
        let mut addr: InstructionPtr = self.ip;
        addr.add(offset);
        match addr.get() {
            Instruction::DropKeep(_, drop_keep) => drop_keep,
            _ => unreachable!("rwasm: can't extract drop keep"),
        }
    }

    pub(crate) fn fetch_table_index(&self, offset: usize) -> TableIdx {
        let mut addr: InstructionPtr = self.ip;
        addr.add(offset);
        match addr.get() {
            Instruction::TableIdx(_, table_idx) => table_idx,
            _ => unreachable!("rwasm: can't extract table index"),
        }
    }

    #[inline(always)]
    pub(crate) fn execute_call_internal(
        &mut self,
        is_nested_call: bool,
        skip: usize,
        func_idx: u32,
    ) -> Result<(), TrapCode> {
        self.ip.add(skip);
        self.value_stack.sync_stack_ptr(self.sp);
        if is_nested_call {
            if self.call_stack.len() > N_MAX_RECURSION_DEPTH {
                return Err(TrapCode::StackOverflow);
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
