use crate::{Instruction, Opcode, RwasmExecutor, TrapCode, UntypedValue, FUNC_REF_OFFSET};

#[inline(always)]
pub(crate) fn exec_stack_opcode<T>(
    vm: &mut RwasmExecutor<T>,
    instr: Instruction,
) -> Result<(), TrapCode> {
    use Opcode::*;
    match instr.opcode() {
        LocalGet => {
            let local_depth = match instr {
                Instruction::LocalDepth(_, local_depth) => local_depth,
                _ => unreachable!(),
            };
            let value = vm.sp.nth_back(local_depth.to_usize());
            vm.sp.push(value);
            vm.ip.add(1);
        }
        LocalSet => {
            let local_depth = match instr {
                Instruction::LocalDepth(_, local_depth) => local_depth,
                _ => unreachable!(),
            };
            let new_value = vm.sp.pop();
            vm.sp.set_nth_back(local_depth.to_usize(), new_value);
            vm.ip.add(1);
        }
        LocalTee => {
            let local_depth = match instr {
                Instruction::LocalDepth(_, local_depth) => local_depth,
                _ => unreachable!(),
            };
            let new_value = vm.sp.last();
            vm.sp.set_nth_back(local_depth.to_usize(), new_value);
            vm.ip.add(1);
        }
        Drop => {
            vm.sp.drop();
            vm.ip.add(1);
        }
        Select => {
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
        RefFunc => {
            let func_idx = match instr {
                Instruction::FuncIdx(_, value) => value,
                _ => unreachable!(),
            };
            vm.sp.push_as(func_idx.to_u32() + FUNC_REF_OFFSET);
            vm.ip.add(1);
        }
        I32Const => {
            let untyped_value = match instr {
                Instruction::UntypedValue(_, value) => value,
                _ => unreachable!(),
            };
            vm.sp.push(untyped_value);
            vm.ip.add(1);
        }
        GlobalGet => {
            let global_idx = match instr {
                Instruction::GlobalIdx(_, value) => value,
                _ => unreachable!(),
            };
            let global_value = vm
                .global_variables
                .get(&global_idx)
                .copied()
                .unwrap_or_default();
            vm.sp.push(global_value);
            vm.ip.add(1);
        }
        GlobalSet => {
            let global_idx = match instr {
                Instruction::GlobalIdx(_, value) => value,
                _ => unreachable!(),
            };
            let new_value = vm.sp.pop();
            vm.global_variables.insert(global_idx, new_value);
            vm.ip.add(1);
        }
        _ => unreachable!(),
    }
    Ok(())
}
