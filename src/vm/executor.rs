mod alu;
mod control_flow;
#[cfg(feature = "fpu")]
mod fpu;
mod memory;
mod stack;
mod system;
mod table;

use crate::{
    types::{AddressOffset, RwasmModule, TableIdx, UntypedValue},
    CallStack,
    Caller,
    InstructionPtr,
    Opcode,
    Store,
    SysFuncIdx,
    TrapCode,
    ValueStack,
    ValueStackPtr,
};

pub fn execute_rwasm_module<'a, T>(
    module: &'a RwasmModule,
    value_stack: &'a mut ValueStack,
    call_stack: &'a mut CallStack,
    store: &'a mut Store<T>,
) -> Result<(), TrapCode> {
    RwasmExecutor::new(&module, value_stack, call_stack, store).run()
}

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
pub struct RwasmExecutor<'a, T> {
    pub(crate) module: &'a RwasmModule,
    pub(crate) value_stack: &'a mut ValueStack,
    pub(crate) sp: ValueStackPtr,
    pub(crate) call_stack: &'a mut CallStack,
    pub(crate) ip: InstructionPtr,
    pub(crate) store: &'a mut Store<T>,
}

impl<'a, T> RwasmExecutor<'a, T> {
    pub fn new(
        module: &'a RwasmModule,
        value_stack: &'a mut ValueStack,
        call_stack: &'a mut CallStack,
        store: &'a mut Store<T>,
    ) -> Self {
        let sp = value_stack.stack_ptr();
        let ip = InstructionPtr::new(module.code_section.instr.as_ptr());
        Self {
            module,
            value_stack,
            sp,
            call_stack,
            ip,
            store,
        }
    }

    pub fn advance_ip(&mut self, offset: usize) {
        self.ip.add(offset)
    }

    pub fn caller(&mut self) -> Caller<T> {
        let program_counter = self.program_counter();
        Caller::new(self.store, &mut self.sp, program_counter, self.ip)
    }

    pub fn program_counter(&self) -> u32 {
        let diff = self.ip.ptr as u32 - self.module.code_section.instr.as_ptr() as u32;
        diff / size_of::<Opcode>() as u32
    }

    pub fn run(&mut self) -> Result<(), TrapCode> {
        macro_rules! exec_opcode {
            ($instr:expr, $terminate_expr:expr) => {{}
            use Opcode::*;
            match $instr {
                // stack
                Unreachable => self.visit_unreachable()?,
                Trap(imm) => self.visit_trap_code(imm)?,
                LocalGet(imm) => self.visit_local_get(imm),
                LocalSet(imm) => self.visit_local_set(imm),
                LocalTee(imm) => self.visit_local_tee(imm),
                Br(imm) => self.visit_br(imm),
                BrIfEqz(imm) => self.visit_br_if(imm),
                BrIfNez(imm) => self.visit_br_if_nez(imm),
                BrTable(imm) => self.visit_br_table(imm),
                ConsumeFuel(imm) => self.visit_consume_fuel(imm)?,
                ConsumeFuelStack => self.visit_consume_fuel_stack()?,
                Return => {
                    if self.visit_return() {
                        $terminate_expr
                    }
                }
                ReturnCallInternal(imm) => self.visit_return_call_internal(imm),
                ReturnCall(imm) => {
                    if self.visit_return_call(imm)? {
                        $terminate_expr
                    }
                }
                ReturnCallIndirect(imm) => self.visit_return_call_indirect(imm)?,
                CallInternal(imm) => self.visit_call_internal(imm)?,
                Call(imm) => {
                    if self.visit_call(imm)? {
                        $terminate_expr
                    }
                }
                CallIndirect(imm) => self.visit_call_indirect(imm)?,
                SignatureCheck(imm) => self.visit_signature_check(imm)?,
                StackCheck(imm) => self.visit_stack_check(imm)?,
                Drop => self.visit_drop(),
                Select => self.visit_select(),
                GlobalGet(imm) => self.visit_global_get(imm),
                GlobalSet(imm) => self.visit_global_set(imm),
                RefFunc(imm) => self.visit_ref_func(imm),
                I32Const(imm) => self.visit_i32_const(imm),

                // alu
                I32Eqz => self.visit_i32_eqz(),
                I32Eq => self.visit_i32_eq(),
                I32Ne => self.visit_i32_ne(),
                I32LtS => self.visit_i32_lt_s(),
                I32LtU => self.visit_i32_lt_u(),
                I32GtS => self.visit_i32_gt_s(),
                I32GtU => self.visit_i32_gt_u(),
                I32LeS => self.visit_i32_le_s(),
                I32LeU => self.visit_i32_le_u(),
                I32GeS => self.visit_i32_ge_s(),
                I32GeU => self.visit_i32_ge_u(),
                I32Clz => self.visit_i32_clz(),
                I32Ctz => self.visit_i32_ctz(),
                I32Popcnt => self.visit_i32_popcnt(),
                I32Add => self.visit_i32_add(),
                I32Sub => self.visit_i32_sub(),
                I32Mul => self.visit_i32_mul(),
                I32DivS => self.visit_i32_div_s()?,
                I32DivU => self.visit_i32_div_u()?,
                I32RemS => self.visit_i32_rem_s()?,
                I32RemU => self.visit_i32_rem_u()?,
                I32And => self.visit_i32_and(),
                I32Or => self.visit_i32_or(),
                I32Xor => self.visit_i32_xor(),
                I32Shl => self.visit_i32_shl(),
                I32ShrS => self.visit_i32_shr_s(),
                I32ShrU => self.visit_i32_shr_u(),
                I32Rotl => self.visit_i32_rotl(),
                I32Rotr => self.visit_i32_rotr(),
                I32WrapI64 => self.visit_i32_wrap_i64(),
                I32Extend8S => self.visit_i32_extend8_s(),
                I32Extend16S => self.visit_i32_extend16_s(),

                // memory
                MemorySize => self.visit_memory_size(),
                MemoryGrow => self.visit_memory_grow()?,
                MemoryFill => self.visit_memory_fill()?,
                MemoryCopy => self.visit_memory_copy()?,
                MemoryInit(imm) => self.visit_memory_init(imm)?,
                DataDrop(imm) => self.visit_data_drop(imm),
                I32Load(imm) => self.visit_i32_load(imm)?,
                I32Load8S(imm) => self.visit_i32_load_i8_s(imm)?,
                I32Load8U(imm) => self.visit_i32_load_i8_u(imm)?,
                I32Load16S(imm) => self.visit_i32_load_i16_s(imm)?,
                I32Load16U(imm) => self.visit_i32_load_i16_u(imm)?,
                I32Store(imm) => self.visit_i32_store(imm)?,
                I32Store8(imm) => self.visit_i32_store_8(imm)?,
                I32Store16(imm) => self.visit_i32_store_16(imm)?,

                // table
                TableSize(imm) => self.visit_table_size(imm),
                TableGrow(imm) => self.visit_table_grow(imm)?,
                TableFill(imm) => self.visit_table_fill(imm)?,
                TableGet(imm) => self.visit_table_get(imm)?,
                TableSet(imm) => self.visit_table_set(imm)?,
                TableCopy(imm) => self.visit_table_copy(imm)?,
                TableInit(imm) => self.visit_table_init(imm)?,
                ElemDrop(imm) => self.visit_element_drop(imm),

                // fpu
                #[cfg(feature = "fpu")]
                opcode => self.exec_fpu_opcode(opcode)?,
            }};
        }

        loop {
            let instr = self.ip.get();

            #[cfg(feature = "debug-print")]
            self.debug_print(&instr);

            #[cfg(feature = "tracing")]
            {
                self.trace_instr_pre(&instr);
                let mut wrapper = |instr: Opcode| -> Result<bool, TrapCode> {
                    exec_opcode!(instr, return Ok(true));
                    Ok(false)
                };
                let res = wrapper(instr);
                self.trace_instr_post(&instr, res.err());
                if res? {
                    break Ok(());
                }
            }

            #[cfg(not(feature = "tracing"))]
            exec_opcode!(instr, break Ok(()));
        }
    }

