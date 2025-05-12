use crate::{
    Caller,
    Instruction,
    InstructionPtr,
    Opcode,
    RwasmExecutor,
    TrapCode,
    FUNC_REF_NULL,
    FUNC_REF_OFFSET,
    N_MAX_RECURSION_DEPTH,
};
use core::cmp;

#[inline(always)]
pub(crate) fn exec_control_flow_opcode<T>(
    vm: &mut RwasmExecutor<T>,
    instr: Instruction,
) -> Result<(), TrapCode> {
    use Opcode::*;
    match instr.opcode() {
        Unreachable => {
            return Err(TrapCode::UnreachableCodeReached);
        }
        ConsumeFuel => {
            let block_fuel = match instr {
                Instruction::BlockFuel(_, block_fuel) => block_fuel,
                _ => unreachable!(),
            };
            if vm.config.fuel_enabled {
                vm.try_consume_fuel(block_fuel.to_u64())?;
            }
            vm.ip.add(1);
        }
        SignatureCheck => {
            let signature_idx = match instr {
                Instruction::SignatureIdx(_, value) => value,
                _ => unreachable!(),
            };
            if let Some(actual_signature) = vm.last_signature.take() {
                if actual_signature != signature_idx {
                    return Err(TrapCode::BadSignature);
                }
            }
            vm.ip.add(1);
        }
        StackAlloc => {
            let max_stack_height = match instr {
                Instruction::StackAlloc(_, value) => value.max_stack_height,
                _ => unreachable!(),
            };
            vm.value_stack.reserve(max_stack_height as usize)?;
            vm.ip.add(1);
        }
        Br => {
            let branch_offset = match instr {
                Instruction::BranchOffset(_, branch_offset) => branch_offset,
                _ => unreachable!(),
            };
            vm.ip.offset(branch_offset.to_i32() as isize)
        }
        BrIfEqz => {
            let branch_offset = match instr {
                Instruction::BranchOffset(_, branch_offset) => branch_offset,
                _ => unreachable!("rwasm: missing instr data"),
            };
            let condition = vm.sp.pop_as();
            if condition {
                vm.ip.add(1);
            } else {
                vm.ip.offset(branch_offset.to_i32() as isize);
            }
        }
        BrIfNez => {
            let branch_offset = match instr {
                Instruction::BranchOffset(_, branch_offset) => branch_offset,
                _ => unreachable!("rwasm: missing instr data"),
            };
            let condition = vm.sp.pop_as();
            if condition {
                vm.ip.offset(branch_offset.to_i32() as isize);
            } else {
                vm.ip.add(1);
            }
        }
        BrAdjust => {
            let branch_offset = match instr {
                Instruction::BranchOffset(_, branch_offset) => branch_offset,
                _ => unreachable!(),
            };
            let drop_keep = vm.fetch_drop_keep(1);
            vm.sp.drop_keep(drop_keep);
            vm.ip.offset(branch_offset.to_i32() as isize);
        }
        BrAdjustIfNez => {
            let branch_offset = match instr {
                Instruction::BranchOffset(_, branch_offset) => branch_offset,
                _ => unreachable!(),
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
        BrTable => {
            let targets = match instr {
                Instruction::BranchTableTargets(_, targets) => targets,
                _ => unreachable!(),
            };
            let index: u32 = vm.sp.pop_as();
            let max_index = targets.to_usize() - 1;
            let normalized_index = cmp::min(index as usize, max_index);
            vm.ip.add(2 * normalized_index + 1);
        }
        Return => {
            let drop_keep = match instr {
                Instruction::DropKeep(_, drop_keep) => drop_keep,
                _ => unreachable!(),
            };
            vm.sp.drop_keep(drop_keep);
            vm.value_stack.sync_stack_ptr(vm.sp);
            match vm.call_stack.pop() {
                Some(caller) => {
                    vm.ip = caller;
                }
                None => return Err(TrapCode::ExecutionHalted),
            }
        }
        ReturnIfNez => {
            let drop_keep = match instr {
                Instruction::DropKeep(_, drop_keep) => drop_keep,
                _ => unreachable!(),
            };
            let condition = vm.sp.pop_as();
            if condition {
                vm.sp.drop_keep(drop_keep);
                vm.value_stack.sync_stack_ptr(vm.sp);
                match vm.call_stack.pop() {
                    Some(caller) => {
                        vm.ip = caller;
                    }
                    None => {
                        return Err(TrapCode::ExecutionHalted);
                    }
                }
            } else {
                vm.ip.add(1);
            }
        }
        ReturnCallInternal => {
            let func_idx = match instr {
                Instruction::CompiledFunc(_, func_idx) => func_idx,
                _ => unreachable!(),
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
        }
        ReturnCall => {
            let func_idx = match instr {
                Instruction::CompiledFunc(_, func_idx) => func_idx,
                _ => unreachable!(),
            };
            let drop_keep = vm.fetch_drop_keep(1);
            vm.sp.drop_keep(drop_keep);
            vm.value_stack.sync_stack_ptr(vm.sp);
            // external call can cause interruption,
            // that is why it's important to increase IP before doing the call
            vm.ip.add(2);
            (vm.syscall_handler)(Caller::new(vm), func_idx)?;
        }
        ReturnCallIndirect => {
            let signature_idx = match instr {
                Instruction::SignatureIdx(_, value) => value,
                _ => unreachable!(),
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
                .ok_or(TrapCode::TableOutOfBounds)?
                .try_into()
                .unwrap();
            if func_idx == 0 {
                return Err(TrapCode::IndirectCallToNull.into());
            }
            let func_idx = func_idx - FUNC_REF_OFFSET;
            vm.execute_call_internal(false, 3, func_idx)?;
        }
        CallInternal => {
            let func_idx = match instr {
                Instruction::CompiledFunc(_, value) => value,
                _ => unreachable!(),
            };
            vm.ip.add(1);
            vm.value_stack.sync_stack_ptr(vm.sp);
            if vm.call_stack.len() > N_MAX_RECURSION_DEPTH {
                return Err(TrapCode::StackOverflow);
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
        }
        Call => {
            let func_idx = match instr {
                Instruction::FuncIdx(_, value) => value,
                _ => unreachable!(),
            };
            vm.value_stack.sync_stack_ptr(vm.sp);
            // external call can cause interruption,
            // that is why it's important to increase IP before doing the call
            vm.ip.add(1);
            (vm.syscall_handler)(Caller::new(vm), func_idx.to_u32())?;
        }
        CallIndirect => {
            let signature_idx = match instr {
                Instruction::SignatureIdx(_, value) => value,
                _ => unreachable!(),
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
                .ok_or(TrapCode::TableOutOfBounds)?;
            if func_idx == FUNC_REF_NULL {
                return Err(TrapCode::IndirectCallToNull);
            }
            let func_idx = func_idx - FUNC_REF_OFFSET;
            // call func
            vm.ip.add(2);
            vm.value_stack.sync_stack_ptr(vm.sp);
            if vm.call_stack.len() > N_MAX_RECURSION_DEPTH {
                return Err(TrapCode::StackOverflow);
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
        }
        _ => unreachable!(),
    }
    Ok(())
}
