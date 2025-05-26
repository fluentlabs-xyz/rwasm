use crate::{
    types::{Opcode, Pages, UntypedValue, N_MAX_RECURSION_DEPTH},
    vm::{
        context::Caller,
        executor::RwasmExecutor,
        instr_ptr::InstructionPtr,
        table_entity::TableEntity,
    },
    AddressOffset,
    BlockFuel,
    BranchOffset,
    BranchTableTargets,
    CompiledFunc,
    DataSegmentIdx,
    ElementSegmentIdx,
    GlobalIdx,
    LocalDepth,
    MaxStackHeight,
    OpcodeMeta,
    SignatureIdx,
    SysFuncIdx,
    TableIdx,
    TrapCode,
    NULL_FUNC_IDX,
};
use core::cmp;

pub(crate) fn run_the_loop<T>(vm: &mut RwasmExecutor<T>) -> Result<i32, TrapCode> {
    let floats_enabled = vm.config.floats_enabled;
    macro_rules! float_wrapper {
        ($func_name:ident, $imm:expr) => {{
            if !floats_enabled {
                return Err(TrapCode::FloatsAreDisabled);
            }
            $crate::vm::executor::fpu::$func_name(vm, $imm)?
        }};
        ($func_name:ident) => {{
            if !floats_enabled {
                return Err(TrapCode::FloatsAreDisabled);
            }
            $crate::vm::executor::fpu::$func_name(vm)?
        }};
    }
    while !vm.stop_exec {
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
        if vm.tracer.is_some() {
            let memory_size: u32 = vm.global_memory.current_pages().into();
            let consumed_fuel = vm.fuel_consumed();
            let stack = vm.value_stack.dump_stack(vm.sp);
            vm.tracer.as_mut().unwrap().pre_opcode_state(
                vm.ip.pc(),
                instr,
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
            Unreachable => visit_unreachable(vm)?,
            LocalGet(imm) => visit_local_get(vm, imm),
            LocalSet(imm) => visit_local_set(vm, imm),
            LocalTee(imm) => visit_local_tee(vm, imm),
            Br(imm) => visit_br(vm, imm),
            BrIfEqz(imm) => visit_br_if(vm, imm),
            BrIfNez(imm) => visit_br_if_nez(vm, imm),
            BrTable(imm) => visit_br_table(vm, imm),
            ConsumeFuel(imm) => visit_consume_fuel(vm, imm)?,
            Return => visit_return(vm),
            ReturnCallInternal(imm) => visit_return_call_internal(vm, imm)?,
            ReturnCall(imm) => visit_return_call(vm, imm)?,
            ReturnCallIndirect(imm) => visit_return_call_indirect(vm, imm)?,
            CallInternal(imm) => visit_call_internal(vm, imm)?,
            Call(imm) => visit_call(vm, imm)?,
            CallIndirect(imm) => visit_call_indirect(vm, imm)?,
            SignatureCheck(imm) => visit_signature_check(vm, imm)?,
            StackCheck(imm) => visit_stack_alloc(vm, imm)?,
            Drop => visit_drop(vm),
            Select => visit_select(vm),
            GlobalGet(imm) => visit_global_get(vm, imm),
            GlobalSet(imm) => visit_global_set(vm, imm),
            I32Load(imm) => visit_i32_load(vm, imm)?,
            I32Load8S(imm) => visit_i32_load_i8_s(vm, imm)?,
            I32Load8U(imm) => visit_i32_load_i8_u(vm, imm)?,
            I32Load16S(imm) => visit_i32_load_i16_s(vm, imm)?,
            I32Load16U(imm) => visit_i32_load_i16_u(vm, imm)?,
            I32Store(imm) => visit_i32_store(vm, imm)?,
            I32Store8(imm) => visit_i32_store_8(vm, imm)?,
            I32Store16(imm) => visit_i32_store_16(vm, imm)?,
            MemorySize => visit_memory_size(vm),
            MemoryGrow => visit_memory_grow(vm)?,
            MemoryFill => visit_memory_fill(vm)?,
            MemoryCopy => visit_memory_copy(vm)?,
            MemoryInit(imm) => visit_memory_init(vm, imm)?,
            DataDrop(imm) => visit_data_drop(vm, imm),
            TableSize(imm) => visit_table_size(vm, imm),
            TableGrow(imm) => visit_table_grow(vm, imm)?,
            TableFill(imm) => visit_table_fill(vm, imm)?,
            TableGet(imm) => visit_table_get(vm, imm)?,
            TableSet(imm) => visit_table_set(vm, imm)?,
            TableCopy(imm) => visit_table_copy(vm, imm)?,
            TableInit(imm) => visit_table_init(vm, imm)?,
            ElemDrop(imm) => visit_element_drop(vm, imm),
            RefFunc(imm) => visit_ref_func(vm, imm),
            I32Const(imm) => visit_i32_i64_const(vm, imm),
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

            I32Clz => visit_i32_clz(vm),
            I32Ctz => visit_i32_ctz(vm),
            I32Popcnt => visit_i32_popcnt(vm),
            I32Add => visit_i32_add(vm),
            I32Sub => visit_i32_sub(vm),
            I32Mul => visit_i32_mul(vm),
            I32DivS => visit_i32_div_s(vm)?,
            I32DivU => visit_i32_div_u(vm)?,
            I32RemS => visit_i32_rem_s(vm)?,
            I32RemU => visit_i32_rem_u(vm)?,
            I32And => visit_i32_and(vm),
            I32Or => visit_i32_or(vm),
            I32Xor => visit_i32_xor(vm),
            I32Shl => visit_i32_shl(vm),
            I32ShrS => visit_i32_shr_s(vm),
            I32ShrU => visit_i32_shr_u(vm),
            I32Rotl => visit_i32_rotl(vm),
            I32Rotr => visit_i32_rotr(vm),
            I32WrapI64 => visit_i32_wrap_i64(vm),
            I32Extend8S => visit_i32_extend8_s(vm),
            I32Extend16S => visit_i32_extend16_s(vm),

            F32Load(imm) => float_wrapper!(visit_f32_load, imm),
            F64Load(imm) => float_wrapper!(visit_f64_load, imm),
            F32Store(imm) => float_wrapper!(visit_f32_store, imm),
            F64Store(imm) => float_wrapper!(visit_f64_store, imm),
            F32Eq => float_wrapper!(visit_f32_eq),
            F32Ne => float_wrapper!(visit_f32_ne),
            F32Lt => float_wrapper!(visit_f32_lt),
            F32Gt => float_wrapper!(visit_f32_gt),
            F32Le => float_wrapper!(visit_f32_le),
            F32Ge => float_wrapper!(visit_f32_ge),
            F64Eq => float_wrapper!(visit_f64_eq),
            F64Ne => float_wrapper!(visit_f64_ne),
            F64Lt => float_wrapper!(visit_f64_lt),
            F64Gt => float_wrapper!(visit_f64_gt),
            F64Le => float_wrapper!(visit_f64_le),
            F64Ge => float_wrapper!(visit_f64_ge),
            F32Abs => float_wrapper!(visit_f32_abs),
            F32Neg => float_wrapper!(visit_f32_neg),
            F32Ceil => float_wrapper!(visit_f32_ceil),
            F32Floor => float_wrapper!(visit_f32_floor),
            F32Trunc => float_wrapper!(visit_f32_trunc),
            F32Nearest => float_wrapper!(visit_f32_nearest),
            F32Sqrt => float_wrapper!(visit_f32_sqrt),
            F32Add => float_wrapper!(visit_f32_add),
            F32Sub => float_wrapper!(visit_f32_sub),
            F32Mul => float_wrapper!(visit_f32_mul),
            F32Div => float_wrapper!(visit_f32_div),
            F32Min => float_wrapper!(visit_f32_min),
            F32Max => float_wrapper!(visit_f32_max),
            F32Copysign => float_wrapper!(visit_f32_copysign),
            F64Abs => float_wrapper!(visit_f64_abs),
            F64Neg => float_wrapper!(visit_f64_neg),
            F64Ceil => float_wrapper!(visit_f64_ceil),
            F64Floor => float_wrapper!(visit_f64_floor),
            F64Trunc => float_wrapper!(visit_f64_trunc),
            F64Nearest => float_wrapper!(visit_f64_nearest),
            F64Sqrt => float_wrapper!(visit_f64_sqrt),
            F64Add => float_wrapper!(visit_f64_add),
            F64Sub => float_wrapper!(visit_f64_sub),
            F64Mul => float_wrapper!(visit_f64_mul),
            F64Div => float_wrapper!(visit_f64_div),
            F64Min => float_wrapper!(visit_f64_min),
            F64Max => float_wrapper!(visit_f64_max),
            F64Copysign => float_wrapper!(visit_f64_copysign),
            I32TruncF32S => float_wrapper!(visit_i32_trunc_f32_s),
            I32TruncF32U => float_wrapper!(visit_i32_trunc_f32_u),
            I32TruncF64S => float_wrapper!(visit_i32_trunc_f64_s),
            I32TruncF64U => float_wrapper!(visit_i32_trunc_f64_u),
            I64TruncF32S => float_wrapper!(visit_i64_trunc_f32_s),
            I64TruncF32U => float_wrapper!(visit_i64_trunc_f32_u),
            I64TruncF64S => float_wrapper!(visit_i64_trunc_f64_s),
            I64TruncF64U => float_wrapper!(visit_i64_trunc_f64_u),
            F32ConvertI32S => float_wrapper!(visit_f32_convert_i32_s),
            F32ConvertI32U => float_wrapper!(visit_f32_convert_i32_u),
            F32ConvertI64S => float_wrapper!(visit_f32_convert_i64_s),
            F32ConvertI64U => float_wrapper!(visit_f32_convert_i64_u),
            F32DemoteF64 => float_wrapper!(visit_f32_demote_f64),
            F64ConvertI32S => float_wrapper!(visit_f64_convert_i32_s),
            F64ConvertI32U => float_wrapper!(visit_f64_convert_i32_u),
            F64ConvertI64S => float_wrapper!(visit_f64_convert_i64_s),
            F64ConvertI64U => float_wrapper!(visit_f64_convert_i64_u),
            F64PromoteF32 => float_wrapper!(visit_f64_promote_f32),
            I32TruncSatF32S => float_wrapper!(visit_i32_trunc_sat_f32_s),
            I32TruncSatF32U => float_wrapper!(visit_i32_trunc_sat_f32_u),
            I32TruncSatF64S => float_wrapper!(visit_i32_trunc_sat_f64_s),
            I32TruncSatF64U => float_wrapper!(visit_i32_trunc_sat_f64_u),
            I64TruncSatF32S => float_wrapper!(visit_i64_trunc_sat_f32_s),
            I64TruncSatF32U => float_wrapper!(visit_i64_trunc_sat_f32_u),
            I64TruncSatF64S => float_wrapper!(visit_i64_trunc_sat_f64_s),
            I64TruncSatF64U => float_wrapper!(visit_i64_trunc_sat_f64_u),
        }
    }
    vm.stop_exec = false;
    vm.next_result
        .take()
        .unwrap_or_else(|| unreachable!("rwasm: next result without reason?"))
}

#[inline(always)]
pub(crate) fn visit_unreachable<T>(_vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    Err(TrapCode::UnreachableCodeReached)
}

#[inline(always)]
pub(crate) fn visit_local_get<T>(vm: &mut RwasmExecutor<T>, local_depth: LocalDepth) {
    let value = vm.sp.nth_back(local_depth as usize);
    vm.sp.push(value);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_local_set<T>(vm: &mut RwasmExecutor<T>, local_depth: LocalDepth) {
    let new_value = vm.sp.pop();
    vm.sp.set_nth_back(local_depth as usize, new_value);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_local_tee<T>(vm: &mut RwasmExecutor<T>, local_depth: LocalDepth) {
    let new_value = vm.sp.last();
    vm.sp.set_nth_back(local_depth as usize, new_value);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_br<T>(vm: &mut RwasmExecutor<T>, branch_offset: BranchOffset) {
    vm.ip.offset(branch_offset.to_i32() as isize)
}

#[inline(always)]
pub(crate) fn visit_br_if<T>(vm: &mut RwasmExecutor<T>, branch_offset: BranchOffset) {
    let condition = vm.sp.pop_as();
    if condition {
        vm.ip.add(1);
    } else {
        vm.ip.offset(branch_offset.to_i32() as isize);
    }
}

#[inline(always)]
pub(crate) fn visit_br_if_nez<T>(vm: &mut RwasmExecutor<T>, branch_offset: BranchOffset) {
    let condition = vm.sp.pop_as();
    if condition {
        vm.ip.offset(branch_offset.to_i32() as isize);
    } else {
        vm.ip.add(1);
    }
}

#[inline(always)]
pub(crate) fn visit_br_table<T>(vm: &mut RwasmExecutor<T>, targets: BranchTableTargets) {
    let index: u32 = vm.sp.pop_as();
    let max_index = targets as usize - 1;
    let normalized_index = cmp::min(index as usize, max_index);
    vm.ip.add(2 * normalized_index + 1);
}

#[inline(always)]
pub(crate) fn visit_consume_fuel<T>(
    vm: &mut RwasmExecutor<T>,
    block_fuel: BlockFuel,
) -> Result<(), TrapCode> {
    if vm.config.fuel_enabled {
        vm.try_consume_fuel(block_fuel.to_u64())?;
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_return<T>(vm: &mut RwasmExecutor<T>) {
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
pub(crate) fn visit_return_call_internal<T>(
    vm: &mut RwasmExecutor<T>,
    compiled_func: CompiledFunc,
) -> Result<(), TrapCode> {
    vm.ip.add(1);
    vm.value_stack.sync_stack_ptr(vm.sp);
    vm.sp = vm.value_stack.stack_ptr();
    vm.ip = InstructionPtr::new(vm.module.code_section.instr.as_ptr());
    vm.ip.add(compiled_func as usize);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_return_call<T>(
    vm: &mut RwasmExecutor<T>,
    sys_func_idx: SysFuncIdx,
) -> Result<(), TrapCode> {
    vm.value_stack.sync_stack_ptr(vm.sp);
    // external call can cause interruption,
    // that is why it's important to increase IP before doing the call
    vm.ip.add(1);
    (vm.syscall_handler)(Caller::new(vm), sys_func_idx)?;
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_return_call_indirect<T>(
    vm: &mut RwasmExecutor<T>,
    signature_idx: SignatureIdx,
) -> Result<(), TrapCode> {
    let table = vm.fetch_table_index(1);
    let func_index: u32 = vm.sp.pop_as();
    vm.last_signature = Some(signature_idx);
    let instr_ref: u32 = vm
        .tables
        .get(&table)
        .expect("rwasm: unresolved table index")
        .get_untyped(func_index)
        .ok_or(TrapCode::TableOutOfBounds)?
        .try_into()
        .unwrap();
    if instr_ref == 0 {
        return Err(TrapCode::IndirectCallToNull.into());
    }
    vm.execute_call_internal(false, 2, instr_ref)
}

#[inline(always)]
pub(crate) fn visit_call_internal<T>(
    vm: &mut RwasmExecutor<T>,
    compiled_func: CompiledFunc,
) -> Result<(), TrapCode> {
    vm.ip.add(1);
    vm.value_stack.sync_stack_ptr(vm.sp);
    if vm.call_stack.len() > N_MAX_RECURSION_DEPTH {
        return Err(TrapCode::StackOverflow);
    }
    vm.call_stack.push(vm.ip);
    vm.sp = vm.value_stack.stack_ptr();
    vm.ip = InstructionPtr::new(vm.module.code_section.instr.as_ptr());
    vm.ip.add(compiled_func as usize);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_call<T>(
    vm: &mut RwasmExecutor<T>,
    sys_func_idx: SysFuncIdx,
) -> Result<(), TrapCode> {
    vm.value_stack.sync_stack_ptr(vm.sp);
    // external call can cause interruption,
    // that is why it's important to increase IP before doing the call
    vm.ip.add(1);
    (vm.syscall_handler)(Caller::new(vm), sys_func_idx)
}

#[inline(always)]
pub(crate) fn visit_call_indirect<T>(
    vm: &mut RwasmExecutor<T>,
    signature_idx: SignatureIdx,
) -> Result<(), TrapCode> {
    // resolve func index
    let table = vm.fetch_table_index(1);
    let func_index: u32 = vm.sp.pop_as();
    vm.last_signature = Some(signature_idx);
    let instr_ref = vm
        .tables
        .get(&table)
        .expect("rwasm: unresolved table index")
        .get_untyped(func_index)
        .map(|v| v.as_u32())
        .ok_or(TrapCode::TableOutOfBounds)?;
    if instr_ref == NULL_FUNC_IDX {
        return Err(TrapCode::IndirectCallToNull);
    }
    // call func
    vm.ip.add(2);
    vm.value_stack.sync_stack_ptr(vm.sp);
    if vm.call_stack.len() > N_MAX_RECURSION_DEPTH {
        return Err(TrapCode::StackOverflow);
    }
    vm.call_stack.push(vm.ip);
    vm.sp = vm.value_stack.stack_ptr();
    vm.ip = InstructionPtr::new(vm.module.code_section.instr.as_ptr());
    vm.ip.add(instr_ref as usize);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_signature_check<T>(
    vm: &mut RwasmExecutor<T>,
    signature_idx: SignatureIdx,
) -> Result<(), TrapCode> {
    if let Some(actual_signature) = vm.last_signature.take() {
        if actual_signature != signature_idx {
            return Err(TrapCode::BadSignature);
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
pub(crate) fn visit_global_get<T>(vm: &mut RwasmExecutor<T>, global_idx: GlobalIdx) {
    let global_value = vm
        .global_variables
        .get(&global_idx)
        .copied()
        .unwrap_or_default();
    vm.sp.push(global_value);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_global_set<T>(vm: &mut RwasmExecutor<T>, global_idx: GlobalIdx) {
    let new_value = vm.sp.pop();
    vm.global_variables.insert(global_idx, new_value);
    vm.ip.add(1);
}

macro_rules! impl_visit_load {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident<T>(vm: &mut RwasmExecutor<T>, address_offset: AddressOffset) -> Result<(), TrapCode> {
                vm.execute_load_extend(address_offset, UntypedValue::$untyped_ident)
            }
        )*
    }
}

impl_visit_load! {
    fn visit_i32_load(i32_load);

    fn visit_i32_load_i8_s(i32_load8_s);
    fn visit_i32_load_i8_u(i32_load8_u);
    fn visit_i32_load_i16_s(i32_load16_s);
    fn visit_i32_load_i16_u(i32_load16_u);
}

macro_rules! impl_visit_store {
    ( $( fn $visit_ident:ident($untyped_ident:ident, $type_size:literal); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident<T>(vm: &mut RwasmExecutor<T>, address_offset: AddressOffset) -> Result<(), TrapCode> {
                vm.execute_store_wrap(address_offset, UntypedValue::$untyped_ident, $type_size)
            }
        )*
    }
}

impl_visit_store! {
    fn visit_i32_store(i32_store, 4);
    fn visit_i32_store_8(i32_store8, 1);
    fn visit_i32_store_16(i32_store16, 2);
}

#[inline(always)]
pub(crate) fn visit_memory_size<T>(vm: &mut RwasmExecutor<T>) {
    let result: u32 = vm.global_memory.current_pages().into();
    vm.sp.push_as(result);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_memory_grow<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
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
pub(crate) fn visit_memory_fill<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
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
        .ok_or(TrapCode::MemoryOutOfBounds)?;
    memory.fill(byte);
    if let Some(tracer) = vm.tracer.as_mut() {
        tracer.memory_change(offset as u32, n as u32, memory);
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_memory_copy<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
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
        .ok_or(TrapCode::MemoryOutOfBounds)?;
    data.get(dst_offset..)
        .and_then(|memory| memory.get(..n))
        .ok_or(TrapCode::MemoryOutOfBounds)?;
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
pub(crate) fn visit_memory_init<T>(
    vm: &mut RwasmExecutor<T>,
    data_segment_idx: DataSegmentIdx,
) -> Result<(), TrapCode> {
    let is_empty_data_segment = vm
        .empty_data_segments
        .get(data_segment_idx as usize)
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
        .ok_or(TrapCode::MemoryOutOfBounds)?;
    let mut memory_section = vm.module.data_section.as_slice();
    if is_empty_data_segment {
        memory_section = &[];
    }
    let data = memory_section
        .get(src_offset..)
        .and_then(|data| data.get(..n))
        .ok_or(TrapCode::MemoryOutOfBounds)?;
    memory.copy_from_slice(data);
    if let Some(tracer) = vm.tracer.as_mut() {
        tracer.global_memory(dst_offset as u32, n as u32, memory);
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_data_drop<T>(vm: &mut RwasmExecutor<T>, data_segment_idx: DataSegmentIdx) {
    vm.empty_data_segments.set(data_segment_idx as usize, true);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_table_size<T>(vm: &mut RwasmExecutor<T>, table_idx: TableIdx) {
    let table_size = vm
        .tables
        .get(&table_idx)
        .expect("rwasm: unresolved table segment")
        .size();
    vm.sp.push_as(table_size);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_table_grow<T>(
    vm: &mut RwasmExecutor<T>,
    table_idx: TableIdx,
) -> Result<(), TrapCode> {
    let (init, delta) = vm.sp.pop2();
    let delta: u32 = delta.into();
    if vm.config.fuel_enabled {
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_elements(delta as u64))?;
    }
    let table = vm.tables.entry(table_idx).or_insert_with(TableEntity::new);
    let result = table.grow_untyped(delta, init);
    vm.sp.push_as(result);
    if let Some(tracer) = vm.tracer.as_mut() {
        tracer.table_size_change(table_idx, init.into(), delta);
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_table_fill<T>(
    vm: &mut RwasmExecutor<T>,
    table_idx: TableIdx,
) -> Result<(), TrapCode> {
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
pub(crate) fn visit_table_get<T>(
    vm: &mut RwasmExecutor<T>,
    table_idx: TableIdx,
) -> Result<(), TrapCode> {
    let index = vm.sp.pop();
    let value = vm
        .tables
        .get_mut(&table_idx)
        .expect("rwasm: missing table")
        .get_untyped(index.into())
        .ok_or(TrapCode::TableOutOfBounds)?;
    vm.sp.push(value);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_table_set<T>(
    vm: &mut RwasmExecutor<T>,
    table_idx: TableIdx,
) -> Result<(), TrapCode> {
    let (index, value) = vm.sp.pop2();
    vm.tables
        .get_mut(&table_idx)
        .expect("rwasm: missing table")
        .set_untyped(index.into(), value)
        .map_err(|_| TrapCode::TableOutOfBounds)?;
    if let Some(tracer) = vm.tracer.as_mut() {
        tracer.table_change(table_idx, index.into(), value);
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_table_copy<T>(
    vm: &mut RwasmExecutor<T>,
    dst_table_idx: TableIdx,
) -> Result<(), TrapCode> {
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
pub(crate) fn visit_table_init<T>(
    vm: &mut RwasmExecutor<T>,
    element_segment_idx: ElementSegmentIdx,
) -> Result<(), TrapCode> {
    let table_idx = vm.fetch_table_index(1);

    let (d, s, n) = vm.sp.pop3();
    let len = u32::from(n);
    let src_index = u32::from(s);
    let dst_index = u32::from(d);

    println!(
        "src_index: {}, dst_index: {}, len: {}",
        src_index, dst_index, len
    );

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
        .get(element_segment_idx as usize)
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
pub(crate) fn visit_element_drop<T>(
    vm: &mut RwasmExecutor<T>,
    element_segment_idx: ElementSegmentIdx,
) {
    vm.empty_elements_segments
        .set(element_segment_idx as usize, true);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_ref_func<T>(vm: &mut RwasmExecutor<T>, compiled_func: CompiledFunc) {
    vm.sp.push_as(compiled_func);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_i32_i64_const<T>(vm: &mut RwasmExecutor<T>, untyped_value: UntypedValue) {
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

    fn visit_i32_clz(i32_clz);
    fn visit_i32_ctz(i32_ctz);
    fn visit_i32_popcnt(i32_popcnt);

    fn visit_i32_wrap_i64(i32_wrap_i64);

    fn visit_i32_extend8_s(i32_extend8_s);
    fn visit_i32_extend16_s(i32_extend16_s);
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
}

macro_rules! impl_visit_fallible_binary {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
                vm.try_execute_binary(UntypedValue::$untyped_ident)
            }
        )*
    }
}

impl_visit_fallible_binary! {
    fn visit_i32_div_s(i32_div_s);
    fn visit_i32_div_u(i32_div_u);
    fn visit_i32_rem_s(i32_rem_s);
    fn visit_i32_rem_u(i32_rem_u);
}

#[inline(always)]
pub(crate) fn visit_stack_alloc<T>(
    vm: &mut RwasmExecutor<T>,
    max_stack_height: MaxStackHeight,
) -> Result<(), TrapCode> {
    vm.value_stack.reserve(max_stack_height as usize)?;
    vm.ip.add(1);
    Ok(())
}