    #[cfg(feature = "tracing")]
    fn trace_instr_pre(&mut self, instr: &Opcode) {
        let pc = self.program_counter();
        let memory_size: u32 = self.store.global_memory.current_pages().into();
        let consumed_fuel = self.store.fuel_consumed();
        self.store.tracer.pre_opcode_state(pc, self.sp, *instr);
    }

    #[cfg(feature = "tracing")]
    fn trace_instr_post(&mut self, instr: &Opcode, trap_code: Option<TrapCode>) {
        // TODO(wangyao): "track trap codes"
        let sp = self.sp.to_position();
        let pc = self.program_counter();
        let stack = self.value_stack.dump_stack();
        self.store.tracer.post_opcode_state(pc, sp, stack);
    }

    #[cfg(feature = "debug-print")]
    fn debug_print(&mut self, instr: &Opcode) {
        println!(
            "{:04}:\t {} \tstack({}):{:?}",
            self.program_counter(),
            instr,
            stack.len(),
            self.value_stack
                .as_slice()
                .iter()
                .rev()
                .take(10)
                .map(|v| v.as_usize())
                .collect::<Vec<_>>()
        );
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
            let memory = self.store.global_memory.data();
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
        let memory = self.store.global_memory.data_mut();
        store_wrap(memory, address, offset, value)?;
        #[cfg(feature = "tracing")]
        {
            let base_address = offset + u32::from(address);
            self.store.tracer.memory_change(
                base_address,
                len,
                &memory[base_address as usize..(base_address + len) as usize],
            );
        }
        self.ip.add(1);
        Ok(())
    }

    pub(crate) fn invoke_syscall(&mut self, sys_func_idx: SysFuncIdx) -> Result<bool, TrapCode> {
        match (self.store.syscall_handler)(self.caller(), sys_func_idx) {
            Ok(_) => Ok(false),
            Err(TrapCode::ExecutionHalted) => Ok(true),
            Err(err) => Err(err),
        }
    }

    pub fn store(&self) -> &Store<T> {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut Store<T> {
        &mut self.store
    }

    pub fn context(&self) -> &T {
        &self.store.context
    }

    pub fn context_mut(&mut self) -> &mut T {
        &mut self.store.context
    }
}
