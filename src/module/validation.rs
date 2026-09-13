use crate::{Opcode, RwasmModuleInner};

/// Establish the bounds needed by the thin instruction pointer once, at module construction.
/// Every executable word must have in-range successors and any required inline table payload.
/// Dynamic table contents are checked at indirect calls, since a host or bytecode can change them.
pub(super) fn executable_len(module: &RwasmModuleInner) -> usize {
    let code = &module.code_section;
    // A tail indirect call can end the section with a TableGet payload. It is read as metadata,
    // never fetched as an instruction: executing it would fall through past the allocation.
    let len = match code.as_slice() {
        [.., Opcode::ReturnCallIndirect(_), Opcode::TableGet(_)] => code.len() - 1,
        _ => code.len(),
    };
    if module.source_pc as usize >= len {
        return 0;
    }
    for (pc, opcode) in code.iter().take(len).enumerate() {
        let relative_target = |offset: i32| {
            pc.checked_add_signed(offset as isize)
                .is_some_and(|target| target < len)
        };
        let table_payload = || matches!(code.get(pc + 1), Some(Opcode::TableGet(_)));
        let valid = match opcode {
            Opcode::Return | Opcode::Unreachable | Opcode::Trap(_) => true,
            Opcode::Br(offset) => relative_target(offset.to_i32()),
            Opcode::BrIfEqz(offset) | Opcode::BrIfNez(offset) => {
                relative_target(offset.to_i32()) && pc + 1 < len
            }
            Opcode::BrTable(targets) => {
                // Each target occupies two opcode words, including the default target.
                *targets > 0
                    && (*targets as usize)
                        .checked_mul(2)
                        .and_then(|words| pc.checked_add(words))
                        .is_some_and(|last| last < code.len() && last - 1 < len)
            }
            Opcode::CallInternal(target) => (*target as usize) < len && pc + 1 < len,
            Opcode::ReturnCallInternal(target) => (*target as usize) < len,
            Opcode::RefFunc(target) => (*target as usize) < len && pc + 1 < len,
            Opcode::CallIndirect(_) | Opcode::TableInit(_) => table_payload() && pc + 2 < len,
            Opcode::ReturnCallIndirect(_) => table_payload(),
            _ => pc + 1 < len,
        };
        if !valid {
            return 0;
        }
    }
    len
}
