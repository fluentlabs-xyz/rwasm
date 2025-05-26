use crate::{types::Opcode, vm::executor::RwasmExecutor, TrapCode};

pub(crate) fn run_the_loop<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    loop {
        let instr = vm.ip.get();
        #[cfg(feature = "debug-print")]
        {
            let stack = vm.value_stack.dump_stack(vm.sp);
            println!(
                "{:04x}:\t {} \tstack({}):{:?}",
                vm.ip.pc(),
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
        #[cfg(feature = "tracing")]
        if vm.tracer.is_some() {
            let memory_size: u32 = vm.global_memory.current_pages().into();
            let consumed_fuel = vm.fuel_consumed();
            let stack = vm.value_stack.dump_stack(vm.sp);
            vm.tracer.as_mut().unwrap().pre_opcode_state(
                vm.ip.pc(),
                instr,
                stack,
                &crate::OpcodeMeta {
                    index: 0,
                    pos: 0,
                    opcode: 0,
                },
                memory_size,
                consumed_fuel,
            );
        }
        use Opcode::*;
        match instr {
            // stack
            Unreachable => super::stack::visit_unreachable(vm)?,
            Trap(imm) => super::stack::visit_trap_code(vm, imm)?,
            LocalGet(imm) => super::stack::visit_local_get(vm, imm),
            LocalSet(imm) => super::stack::visit_local_set(vm, imm),
            LocalTee(imm) => super::stack::visit_local_tee(vm, imm),
            Br(imm) => super::stack::visit_br(vm, imm),
            BrIfEqz(imm) => super::stack::visit_br_if(vm, imm),
            BrIfNez(imm) => super::stack::visit_br_if_nez(vm, imm),
            BrTable(imm) => super::stack::visit_br_table(vm, imm),
            ConsumeFuel(imm) => super::stack::visit_consume_fuel(vm, imm)?,
            Return => super::stack::visit_return(vm)?,
            ReturnCallInternal(imm) => super::stack::visit_return_call_internal(vm, imm)?,
            ReturnCall(imm) => super::stack::visit_return_call(vm, imm)?,
            ReturnCallIndirect(imm) => super::stack::visit_return_call_indirect(vm, imm)?,
            CallInternal(imm) => super::stack::visit_call_internal(vm, imm)?,
            Call(imm) => super::stack::visit_call(vm, imm)?,
            CallIndirect(imm) => super::stack::visit_call_indirect(vm, imm)?,
            SignatureCheck(imm) => super::stack::visit_signature_check(vm, imm)?,
            StackCheck(imm) => super::stack::visit_stack_alloc(vm, imm)?,
            Drop => super::stack::visit_drop(vm),
            Select => super::stack::visit_select(vm),
            GlobalGet(imm) => super::stack::visit_global_get(vm, imm),
            GlobalSet(imm) => super::stack::visit_global_set(vm, imm),
            RefFunc(imm) => super::stack::visit_ref_func(vm, imm),
            I32Const(imm) => super::stack::visit_i32_i64_const(vm, imm),

            // alu
            I32Eqz => super::alu::visit_i32_eqz(vm),
            I32Eq => super::alu::visit_i32_eq(vm),
            I32Ne => super::alu::visit_i32_ne(vm),
            I32LtS => super::alu::visit_i32_lt_s(vm),
            I32LtU => super::alu::visit_i32_lt_u(vm),
            I32GtS => super::alu::visit_i32_gt_s(vm),
            I32GtU => super::alu::visit_i32_gt_u(vm),
            I32LeS => super::alu::visit_i32_le_s(vm),
            I32LeU => super::alu::visit_i32_le_u(vm),
            I32GeS => super::alu::visit_i32_ge_s(vm),
            I32GeU => super::alu::visit_i32_ge_u(vm),
            I32Clz => super::alu::visit_i32_clz(vm),
            I32Ctz => super::alu::visit_i32_ctz(vm),
            I32Popcnt => super::alu::visit_i32_popcnt(vm),
            I32Add => super::alu::visit_i32_add(vm),
            I32Sub => super::alu::visit_i32_sub(vm),
            I32Mul => super::alu::visit_i32_mul(vm),
            I32DivS => super::alu::visit_i32_div_s(vm)?,
            I32DivU => super::alu::visit_i32_div_u(vm)?,
            I32RemS => super::alu::visit_i32_rem_s(vm)?,
            I32RemU => super::alu::visit_i32_rem_u(vm)?,
            I32And => super::alu::visit_i32_and(vm),
            I32Or => super::alu::visit_i32_or(vm),
            I32Xor => super::alu::visit_i32_xor(vm),
            I32Shl => super::alu::visit_i32_shl(vm),
            I32ShrS => super::alu::visit_i32_shr_s(vm),
            I32ShrU => super::alu::visit_i32_shr_u(vm),
            I32Rotl => super::alu::visit_i32_rotl(vm),
            I32Rotr => super::alu::visit_i32_rotr(vm),
            I32WrapI64 => super::alu::visit_i32_wrap_i64(vm),
            I32Extend8S => super::alu::visit_i32_extend8_s(vm),
            I32Extend16S => super::alu::visit_i32_extend16_s(vm),

            // memory
            MemorySize => super::memory::visit_memory_size(vm),
            MemoryGrow => super::memory::visit_memory_grow(vm)?,
            MemoryFill => super::memory::visit_memory_fill(vm)?,
            MemoryCopy => super::memory::visit_memory_copy(vm)?,
            MemoryInit(imm) => super::memory::visit_memory_init(vm, imm)?,
            DataDrop(imm) => super::memory::visit_data_drop(vm, imm),
            I32Load(imm) => super::memory::visit_i32_load(vm, imm)?,
            I32Load8S(imm) => super::memory::visit_i32_load_i8_s(vm, imm)?,
            I32Load8U(imm) => super::memory::visit_i32_load_i8_u(vm, imm)?,
            I32Load16S(imm) => super::memory::visit_i32_load_i16_s(vm, imm)?,
            I32Load16U(imm) => super::memory::visit_i32_load_i16_u(vm, imm)?,
            I32Store(imm) => super::memory::visit_i32_store(vm, imm)?,
            I32Store8(imm) => super::memory::visit_i32_store_8(vm, imm)?,
            I32Store16(imm) => super::memory::visit_i32_store_16(vm, imm)?,

            // table
            TableSize(imm) => super::table::visit_table_size(vm, imm),
            TableGrow(imm) => super::table::visit_table_grow(vm, imm)?,
            TableFill(imm) => super::table::visit_table_fill(vm, imm)?,
            TableGet(imm) => super::table::visit_table_get(vm, imm)?,
            TableSet(imm) => super::table::visit_table_set(vm, imm)?,
            TableCopy(imm) => super::table::visit_table_copy(vm, imm)?,
            TableInit(imm) => super::table::visit_table_init(vm, imm)?,
            ElemDrop(imm) => super::table::visit_element_drop(vm, imm),

            // fpu
            #[cfg(feature = "fpu")]
            opcode => super::fpu::exec_fpu_opcode(vm, opcode)?,
        }
    }
}
