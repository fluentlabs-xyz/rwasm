mod alu;
mod control_flow;
#[cfg(feature = "fpu")]
mod fpu;
mod memory;
mod stack;
mod system;
mod table;

use crate::{
    mem_index::UNIT,
    types::{AddressOffset, RwasmModule, TableIdx, UntypedValue},
    CallStack,
    InstructionPtr,
    Opcode,
    RwasmCaller,
    RwasmStore,
    SysFuncIdx,
    TrapCode,
    TypedCaller,
    Value,
    ValueStack,
    ValueStackPtr,
};
use smallvec::SmallVec;

/// The `RwasmExecutor` struct is a foundational component for executing WebAssembly modules
/// in the `rwasm` runtime environment. It acts as the primary execution object, coordinating
/// the state and execution flow of a WebAssembly module.
///
/// # Type Parameters
/// - `'a`: A lifetime tied to borrowed references within the executor, ensuring the validity of
///   borrowed objects during execution.
/// - `T`: A generic parameter that must implement the `Send` and `Sync` traits. This allows
///   multithreaded access and mutable operations on the WASM store.
///
/// # Fields
/// - `module` (`&'a RwasmModule`): A reference to the rWasm module being executed. This contains
///   the compiled function definitions, memory, and other runtime components for execution.
///
/// - `value_stack` (`&'a mut ValueStack`): A mutable reference to the value stack, which is used
///   during execution to store intermediate values, operand results, and function return values.
///
/// - `sp` (`ValueStackPtr`): A pointer to the current position in the value stack. This tracks the
///   stack pointer (SP) for operand and value management during execution.
///
/// - `call_stack` (`&'a mut CallStack`): A mutable reference to the call stack, which is
///   responsible for managing the function call/return frames to track execution flow across nested
///   function calls.
///
/// - `ip` (`InstructionPtr`): The instruction pointer representing the location of the current
///   instruction in the execution sequence of the WebAssembly module.
///
/// - `store` (`&'a mut RwasmStore<T>`): A mutable reference to the runtime store which maintains
///   memory, global variables, and another runtime state for the execution context. The store also
///   allows external data of the type `T` to be integrated with the WebAssembly instance.
///
/// # Usage
/// The `RwasmExecutor` is typically constructed internally by the runtime and should be
/// used to step through execution of instructions within a WebAssembly module.
/// It provides internal access to the runtime's data structures for fine-grained
/// control over WebAssembly execution.
///
/// # Thread Safety
/// The `Send` and `Sync` constraints on `T` ensure that the executor's associated runtime
/// store is safe for concurrent mutation and multithreaded execution scenarios, as
/// required by the WebAssembly specification's concurrency guarantees.
pub struct RwasmExecutor<'a, T: Send + Sync + 'static> {
    pub(crate) module: &'a RwasmModule,
    pub(crate) value_stack: &'a mut ValueStack,
    pub(crate) sp: ValueStackPtr,
    pub(crate) call_stack: &'a mut CallStack,
    pub(crate) ip: InstructionPtr,
    pub(crate) store: &'a mut RwasmStore<T>,
}

