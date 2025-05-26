use crate::{
    BlockFuel,
    BranchOffset,
    BranchTableTargets,
    Caller,
    CompiledFunc,
    GlobalIdx,
    InstructionPtr,
    LocalDepth,
    MaxStackHeight,
    RwasmExecutor,
    SignatureIdx,
    SysFuncIdx,
    TrapCode,
    UntypedValue,
    NULL_FUNC_IDX,
    N_MAX_RECURSION_DEPTH,
};
use core::cmp;

#[inline(always)]
pub(crate) fn visit_unreachable<T>(_vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    Err(TrapCode::UnreachableCodeReached)
}

#[inline(always)]
pub(crate) fn visit_trap_code<T>(
    _vm: &mut RwasmExecutor<T>,
    trap_code: TrapCode,
) -> Result<(), TrapCode> {
    Err(trap_code)
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
pub(crate) fn visit_return<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    vm.value_stack.sync_stack_ptr(vm.sp);
    match vm.call_stack.pop() {
        Some(caller) => {
            vm.ip = caller;
            Ok(())
        }
        None => Err(TrapCode::ExecutionHalted),
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

#[inline(always)]
pub(crate) fn visit_stack_alloc<T>(
    vm: &mut RwasmExecutor<T>,
    max_stack_height: MaxStackHeight,
) -> Result<(), TrapCode> {
    vm.value_stack.reserve(max_stack_height as usize)?;
    vm.ip.add(1);
    Ok(())
}
