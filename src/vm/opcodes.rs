use crate::{
    types::{
        Opcode,
        OpcodeData,
        Pages,
        RwasmError,
        UntypedValue,
        FUNC_REF_NULL,
        FUNC_REF_OFFSET,
        N_MAX_RECURSION_DEPTH,
    },
    vm::{
        context::Caller,
        executor::RwasmExecutor,
        instr_ptr::InstructionPtr,
        table_entity::TableEntity,
    },
    OpcodeMeta,
};
use core::cmp;

pub(crate) fn run_the_loop<T>(vm: &mut RwasmExecutor<T>) -> Result<i32, RwasmError> {
    let floats_enabled = vm.config.floats_enabled;
    macro_rules! float_wrapper {
        ($expr:expr) => {{
            if !floats_enabled {
                return Err(RwasmError::FloatsAreDisabled);
            }
            $expr
        }};
    }
    while !vm.stop_exec {
        let instr = vm.ip.get();
        #[cfg(feature = "debug-print")]
        {
            let stack = vm.value_stack.dump_stack(vm.sp);
            println!(
                "{}:\t {:?} \tstack({}):{:?}",
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
        if vm.tracer.is_some() {
            let memory_size: u32 = vm.global_memory.current_pages().into();
            let consumed_fuel = vm.fuel_consumed();
            let stack = vm.value_stack.dump_stack(vm.sp);
            vm.tracer.as_mut().unwrap().pre_opcode_state(
                vm.ip.pc(),
                instr,
                vm.ip.data().clone(),
                stack,
                &OpcodeMeta {
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
            Unreachable => visit_unreachable_wrapped(vm),
            LocalGet => visit_local_get(vm),
            LocalSet => visit_local_set(vm),
            LocalTee => visit_local_tee(vm),
            Br => visit_br(vm),
            BrIfEqz => visit_br_if(vm),
            BrIfNez => visit_br_if_nez(vm),
            BrAdjust => visit_br_adjust(vm),
            BrAdjustIfNez => visit_br_adjust_if_nez(vm),
            BrTable => visit_br_table(vm),
            ConsumeFuel => visit_consume_fuel_wrapped(vm),
            Return => visit_return(vm),
            ReturnIfNez => visit_return_if_nez(vm),
            ReturnCallInternal => visit_return_call_internal_wrapped(vm),
            ReturnCall => visit_return_call_wrapped(vm),
            ReturnCallIndirect => visit_return_call_indirect_wrapped(vm),
            CallInternal => visit_call_internal_wrapped(vm),
            Call => visit_call_wrapped(vm),
            CallIndirect => visit_call_indirect_wrapped(vm),
            SignatureCheck => visit_signature_check_wrapped(vm),
            Drop => visit_drop(vm),
            Select => visit_select(vm),
            GlobalGet => visit_global_get(vm),
            GlobalSet => visit_global_set(vm),
            I32Load => visit_i32_load_wrapped(vm),
            I64Load => visit_i64_load_wrapped(vm),
            F32Load => float_wrapper!(visit_f32_load_wrapped(vm)),
            F64Load => float_wrapper!(visit_f64_load_wrapped(vm)),
            I32Load8S => visit_i32_load_i8_s_wrapped(vm),
            I32Load8U => visit_i32_load_i8_u_wrapped(vm),
            I32Load16S => visit_i32_load_i16_s_wrapped(vm),
            I32Load16U => visit_i32_load_i16_u_wrapped(vm),
            I64Load8S => visit_i64_load_i8_s_wrapped(vm),
            I64Load8U => visit_i64_load_i8_u_wrapped(vm),
            I64Load16S => visit_i64_load_i16_s_wrapped(vm),
            I64Load16U => visit_i64_load_i16_u_wrapped(vm),
            I64Load32S => visit_i64_load_i32_s_wrapped(vm),
            I64Load32U => visit_i64_load_i32_u_wrapped(vm),
            I32Store => visit_i32_store_wrapped(vm),
            I64Store => visit_i64_store_wrapped(vm),
            F32Store => float_wrapper!(visit_f32_store_wrapped(vm)),
            F64Store => float_wrapper!(visit_f64_store_wrapped(vm)),
            I32Store8 => visit_i32_store_8_wrapped(vm),
            I32Store16 => visit_i32_store_16_wrapped(vm),
            I64Store8 => visit_i64_store_8_wrapped(vm),
            I64Store16 => visit_i64_store_16_wrapped(vm),
            I64Store32 => visit_i64_store_32_wrapped(vm),
            MemorySize => visit_memory_size(vm),
            MemoryGrow => visit_memory_grow_wrapped(vm),
            MemoryFill => visit_memory_fill_wrapped(vm),
            MemoryCopy => visit_memory_copy_wrapped(vm),
            MemoryInit => visit_memory_init_wrapped(vm),
            DataDrop => visit_data_drop(vm),
            TableSize => visit_table_size(vm),
            TableGrow => visit_table_grow_wrapped(vm),
            TableFill => visit_table_fill_wrapped(vm),
            TableGet => visit_table_get_wrapped(vm),
            TableSet => visit_table_set_wrapped(vm),
            TableCopy => visit_table_copy_wrapped(vm),
            TableInit => visit_table_init_wrapped(vm),
            ElemDrop => visit_element_drop(vm),
            RefFunc => visit_ref_func(vm),
            I32Const => visit_i32_i64_const(vm),
            I64Const => visit_i32_i64_const(vm),
            F32Const => float_wrapper!(visit_i32_i64_const(vm)),
            F64Const => float_wrapper!(visit_i32_i64_const(vm)),
            I32Eqz => visit_i32_eqz(vm),
            I32Eq => visit_i32_eq(vm),
            I32Ne => visit_i32_ne(vm),
            I32LtS => visit_i32_lt_s(vm),
            I32LtU => visit_i32_lt_u(vm),
            I32GtS => visit_i32_gt_s(vm),
            I32GtU => visit_i32_gt_u(vm),
            I32LeS => visit_i32_le_s(vm),
            I32LeU => visit_i32_le_u(vm),
            I32GeS => visit_i32_ge_s(vm),
            I32GeU => visit_i32_ge_u(vm),
            I64Eqz => visit_i64_eqz(vm),
            I64Eq => visit_i64_eq(vm),
            I64Ne => visit_i64_ne(vm),
            I64LtS => visit_i64_lt_s(vm),
            I64LtU => visit_i64_lt_u(vm),
            I64GtS => visit_i64_gt_s(vm),
            I64GtU => visit_i64_gt_u(vm),
            I64LeS => visit_i64_le_s(vm),
            I64LeU => visit_i64_le_u(vm),
            I64GeS => visit_i64_ge_s(vm),
            I64GeU => visit_i64_ge_u(vm),
            F32Eq => float_wrapper!(visit_f32_eq(vm)),
            F32Ne => float_wrapper!(visit_f32_ne(vm)),
            F32Lt => float_wrapper!(visit_f32_lt(vm)),
            F32Gt => float_wrapper!(visit_f32_gt(vm)),
            F32Le => float_wrapper!(visit_f32_le(vm)),
            F32Ge => float_wrapper!(visit_f32_ge(vm)),
            F64Eq => float_wrapper!(visit_f64_eq(vm)),
            F64Ne => float_wrapper!(visit_f64_ne(vm)),
            F64Lt => float_wrapper!(visit_f64_lt(vm)),
            F64Gt => float_wrapper!(visit_f64_gt(vm)),
            F64Le => float_wrapper!(visit_f64_le(vm)),
            F64Ge => float_wrapper!(visit_f64_ge(vm)),
            I32Clz => visit_i32_clz(vm),
            I32Ctz => visit_i32_ctz(vm),
            I32Popcnt => visit_i32_popcnt(vm),
            I32Add => visit_i32_add(vm),
            I32Sub => visit_i32_sub(vm),
            I32Mul => visit_i32_mul(vm),
            I32DivS => visit_i32_div_s_wrapped(vm),
            I32DivU => visit_i32_div_u_wrapped(vm),
            I32RemS => visit_i32_rem_s_wrapped(vm),
            I32RemU => visit_i32_rem_u_wrapped(vm),
            I32And => visit_i32_and(vm),
            I32Or => visit_i32_or(vm),
            I32Xor => visit_i32_xor(vm),
            I32Shl => visit_i32_shl(vm),
            I32ShrS => visit_i32_shr_s(vm),
            I32ShrU => visit_i32_shr_u(vm),
            I32Rotl => visit_i32_rotl(vm),
            I32Rotr => visit_i32_rotr(vm),
            I64Clz => visit_i64_clz(vm),
            I64Ctz => visit_i64_ctz(vm),
            I64Popcnt => visit_i64_popcnt(vm),
            I64Add => visit_i64_add(vm),
            I64Sub => visit_i64_sub(vm),
            I64Mul => visit_i64_mul(vm),
            I64DivS => visit_i64_div_s_wrapped(vm),
            I64DivU => visit_i64_div_u_wrapped(vm),
            I64RemS => visit_i64_rem_s_wrapped(vm),
            I64RemU => visit_i64_rem_u_wrapped(vm),
            I64And => visit_i64_and(vm),
            I64Or => visit_i64_or(vm),
            I64Xor => visit_i64_xor(vm),
            I64Shl => visit_i64_shl(vm),
            I64ShrS => visit_i64_shr_s(vm),
            I64ShrU => visit_i64_shr_u(vm),
            I64Rotl => visit_i64_rotl(vm),
            I64Rotr => visit_i64_rotr(vm),
            F32Abs => float_wrapper!(visit_f32_abs(vm)),
            F32Neg => float_wrapper!(visit_f32_neg(vm)),
            F32Ceil => float_wrapper!(visit_f32_ceil(vm)),
            F32Floor => float_wrapper!(visit_f32_floor(vm)),
            F32Trunc => float_wrapper!(visit_f32_trunc(vm)),
            F32Nearest => float_wrapper!(visit_f32_nearest(vm)),
            F32Sqrt => float_wrapper!(visit_f32_sqrt(vm)),
            F32Add => float_wrapper!(visit_f32_add(vm)),
            F32Sub => float_wrapper!(visit_f32_sub(vm)),
            F32Mul => float_wrapper!(visit_f32_mul(vm)),
            F32Div => float_wrapper!(visit_f32_div(vm)),
            F32Min => float_wrapper!(visit_f32_min(vm)),
            F32Max => float_wrapper!(visit_f32_max(vm)),
            F32Copysign => float_wrapper!(visit_f32_copysign(vm)),
            F64Abs => float_wrapper!(visit_f64_abs(vm)),
            F64Neg => float_wrapper!(visit_f64_neg(vm)),
            F64Ceil => float_wrapper!(visit_f64_ceil(vm)),
            F64Floor => float_wrapper!(visit_f64_floor(vm)),
            F64Trunc => float_wrapper!(visit_f64_trunc(vm)),
            F64Nearest => float_wrapper!(visit_f64_nearest(vm)),
            F64Sqrt => float_wrapper!(visit_f64_sqrt(vm)),
            F64Add => float_wrapper!(visit_f64_add(vm)),
            F64Sub => float_wrapper!(visit_f64_sub(vm)),
            F64Mul => float_wrapper!(visit_f64_mul(vm)),
            F64Div => float_wrapper!(visit_f64_div(vm)),
            F64Min => float_wrapper!(visit_f64_min(vm)),
            F64Max => float_wrapper!(visit_f64_max(vm)),
            F64Copysign => float_wrapper!(visit_f64_copysign(vm)),
            I32WrapI64 => visit_i32_wrap_i64(vm),
            I32TruncF32S => float_wrapper!(visit_i32_trunc_f32_s_wrapped(vm)),
            I32TruncF32U => float_wrapper!(visit_i32_trunc_f32_u_wrapped(vm)),
            I32TruncF64S => float_wrapper!(visit_i32_trunc_f64_s_wrapped(vm)),
            I32TruncF64U => float_wrapper!(visit_i32_trunc_f64_u_wrapped(vm)),
            I64ExtendI32S => visit_i64_extend_i32_s(vm),
            I64ExtendI32U => visit_i64_extend_i32_u(vm),
            I64TruncF32S => float_wrapper!(visit_i64_trunc_f32_s_wrapped(vm)),
            I64TruncF32U => float_wrapper!(visit_i64_trunc_f32_u_wrapped(vm)),
            I64TruncF64S => float_wrapper!(visit_i64_trunc_f64_s_wrapped(vm)),
            I64TruncF64U => float_wrapper!(visit_i64_trunc_f64_u_wrapped(vm)),
            F32ConvertI32S => float_wrapper!(visit_f32_convert_i32_s(vm)),
            F32ConvertI32U => float_wrapper!(visit_f32_convert_i32_u(vm)),
            F32ConvertI64S => float_wrapper!(visit_f32_convert_i64_s(vm)),
            F32ConvertI64U => float_wrapper!(visit_f32_convert_i64_u(vm)),
            F32DemoteF64 => float_wrapper!(visit_f32_demote_f64(vm)),
            F64ConvertI32S => float_wrapper!(visit_f64_convert_i32_s(vm)),
            F64ConvertI32U => float_wrapper!(visit_f64_convert_i32_u(vm)),
            F64ConvertI64S => float_wrapper!(visit_f64_convert_i64_s(vm)),
            F64ConvertI64U => float_wrapper!(visit_f64_convert_i64_u(vm)),
            F64PromoteF32 => float_wrapper!(visit_f64_promote_f32(vm)),
            I32TruncSatF32S => float_wrapper!(visit_i32_trunc_sat_f32_s(vm)),
            I32TruncSatF32U => float_wrapper!(visit_i32_trunc_sat_f32_u(vm)),
            I32TruncSatF64S => float_wrapper!(visit_i32_trunc_sat_f64_s(vm)),
            I32TruncSatF64U => float_wrapper!(visit_i32_trunc_sat_f64_u(vm)),
            I64TruncSatF32S => float_wrapper!(visit_i64_trunc_sat_f32_s(vm)),
            I64TruncSatF32U => float_wrapper!(visit_i64_trunc_sat_f32_u(vm)),
            I64TruncSatF64S => float_wrapper!(visit_i64_trunc_sat_f64_s(vm)),
            I64TruncSatF64U => float_wrapper!(visit_i64_trunc_sat_f64_u(vm)),
            I32Extend8S => visit_i32_extend8_s(vm),
            I32Extend16S => visit_i32_extend16_s(vm),
            I64Extend8S => visit_i64_extend8_s(vm),
            I64Extend16S => visit_i64_extend16_s(vm),
            I64Extend32S => visit_i64_extend32_s(vm),
            StackAlloc => visit_stack_alloc_wrapped(vm),
        }
    }
    vm.stop_exec = false;
    vm.next_result
        .take()
        .unwrap_or_else(|| unreachable!("rwasm: next result without reason?"))
}

macro_rules! wrap_function_result {
    ($fn_name:ident) => {
        paste::paste! {
            #[inline(always)]
            pub(crate) fn [< $fn_name _wrapped >]<T>(vm: &mut RwasmExecutor<T>,) {
                if let Err(err) = $fn_name(vm, /* &mut ResourceLimiterRef<'_> */) {
                    vm.next_result = Some(Err(RwasmError::from(err)));
                    vm.stop_exec = true;
                }
            }
        }
    };
}

wrap_function_result!(visit_unreachable);
wrap_function_result!(visit_consume_fuel);
wrap_function_result!(visit_return_call_internal);
wrap_function_result!(visit_return_call);
wrap_function_result!(visit_return_call_indirect);
wrap_function_result!(visit_call_internal);
wrap_function_result!(visit_call);
wrap_function_result!(visit_call_indirect);
wrap_function_result!(visit_signature_check);
wrap_function_result!(visit_memory_grow);
wrap_function_result!(visit_memory_fill);
wrap_function_result!(visit_memory_copy);
wrap_function_result!(visit_memory_init);
wrap_function_result!(visit_table_grow);
wrap_function_result!(visit_table_fill);
wrap_function_result!(visit_table_get);
wrap_function_result!(visit_table_set);
wrap_function_result!(visit_table_copy);
wrap_function_result!(visit_table_init);
wrap_function_result!(visit_stack_alloc);

#[inline(always)]
pub(crate) fn visit_unreachable<T>(_vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    Err(RwasmError::UnreachableCodeReached)
}

#[inline(always)]
pub(crate) fn visit_local_get<T>(vm: &mut RwasmExecutor<T>) {
    let local_depth = match vm.ip.data() {
        OpcodeData::LocalDepth(local_depth) => local_depth,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let value = vm.sp.nth_back(local_depth.to_usize());
    vm.sp.push(value);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_local_set<T>(vm: &mut RwasmExecutor<T>) {
    let local_depth = match vm.ip.data() {
        OpcodeData::LocalDepth(local_depth) => local_depth,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let new_value = vm.sp.pop();
    vm.sp.set_nth_back(local_depth.to_usize(), new_value);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_local_tee<T>(vm: &mut RwasmExecutor<T>) {
    let local_depth = match vm.ip.data() {
        OpcodeData::LocalDepth(local_depth) => local_depth,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let new_value = vm.sp.last();
    vm.sp.set_nth_back(local_depth.to_usize(), new_value);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_br<T>(vm: &mut RwasmExecutor<T>) {
    let branch_offset = match vm.ip.data() {
        OpcodeData::BranchOffset(branch_offset) => branch_offset,
        _ => unreachable!("rwasm: missing instr data"),
    };
    vm.ip.offset(branch_offset.to_i32() as isize)
}

#[inline(always)]
pub(crate) fn visit_br_if<T>(vm: &mut RwasmExecutor<T>) {
    let branch_offset = match vm.ip.data() {
        OpcodeData::BranchOffset(branch_offset) => branch_offset,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let condition = vm.sp.pop_as();
    if condition {
        vm.ip.add(1);
    } else {
        vm.ip.offset(branch_offset.to_i32() as isize);
    }
}

#[inline(always)]
pub(crate) fn visit_br_if_nez<T>(vm: &mut RwasmExecutor<T>) {
    let branch_offset = match vm.ip.data() {
        OpcodeData::BranchOffset(branch_offset) => branch_offset,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let condition = vm.sp.pop_as();
    if condition {
        vm.ip.offset(branch_offset.to_i32() as isize);
    } else {
        vm.ip.add(1);
    }
}

#[inline(always)]
pub(crate) fn visit_br_adjust<T>(vm: &mut RwasmExecutor<T>) {
    let branch_offset = match vm.ip.data() {
        OpcodeData::BranchOffset(branch_offset) => branch_offset,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let drop_keep = vm.fetch_drop_keep(1);
    vm.sp.drop_keep(drop_keep);
    vm.ip.offset(branch_offset.to_i32() as isize);
}

#[inline(always)]
pub(crate) fn visit_br_adjust_if_nez<T>(vm: &mut RwasmExecutor<T>) {
    let branch_offset = match vm.ip.data() {
        OpcodeData::BranchOffset(branch_offset) => branch_offset,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let condition = vm.sp.pop_as();
    if condition {
        let drop_keep = vm.fetch_drop_keep(1);
        vm.sp.drop_keep(drop_keep);
        vm.ip.offset(branch_offset.to_i32() as isize);
    } else {
        vm.ip.add(2);
    }
}

#[inline(always)]
pub(crate) fn visit_br_table<T>(vm: &mut RwasmExecutor<T>) {
    let targets = match vm.ip.data() {
        OpcodeData::BranchTableTargets(targets) => targets,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let index: u32 = vm.sp.pop_as();
    let max_index = targets.to_usize() - 1;
    let normalized_index = cmp::min(index as usize, max_index);
    vm.ip.add(2 * normalized_index + 1);
}

#[inline(always)]
pub(crate) fn visit_consume_fuel<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let block_fuel = match vm.ip.data() {
        OpcodeData::BlockFuel(block_fuel) => block_fuel,
        _ => unreachable!("rwasm: missing instr data"),
    };
    if vm.config.fuel_enabled {
        vm.try_consume_fuel(block_fuel.to_u64())?;
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_return<T>(vm: &mut RwasmExecutor<T>) {
    let drop_keep = match vm.ip.data() {
        OpcodeData::DropKeep(drop_keep) => drop_keep,
        _ => unreachable!("rwasm: missing instr data"),
    };
    vm.sp.drop_keep(*drop_keep);
    vm.value_stack.sync_stack_ptr(vm.sp);
    match vm.call_stack.pop() {
        Some(caller) => {
            vm.ip = caller;
        }
        None => {
            vm.next_result = Some(Ok(0));
            vm.stop_exec = true;
        }
    }
}

#[inline(always)]
pub(crate) fn visit_return_if_nez<T>(vm: &mut RwasmExecutor<T>) {
    let drop_keep = match vm.ip.data() {
        OpcodeData::DropKeep(drop_keep) => drop_keep,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let condition = vm.sp.pop_as();
    if condition {
        vm.sp.drop_keep(*drop_keep);
        vm.value_stack.sync_stack_ptr(vm.sp);
        match vm.call_stack.pop() {
            Some(caller) => {
                vm.ip = caller;
            }
            None => {
                vm.next_result = Some(Ok(0));
                vm.stop_exec = true;
            }
        }
    } else {
        vm.ip.add(1);
    }
}

#[inline(always)]
pub(crate) fn visit_return_call_internal<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let func_idx = match vm.ip.data() {
        OpcodeData::CompiledFunc(func_idx) => *func_idx,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let drop_keep = vm.fetch_drop_keep(1);
    vm.sp.drop_keep(drop_keep);
    vm.ip.add(2);
    vm.value_stack.sync_stack_ptr(vm.sp);
    let instr_ref = vm
        .module
        .func_section
        .get(func_idx as usize)
        .copied()
        .expect("rwasm: unknown internal function");
    vm.sp = vm.value_stack.stack_ptr();
    vm.ip = InstructionPtr::new(vm.module.code_section.instr.as_ptr());
    vm.ip.add(instr_ref as usize);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_return_call<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let func_idx = match vm.ip.data() {
        OpcodeData::CompiledFunc(func_idx) => *func_idx,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let drop_keep = vm.fetch_drop_keep(1);
    vm.sp.drop_keep(drop_keep);
    vm.value_stack.sync_stack_ptr(vm.sp);
    // external call can cause interruption,
    // that is why it's important to increase IP before doing the call
    vm.ip.add(2);
    (vm.syscall_handler)(Caller::new(vm), func_idx)?;
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_return_call_indirect<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let signature_idx = match vm.ip.data() {
        OpcodeData::SignatureIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let drop_keep = vm.fetch_drop_keep(1);
    let table = vm.fetch_table_index(2);
    let func_index: u32 = vm.sp.pop_as();
    vm.sp.drop_keep(drop_keep);
    vm.last_signature = Some(signature_idx);
    let func_idx: u32 = vm
        .tables
        .get(&table)
        .expect("rwasm: unresolved table index")
        .get_untyped(func_index)
        .ok_or(RwasmError::TableOutOfBounds)?
        .try_into()
        .unwrap();
    if func_idx == 0 {
        return Err(RwasmError::IndirectCallToNull.into());
    }
    let func_idx = func_idx - FUNC_REF_OFFSET;
    vm.execute_call_internal(false, 3, func_idx)
}

#[inline(always)]
pub(crate) fn visit_call_internal<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let func_idx = match vm.ip.data() {
        OpcodeData::CompiledFunc(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    vm.ip.add(1);
    vm.value_stack.sync_stack_ptr(vm.sp);
    if vm.call_stack.len() > N_MAX_RECURSION_DEPTH {
        return Err(RwasmError::StackOverflow);
    }
    vm.call_stack.push(vm.ip);
    let instr_ref = vm
        .module
        .func_section
        .get(func_idx as usize)
        .copied()
        .expect("rwasm: unknown internal function");
    vm.sp = vm.value_stack.stack_ptr();
    vm.ip = InstructionPtr::new(vm.module.code_section.instr.as_ptr());
    vm.ip.add(instr_ref as usize);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_call<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let func_idx = match vm.ip.data() {
        OpcodeData::FuncIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    vm.value_stack.sync_stack_ptr(vm.sp);
    // external call can cause interruption,
    // that is why it's important to increase IP before doing the call
    vm.ip.add(1);
    (vm.syscall_handler)(Caller::new(vm), func_idx.to_u32())
}

#[inline(always)]
pub(crate) fn visit_call_indirect<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let signature_idx = match vm.ip.data() {
        OpcodeData::SignatureIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    // resolve func index
    let table = vm.fetch_table_index(1);
    let func_index: u32 = vm.sp.pop_as();
    vm.last_signature = Some(signature_idx);
    let func_idx = vm
        .tables
        .get(&table)
        .expect("rwasm: unresolved table index")
        .get_untyped(func_index)
        .map(|v| v.as_u32())
        .ok_or(RwasmError::TableOutOfBounds)?;
    if func_idx == FUNC_REF_NULL {
        return Err(RwasmError::IndirectCallToNull);
    }
    let func_idx = func_idx - FUNC_REF_OFFSET;
    // call func
    vm.ip.add(2);
    vm.value_stack.sync_stack_ptr(vm.sp);
    if vm.call_stack.len() > N_MAX_RECURSION_DEPTH {
        return Err(RwasmError::StackOverflow);
    }
    vm.call_stack.push(vm.ip);
    let instr_ref = vm
        .module
        .func_section
        .get(func_idx as usize)
        .copied()
        .expect("rwasm: unknown internal function");
    vm.sp = vm.value_stack.stack_ptr();
    vm.ip = InstructionPtr::new(vm.module.code_section.instr.as_ptr());
    vm.ip.add(instr_ref as usize);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_signature_check<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let signature_idx = match vm.ip.data() {
        OpcodeData::SignatureIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    if let Some(actual_signature) = vm.last_signature.take() {
        if actual_signature != signature_idx {
            return Err(RwasmError::BadSignature);
        }
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_drop<T>(vm: &mut RwasmExecutor<T>) {
    vm.sp.drop();
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_select<T>(vm: &mut RwasmExecutor<T>) {
    vm.sp.eval_top3(|e1, e2, e3| {
        let condition = <bool as From<UntypedValue>>::from(e3);
        if condition {
            e1
        } else {
            e2
        }
    });
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_global_get<T>(vm: &mut RwasmExecutor<T>) {
    let global_idx = match vm.ip.data() {
        OpcodeData::GlobalIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let global_value = vm
        .global_variables
        .get(&global_idx)
        .copied()
        .unwrap_or_default();
    vm.sp.push(global_value);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_global_set<T>(vm: &mut RwasmExecutor<T>) {
    let global_idx = match vm.ip.data() {
        OpcodeData::GlobalIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let new_value = vm.sp.pop();
    vm.global_variables.insert(global_idx, new_value);
    vm.ip.add(1);
}

macro_rules! impl_visit_load {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
                let offset = match vm.ip.data() {
                    OpcodeData::AddressOffset(value) => *value,
                    _ => unreachable!("rwasm: missing instr data"),
                };
                vm.execute_load_extend(offset, UntypedValue::$untyped_ident)
            }
            wrap_function_result!($visit_ident);
        )*
    }
}

impl_visit_load! {
    fn visit_i32_load(i32_load);
    fn visit_i64_load(i64_load);
    fn visit_f32_load(f32_load);
    fn visit_f64_load(f64_load);

    fn visit_i32_load_i8_s(i32_load8_s);
    fn visit_i32_load_i8_u(i32_load8_u);
    fn visit_i32_load_i16_s(i32_load16_s);
    fn visit_i32_load_i16_u(i32_load16_u);

    fn visit_i64_load_i8_s(i64_load8_s);
    fn visit_i64_load_i8_u(i64_load8_u);
    fn visit_i64_load_i16_s(i64_load16_s);
    fn visit_i64_load_i16_u(i64_load16_u);
    fn visit_i64_load_i32_s(i64_load32_s);
    fn visit_i64_load_i32_u(i64_load32_u);
}

macro_rules! impl_visit_store {
    ( $( fn $visit_ident:ident($untyped_ident:ident, $type_size:literal); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
                let offset = match vm.ip.data() {
                    OpcodeData::AddressOffset(value) => *value,
                    _ => unreachable!("rwasm: missing instr data"),
                };
                vm.execute_store_wrap(offset, UntypedValue::$untyped_ident, $type_size)
            }
            wrap_function_result!($visit_ident);
        )*
    }
}

impl_visit_store! {
    fn visit_i32_store(i32_store, 4);
    fn visit_i64_store(i64_store, 8);
    fn visit_f32_store(f32_store, 4);
    fn visit_f64_store(f64_store, 8);

    fn visit_i32_store_8(i32_store8, 1);
    fn visit_i32_store_16(i32_store16, 2);

    fn visit_i64_store_8(i64_store8, 1);
    fn visit_i64_store_16(i64_store16, 2);
    fn visit_i64_store_32(i64_store32, 4);
}

#[inline(always)]
pub(crate) fn visit_memory_size<T>(vm: &mut RwasmExecutor<T>) {
    let result: u32 = vm.global_memory.current_pages().into();
    vm.sp.push_as(result);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_memory_grow<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let delta: u32 = vm.sp.pop_as();
    let delta = match Pages::new(delta) {
        Some(delta) => delta,
        None => {
            vm.sp.push_as(u32::MAX);
            vm.ip.add(1);
            return Ok(());
        }
    };
    if vm.config.fuel_enabled {
        let delta_in_bytes = delta.to_bytes().unwrap_or(0) as u64;
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_bytes(delta_in_bytes))?;
    }
    let new_pages = vm
        .global_memory
        .grow(delta)
        .map(u32::from)
        .unwrap_or(u32::MAX);
    vm.sp.push_as(new_pages);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_memory_fill<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let (d, val, n) = vm.sp.pop3();
    let n = i32::from(n) as usize;
    let offset = i32::from(d) as usize;
    let byte = u8::from(val);
    if vm.config.fuel_enabled {
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_bytes(n as u64))?;
    }
    let memory = vm
        .global_memory
        .data_mut()
        .get_mut(offset..)
        .and_then(|memory| memory.get_mut(..n))
        .ok_or(RwasmError::MemoryOutOfBounds)?;
    memory.fill(byte);
    if let Some(tracer) = vm.tracer.as_mut() {
        tracer.memory_change(offset as u32, n as u32, memory);
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_memory_copy<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let (d, s, n) = vm.sp.pop3();
    let n = i32::from(n) as usize;
    let src_offset = i32::from(s) as usize;
    let dst_offset = i32::from(d) as usize;
    if vm.config.fuel_enabled {
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_bytes(n as u64))?;
    }
    // these accesses just perform the bound checks required by the Wasm spec.
    let data = vm.global_memory.data_mut();
    data.get(src_offset..)
        .and_then(|memory| memory.get(..n))
        .ok_or(RwasmError::MemoryOutOfBounds)?;
    data.get(dst_offset..)
        .and_then(|memory| memory.get(..n))
        .ok_or(RwasmError::MemoryOutOfBounds)?;
    data.copy_within(src_offset..src_offset.wrapping_add(n), dst_offset);
    if let Some(tracer) = vm.tracer.as_mut() {
        tracer.memory_change(
            dst_offset as u32,
            n as u32,
            &data[dst_offset..(dst_offset + n)],
        );
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_memory_init<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let data_segment_idx = match vm.ip.data() {
        OpcodeData::DataSegmentIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };

    let is_empty_data_segment = vm
        .empty_data_segments
        .get(data_segment_idx.to_u32() as usize)
        .as_deref()
        .copied()
        .unwrap_or(false);

    let (d, s, n) = vm.sp.pop3();
    let n = i32::from(n) as usize;
    let src_offset = i32::from(s) as usize;
    let dst_offset = i32::from(d) as usize;
    if vm.config.fuel_enabled {
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_bytes(n as u64))?;
    }
    let memory = vm
        .global_memory
        .data_mut()
        .get_mut(dst_offset..)
        .and_then(|memory| memory.get_mut(..n))
        .ok_or(RwasmError::MemoryOutOfBounds)?;
    let mut memory_section = vm.module.memory_section.as_slice();
    if is_empty_data_segment {
        memory_section = &[];
    }
    let data = memory_section
        .get(src_offset..)
        .and_then(|data| data.get(..n))
        .ok_or(RwasmError::MemoryOutOfBounds)?;
    memory.copy_from_slice(data);
    if let Some(tracer) = vm.tracer.as_mut() {
        tracer.global_memory(dst_offset as u32, n as u32, memory);
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_data_drop<T>(vm: &mut RwasmExecutor<T>) {
    let data_segment_idx = match vm.ip.data() {
        OpcodeData::DataSegmentIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    vm.empty_data_segments
        .set(data_segment_idx.to_u32() as usize, true);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_table_size<T>(vm: &mut RwasmExecutor<T>) {
    let table_idx = match vm.ip.data() {
        OpcodeData::TableIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let table_size = vm
        .tables
        .get(&table_idx)
        .expect("rwasm: unresolved table segment")
        .size();
    vm.sp.push_as(table_size);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_table_grow<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let table_idx = match vm.ip.data() {
        OpcodeData::TableIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let (init, delta) = vm.sp.pop2();
    let delta: u32 = delta.into();
    if vm.config.fuel_enabled {
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_elements(delta as u64))?;
    }
    let table = vm.tables.entry(table_idx).or_insert_with(TableEntity::new);
    let result = table.grow_untyped(delta, init);
    vm.sp.push_as(result);
    if let Some(tracer) = vm.tracer.as_mut() {
        tracer.table_size_change(table_idx.to_u32(), init.into(), delta);
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_table_fill<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let table_idx = match vm.ip.data() {
        OpcodeData::TableIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let (i, val, n) = vm.sp.pop3();
    if vm.config.fuel_enabled {
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_elements(n.into()))?;
    }
    vm.tables
        .get_mut(&table_idx)
        .expect("rwasm: missing table")
        .fill_untyped(i.into(), val, n.into())?;
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_table_get<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let table_idx = match vm.ip.data() {
        OpcodeData::TableIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let index = vm.sp.pop();
    let value = vm
        .tables
        .get_mut(&table_idx)
        .expect("rwasm: missing table")
        .get_untyped(index.into())
        .ok_or(RwasmError::TableOutOfBounds)?;
    vm.sp.push(value);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_table_set<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let table_idx = match vm.ip.data() {
        OpcodeData::TableIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let (index, value) = vm.sp.pop2();
    vm.tables
        .get_mut(&table_idx)
        .expect("rwasm: missing table")
        .set_untyped(index.into(), value)
        .map_err(|_| RwasmError::TableOutOfBounds)?;
    if let Some(tracer) = vm.tracer.as_mut() {
        tracer.table_change(table_idx.to_u32(), index.into(), value);
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_table_copy<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let dst_table_idx = match vm.ip.data() {
        OpcodeData::TableIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let src_table_idx = vm.fetch_table_index(1);
    let (d, s, n) = vm.sp.pop3();
    let len = u32::from(n);
    let src_index = u32::from(s);
    let dst_index = u32::from(d);
    if vm.config.fuel_enabled {
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_elements(len as u64))?;
    }
    // Query both tables and check if they are the same:
    if src_table_idx != dst_table_idx {
        let [src, dst] = vm
            .tables
            .get_many_mut([&src_table_idx, &dst_table_idx])
            .map(|v| v.expect("rwasm: unresolved table segment"));
        TableEntity::copy(dst, dst_index, src, src_index, len)?;
    } else {
        let src = vm
            .tables
            .get_mut(&src_table_idx)
            .expect("rwasm: unresolved table segment");
        src.copy_within(dst_index, src_index, len)?;
    }
    vm.ip.add(2);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_table_init<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let element_segment_idx = match vm.ip.data() {
        OpcodeData::ElementSegmentIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    let table_idx = vm.fetch_table_index(1);

    let (d, s, n) = vm.sp.pop3();
    let len = u32::from(n);
    let src_index = u32::from(s);
    let dst_index = u32::from(d);

    if vm.config.fuel_enabled {
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_elements(len as u64))?;
    }

    // There is a trick with `element_segment_idx`:
    // it refers to the segment number.
    // However, in rwasm, all elements are stored in segment 0,
    // so there is no need to store information about the remaining segments.
    // According to the WebAssembly standards, though,
    // we must retain information about all dropped element segments
    // to perform an emptiness check.
    // Therefore, in `element_segment_idx`, we store the original index,
    // which is always > 0.
    let is_empty_segment = vm
        .empty_elements_segments
        .get(element_segment_idx.to_u32() as usize)
        .as_deref()
        .copied()
        .unwrap_or(false);

    let mut module_elements_section = &vm.default_elements_segment[..];
    if is_empty_segment {
        module_elements_section = &[];
    }
    let table = vm.tables.get_mut(&table_idx).expect("rwasm: missing table");
    table.init_untyped(dst_index, module_elements_section, src_index, len)?;

    vm.ip.add(2);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_element_drop<T>(vm: &mut RwasmExecutor<T>) {
    let element_segment_idx = match vm.ip.data() {
        OpcodeData::ElementSegmentIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    vm.empty_elements_segments
        .set(element_segment_idx.to_u32() as usize, true);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_ref_func<T>(vm: &mut RwasmExecutor<T>) {
    let func_idx = match vm.ip.data() {
        OpcodeData::FuncIdx(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    vm.sp.push_as(func_idx.to_u32() + FUNC_REF_OFFSET);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_i32_i64_const<T>(vm: &mut RwasmExecutor<T>) {
    let untyped_value = match vm.ip.data() {
        OpcodeData::UntypedValue(value) => *value,
        _ => unreachable!("rwasm: missing instr data"),
    };
    vm.sp.push(untyped_value);
    vm.ip.add(1);
}

macro_rules! impl_visit_unary {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident<T>(exec: &mut RwasmExecutor<T>) {
                exec.execute_unary(UntypedValue::$untyped_ident)
            }
        )*
    }
}

impl_visit_unary! {
    fn visit_i32_eqz(i32_eqz);
    fn visit_i64_eqz(i64_eqz);

    fn visit_i32_clz(i32_clz);
    fn visit_i32_ctz(i32_ctz);
    fn visit_i32_popcnt(i32_popcnt);

    fn visit_i64_clz(i64_clz);
    fn visit_i64_ctz(i64_ctz);
    fn visit_i64_popcnt(i64_popcnt);

    fn visit_f32_abs(f32_abs);
    fn visit_f32_neg(f32_neg);
    fn visit_f32_ceil(f32_ceil);
    fn visit_f32_floor(f32_floor);
    fn visit_f32_trunc(f32_trunc);
    fn visit_f32_nearest(f32_nearest);
    fn visit_f32_sqrt(f32_sqrt);

    fn visit_f64_abs(f64_abs);
    fn visit_f64_neg(f64_neg);
    fn visit_f64_ceil(f64_ceil);
    fn visit_f64_floor(f64_floor);
    fn visit_f64_trunc(f64_trunc);
    fn visit_f64_nearest(f64_nearest);
    fn visit_f64_sqrt(f64_sqrt);

    fn visit_i32_wrap_i64(i32_wrap_i64);
    fn visit_i64_extend_i32_s(i64_extend_i32_s);
    fn visit_i64_extend_i32_u(i64_extend_i32_u);

    fn visit_f32_convert_i32_s(f32_convert_i32_s);
    fn visit_f32_convert_i32_u(f32_convert_i32_u);
    fn visit_f32_convert_i64_s(f32_convert_i64_s);
    fn visit_f32_convert_i64_u(f32_convert_i64_u);
    fn visit_f32_demote_f64(f32_demote_f64);
    fn visit_f64_convert_i32_s(f64_convert_i32_s);
    fn visit_f64_convert_i32_u(f64_convert_i32_u);
    fn visit_f64_convert_i64_s(f64_convert_i64_s);
    fn visit_f64_convert_i64_u(f64_convert_i64_u);
    fn visit_f64_promote_f32(f64_promote_f32);

    fn visit_i32_extend8_s(i32_extend8_s);
    fn visit_i32_extend16_s(i32_extend16_s);
    fn visit_i64_extend8_s(i64_extend8_s);
    fn visit_i64_extend16_s(i64_extend16_s);
    fn visit_i64_extend32_s(i64_extend32_s);

    fn visit_i32_trunc_sat_f32_s(i32_trunc_sat_f32_s);
    fn visit_i32_trunc_sat_f32_u(i32_trunc_sat_f32_u);
    fn visit_i32_trunc_sat_f64_s(i32_trunc_sat_f64_s);
    fn visit_i32_trunc_sat_f64_u(i32_trunc_sat_f64_u);
    fn visit_i64_trunc_sat_f32_s(i64_trunc_sat_f32_s);
    fn visit_i64_trunc_sat_f32_u(i64_trunc_sat_f32_u);
    fn visit_i64_trunc_sat_f64_s(i64_trunc_sat_f64_s);
    fn visit_i64_trunc_sat_f64_u(i64_trunc_sat_f64_u);
}

macro_rules! impl_visit_fallible_unary {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
                vm.try_execute_unary(UntypedValue::$untyped_ident)
            }
            wrap_function_result!($visit_ident);
        )*
    }
}

impl_visit_fallible_unary! {
    fn visit_i32_trunc_f32_s(i32_trunc_f32_s);
    fn visit_i32_trunc_f32_u(i32_trunc_f32_u);
    fn visit_i32_trunc_f64_s(i32_trunc_f64_s);
    fn visit_i32_trunc_f64_u(i32_trunc_f64_u);

    fn visit_i64_trunc_f32_s(i64_trunc_f32_s);
    fn visit_i64_trunc_f32_u(i64_trunc_f32_u);
    fn visit_i64_trunc_f64_s(i64_trunc_f64_s);
    fn visit_i64_trunc_f64_u(i64_trunc_f64_u);
}

macro_rules! impl_visit_binary {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident<T>(vm: &mut RwasmExecutor<T>) {
                vm.execute_binary(UntypedValue::$untyped_ident)
            }
        )*
    }
}

impl_visit_binary! {
    fn visit_i32_eq(i32_eq);
    fn visit_i32_ne(i32_ne);
    fn visit_i32_lt_s(i32_lt_s);
    fn visit_i32_lt_u(i32_lt_u);
    fn visit_i32_gt_s(i32_gt_s);
    fn visit_i32_gt_u(i32_gt_u);
    fn visit_i32_le_s(i32_le_s);
    fn visit_i32_le_u(i32_le_u);
    fn visit_i32_ge_s(i32_ge_s);
    fn visit_i32_ge_u(i32_ge_u);

    fn visit_i64_eq(i64_eq);
    fn visit_i64_ne(i64_ne);
    fn visit_i64_lt_s(i64_lt_s);
    fn visit_i64_lt_u(i64_lt_u);
    fn visit_i64_gt_s(i64_gt_s);
    fn visit_i64_gt_u(i64_gt_u);
    fn visit_i64_le_s(i64_le_s);
    fn visit_i64_le_u(i64_le_u);
    fn visit_i64_ge_s(i64_ge_s);
    fn visit_i64_ge_u(i64_ge_u);

    fn visit_f32_eq(f32_eq);
    fn visit_f32_ne(f32_ne);
    fn visit_f32_lt(f32_lt);
    fn visit_f32_gt(f32_gt);
    fn visit_f32_le(f32_le);
    fn visit_f32_ge(f32_ge);

    fn visit_f64_eq(f64_eq);
    fn visit_f64_ne(f64_ne);
    fn visit_f64_lt(f64_lt);
    fn visit_f64_gt(f64_gt);
    fn visit_f64_le(f64_le);
    fn visit_f64_ge(f64_ge);

    fn visit_i32_add(i32_add);
    fn visit_i32_sub(i32_sub);
    fn visit_i32_mul(i32_mul);
    fn visit_i32_and(i32_and);
    fn visit_i32_or(i32_or);
    fn visit_i32_xor(i32_xor);
    fn visit_i32_shl(i32_shl);
    fn visit_i32_shr_s(i32_shr_s);
    fn visit_i32_shr_u(i32_shr_u);
    fn visit_i32_rotl(i32_rotl);
    fn visit_i32_rotr(i32_rotr);

    fn visit_i64_add(i64_add);
    fn visit_i64_sub(i64_sub);
    fn visit_i64_mul(i64_mul);
    fn visit_i64_and(i64_and);
    fn visit_i64_or(i64_or);
    fn visit_i64_xor(i64_xor);
    fn visit_i64_shl(i64_shl);
    fn visit_i64_shr_s(i64_shr_s);
    fn visit_i64_shr_u(i64_shr_u);
    fn visit_i64_rotl(i64_rotl);
    fn visit_i64_rotr(i64_rotr);

    fn visit_f32_add(f32_add);
    fn visit_f32_sub(f32_sub);
    fn visit_f32_mul(f32_mul);
    fn visit_f32_div(f32_div);
    fn visit_f32_min(f32_min);
    fn visit_f32_max(f32_max);
    fn visit_f32_copysign(f32_copysign);

    fn visit_f64_add(f64_add);
    fn visit_f64_sub(f64_sub);
    fn visit_f64_mul(f64_mul);
    fn visit_f64_div(f64_div);
    fn visit_f64_min(f64_min);
    fn visit_f64_max(f64_max);
    fn visit_f64_copysign(f64_copysign);
}

macro_rules! impl_visit_fallible_binary {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
                vm.try_execute_binary(UntypedValue::$untyped_ident)
            }
            wrap_function_result!($visit_ident);
        )*
    }
}

impl_visit_fallible_binary! {
    fn visit_i32_div_s(i32_div_s);
    fn visit_i32_div_u(i32_div_u);
    fn visit_i32_rem_s(i32_rem_s);
    fn visit_i32_rem_u(i32_rem_u);

    fn visit_i64_div_s(i64_div_s);
    fn visit_i64_div_u(i64_div_u);
    fn visit_i64_rem_s(i64_rem_s);
    fn visit_i64_rem_u(i64_rem_u);
}

#[inline(always)]
pub(crate) fn visit_stack_alloc<T>(vm: &mut RwasmExecutor<T>) -> Result<(), RwasmError> {
    let max_stack_height = match vm.ip.data() {
        OpcodeData::StackAlloc(value) => value.max_stack_height,
        _ => unreachable!("rwasm: missing instr data"),
    };
    vm.value_stack.reserve(max_stack_height as usize)?;
    vm.ip.add(1);
    Ok(())
}