macro_rules! exec_opcode {
    ($self:ident, $instr:expr, $terminate_expr:expr) => {{}
    use Opcode::*;
    match $instr {
        // stack
        Unreachable => $self.visit_unreachable()?,
        Trap(imm) => $self.visit_trap_code(imm)?,
        LocalGet(imm) => $self.visit_local_get(imm),
        LocalSet(imm) => $self.visit_local_set(imm),
        LocalTee(imm) => $self.visit_local_tee(imm),
        Br(imm) => $self.visit_br(imm),
        BrIfEqz(imm) => $self.visit_br_if(imm),
        BrIfNez(imm) => $self.visit_br_if_nez(imm),
        BrTable(imm) => $self.visit_br_table(imm),
        ConsumeFuel(imm) => $self.visit_consume_fuel(imm)?,
        ConsumeFuelStack => $self.visit_consume_fuel_stack()?,
        Return => {
            if $self.visit_return() {
                $terminate_expr
            }
        }
        ReturnCallInternal(imm) => $self.visit_return_call_internal(imm),
        ReturnCall(imm) => {
            if $self.visit_return_call(imm)? {
                $terminate_expr
            }
        }
        ReturnCallIndirect(imm) => $self.visit_return_call_indirect(imm)?,
        CallInternal(imm) => $self.visit_call_internal(imm)?,
        Call(imm) => {
            if $self.visit_call(imm)? {
                $terminate_expr
            }
        }
        CallIndirect(imm) => $self.visit_call_indirect(imm)?,
        SignatureCheck(imm) => $self.visit_signature_check(imm)?,
        StackCheck(imm) => $self.visit_stack_check(imm)?,
        Drop => $self.visit_drop(),
        Select => $self.visit_select(),
        GlobalGet(imm) => $self.visit_global_get(imm),
        GlobalSet(imm) => $self.visit_global_set(imm),
        RefFunc(imm) => $self.visit_ref_func(imm),
        I32Const(imm) => $self.visit_i32_const(imm),

        // alu
        I32Eqz => $self.visit_i32_eqz(),
        I32Eq => $self.visit_i32_eq(),
        I32Ne => $self.visit_i32_ne(),
        I32LtS => $self.visit_i32_lt_s(),
        I32LtU => $self.visit_i32_lt_u(),
        I32GtS => $self.visit_i32_gt_s(),
        I32GtU => $self.visit_i32_gt_u(),
        I32LeS => $self.visit_i32_le_s(),
        I32LeU => $self.visit_i32_le_u(),
        I32GeS => $self.visit_i32_ge_s(),
        I32GeU => $self.visit_i32_ge_u(),
        I32Clz => $self.visit_i32_clz(),
        I32Ctz => $self.visit_i32_ctz(),
        I32Popcnt => $self.visit_i32_popcnt(),
        I32Add => $self.visit_i32_add(),
        I32Sub => $self.visit_i32_sub(),
        I32Mul => $self.visit_i32_mul(),
        I32DivS => $self.visit_i32_div_s()?,
        I32DivU => $self.visit_i32_div_u()?,
        I32RemS => $self.visit_i32_rem_s()?,
        I32RemU => $self.visit_i32_rem_u()?,
        I32And => $self.visit_i32_and(),
        I32Or => $self.visit_i32_or(),
        I32Xor => $self.visit_i32_xor(),
        I32Shl => $self.visit_i32_shl(),
        I32ShrS => $self.visit_i32_shr_s(),
        I32ShrU => $self.visit_i32_shr_u(),
        I32Rotl => $self.visit_i32_rotl(),
        I32Rotr => $self.visit_i32_rotr(),
        I32WrapI64 => $self.visit_i32_wrap_i64(),
        I32Extend8S => $self.visit_i32_extend8_s(),
        I32Extend16S => $self.visit_i32_extend16_s(),

        // memory
        MemorySize => $self.visit_memory_size(),
        MemoryGrow => $self.visit_memory_grow()?,
        MemoryFill => $self.visit_memory_fill()?,
        MemoryCopy => $self.visit_memory_copy()?,
        MemoryInit(imm) => $self.visit_memory_init(imm)?,
        DataDrop(imm) => $self.visit_data_drop(imm),
        I32Load(imm) => $self.visit_i32_load(imm)?,
        I32Load8S(imm) => $self.visit_i32_load_i8_s(imm)?,
        I32Load8U(imm) => $self.visit_i32_load_i8_u(imm)?,
        I32Load16S(imm) => $self.visit_i32_load_i16_s(imm)?,
        I32Load16U(imm) => $self.visit_i32_load_i16_u(imm)?,
        I32Store(imm) => $self.visit_i32_store(imm)?,
        I32Store8(imm) => $self.visit_i32_store_8(imm)?,
        I32Store16(imm) => $self.visit_i32_store_16(imm)?,

        // table
        TableSize(imm) => $self.visit_table_size(imm),
        TableGrow(imm) => $self.visit_table_grow(imm)?,
        TableFill(imm) => $self.visit_table_fill(imm)?,
        TableGet(imm) => $self.visit_table_get(imm)?,
        TableSet(imm) => $self.visit_table_set(imm)?,
        TableCopy(dst_imm, src_imm) => $self.visit_table_copy(dst_imm, src_imm)?,
        TableInit(imm) => $self.visit_table_init(imm)?,
        ElemDrop(imm) => $self.visit_element_drop(imm),

        // fpu
        #[cfg(feature = "fpu")]
        opcode => $self.exec_fpu_opcode(opcode)?,
    }};
}

impl<'a, T: Send + Sync> RwasmExecutor<'a, T> {
    pub fn entrypoint(
        module: &'a RwasmModule,
        value_stack: &'a mut ValueStack,
        call_stack: &'a mut CallStack,
        store: &'a mut RwasmStore<T>,
    ) -> Self {
        let sp = value_stack.stack_ptr();
        let ip = InstructionPtr::new(module.code_section.instr.as_ptr());
        Self::new(module, value_stack, sp, call_stack, ip, store)
    }

    pub fn new(
        module: &'a RwasmModule,
        value_stack: &'a mut ValueStack,
        sp: ValueStackPtr,
        call_stack: &'a mut CallStack,
        ip: InstructionPtr,
        store: &'a mut RwasmStore<T>,
    ) -> Self {
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

    pub fn caller<'vm>(&'vm mut self) -> TypedCaller<'vm, T> {
        let program_counter = self.program_counter();
        TypedCaller::Rwasm(RwasmCaller::<'vm, T>::new(
            &mut self.store,
            program_counter,
            self.sp,
        ))
    }

    pub fn program_counter(&self) -> u32 {
        let diff = self.ip.ptr as u32 - self.module.code_section.instr.as_ptr() as u32;
        diff / size_of::<Opcode>() as u32
    }

    pub fn run(&mut self, params: &[Value], result: &mut [Value]) -> Result<(), TrapCode> {
        // copy input params
        for x in params {
            self.sp.push_value(x);
        }
        // run the loop
        let status = self.run_the_loop(params, result);
        // trap halts the execution, we need to clear the stack
        if let Some(trap_code) = status.err() {
            // clear stack only for non-interrupted calls
            if trap_code != TrapCode::InterruptionCalled {
                // TODO(dmitry123): "do we also need to reset store flags?"
                self.call_stack.reset();
            }
            // forward the error
            return Err(trap_code);
        }
        // copy output values in case of successful execution
        for x in result {
            *x = self.sp.pop_value(x.ty());
        }
        self.value_stack.sync_stack_ptr(self.sp);
        // execution is over, clear stacks
        // TODO(dmitry123): "enable this check after refactoring tests"
        // debug_assert_eq!(
        //     self.value_stack.stack_len(self.sp),
        //     0,
        //     "after execution the value stack must be empty"
        // );
        // we must reset the call stack in case of traps inside nested calls
        self.call_stack.reset();
        Ok(())
    }

    fn run_the_loop(&mut self, params: &[Value], result: &mut [Value]) -> Result<(), TrapCode> {
        loop {
            let instr = self.ip.get();
            #[cfg(feature = "debug-print")]
            self.debug_print(&instr);
            #[cfg(feature = "tracing")]
            {
                self.trace_instr_pre(&instr);
                let mut wrapper = |instr: Opcode| -> Result<bool, TrapCode> {
                    exec_opcode!(self, instr, return Ok(true));
                    Ok(false)
                };
                let res = wrapper(instr);
                self.trace_instr_post(&instr, res.err());
                if res? {
                    break Ok(());
                }
            }
            #[cfg(not(feature = "tracing"))]
            exec_opcode!(self, instr, break Ok(()));
        }
    }

    #[cfg(feature = "tracing")]
    pub fn step(mut self) -> (Result<bool, TrapCode>, InstructionPtr, ValueStackPtr) {
        if !self
            .ip
            .is_valid((self.module.code_section.instr.last().unwrap()) as *const Opcode as u64)
        {
            return (Err(TrapCode::UnreachableCodeReached), self.ip, self.sp);
        };
        let instr = self.ip.get();
        self.trace_instr_pre(&instr);
        let mut wrapper = |instr: Opcode| -> Result<bool, TrapCode> {
            exec_opcode!(self, instr, return Ok(true));
            Ok(false)
        };
        let res = wrapper(instr);
        self.trace_instr_post(&instr, res.err());
        (res, self.ip, self.sp)
    }

    #[cfg(feature = "tracing")]
    fn trace_instr_pre(&mut self, instr: &Opcode) {
        let pc = self.program_counter();
        let memory_size: u32 = self.store.global_memory.current_pages().into();
        let consumed_fuel = self.store.fuel_consumed();
        self.store.tracer.pre_opcode_state(pc, self.sp, *instr);
        self.store.tracer.state.next_cycle();
    }

    #[cfg(feature = "tracing")]
    fn trace_instr_post(&mut self, instr: &Opcode, trap_code: Option<TrapCode>) {
        let sp = self.sp.to_relative_address();
        let pc = self.program_counter();
        let stack = self.value_stack.dump_stack(self.sp);
        let op_state = self.store.tracer.logs.last_mut().unwrap();
        op_state.next_pc = pc;
        op_state.next_sp = sp;
        let opcode = op_state.opcode;
        self.store.tracer.post_opcode_state(pc, sp, *instr, stack);
        println!("op_state:{:?}", self.store.tracer.logs.last());
        // TODO(wangyao): "track trap codes"
        self.store.tracer.state.next_cycle();
    }

    #[cfg(feature = "tracing")]
    pub fn relative_ip(self) -> isize {
        self.ip.to_offset(self.module.code_section.instr.as_ptr())
    }

    #[cfg(feature = "debug-print")]
    fn debug_print(&mut self, instr: &Opcode) {
        let stack = self.value_stack.dump_stack(self.sp);
        println!(
            "{:04}:\t {} \tstack({}):{:?}",
            self.program_counter(),
            instr,
            stack.len(),
            stack
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
            _ => unreachable!("can't extract table index"),
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
        #[cfg(not(feature = "tracing"))]
        {
            let (address, value) = self.sp.pop2();
            let memory = self.store.global_memory.data_mut();
            store_wrap(memory, address, offset, value)?;
        }
        #[cfg(feature = "tracing")]
        {
            use crate::{align, is_multi_align, mem::MemoryRecordEnum, mem_index::AddressType};
            let (address, value) = self.sp.pop2();
            println!("base_addrss:{},value:{}", address, value);
            let memory = self.store.global_memory.data_mut();
            let addr = address.to_bits() + offset;
            let aligned_addr: u32 = align(addr);
            let old_val = match self.store.tracer.memory_records.get(&aligned_addr) {
                Some(record) => record.value,
                None => 0,
            };

            store_wrap(memory, address, offset, value)?;

            let typed_addr = AddressType::GlobalMemory(aligned_addr.into());
            let new_val = u32::from_le_bytes(
                memory[aligned_addr as usize..(aligned_addr + UNIT) as usize]
                    .try_into()
                    .unwrap(),
            );
            println!("rawaddr store:{}", aligned_addr);
            println!("virtual_addr:{}", typed_addr.to_virtual_addr());
            let res_memory_record = self
                .store
                .tracer
                .mw(typed_addr.to_virtual_addr(), new_val.into());

            self.store
                .tracer
                .logs
                .last_mut()
                .unwrap()
                .memory_access
                .memory = Some(MemoryRecordEnum::Write(res_memory_record));

            self.store.tracer.logs.last_mut().unwrap().res = value.into();
            let opcode = self.ip.get();

            if is_multi_align(opcode, addr) {
                let aligned_addr_hi = aligned_addr + UNIT;
                let new_val_hi = u32::from_le_bytes(
                    memory[(aligned_addr_hi) as usize..(aligned_addr_hi + UNIT) as usize]
                        .try_into()
                        .unwrap(),
                );
                let typed_addr_hi = AddressType::GlobalMemory(aligned_addr_hi.into());
                let res_record_hi = self
                    .store
                    .tracer
                    .mw(typed_addr_hi.to_virtual_addr(), new_val_hi);
                self.store
                    .tracer
                    .logs
                    .last_mut()
                    .unwrap()
                    .memory_access
                    .memory_hi = Some(MemoryRecordEnum::Write(res_record_hi));
            }
        }
        self.ip.add(1);
        Ok(())
    }

    pub(crate) fn invoke_syscall(&mut self, sys_func_idx: SysFuncIdx) -> Result<bool, TrapCode> {
        let (params, result) = self
            .store
            .import_linker
            .resolve_by_func_idx(sys_func_idx)
            .map(|v| (v.params, v.result))
            .unwrap_or_else(|| unreachable!("can't resolve syscall in the import linker"));
        let params_len = params.len();
        let result_len = result.len();
        let max_in_out = params_len.max(result_len);
        self.value_stack.reserve(max_in_out)?;
        let mut buffer = SmallVec::<[Value; 16]>::default();
        buffer.resize(params.len() + result.len(), Value::I32(0));
        for (i, x) in params.iter().enumerate() {
            buffer[params.len() - i - 1] = self.sp.pop_value(*x);
        }
        for (i, x) in result.iter().enumerate() {
            buffer[params.len() + i] = Value::default(*x);
        }
        let (params, result) = buffer.split_at_mut(params.len());
        let syscall_handler = self.store.syscall_handler;
        let mut caller = self.caller();
        match syscall_handler(&mut caller, sys_func_idx, params, result) {
            Ok(_) => {
                // TODO(dmitry123): "resync SP, only for e2e testing suite"
                self.sp = caller.as_rwasm_ref().sp();
                // if execution succeeded, then copy output params back to the stack
                for x in result {
                    self.sp.push_value(x)
                }
                // just continue the execution, don't terminate the loop
                Ok(false)
            }
            Err(TrapCode::ExecutionHalted) => {
                // if execution halted, then copy output params back to the stack because the caller
                // might want to read these params
                for x in result {
                    self.sp.push_value(x)
                }
                // when execution is halted, then we terminate an execution loop
                Ok(true)
            }
            Err(TrapCode::InterruptionCalled) => {
                // terminate an execution
                Err(TrapCode::InterruptionCalled)
            }
            Err(err) => Err(err),
        }
    }
}
