use super::{RwasmModule, RwasmModuleInner};
use crate::{
    InstructionSet, Opcode, N_MAX_DATA_SEGMENTS, N_MAX_ELEM_SEGMENTS, N_MAX_STACK_SIZE,
    N_MAX_TABLES,
};
use alloc::{vec, vec::Vec};
use bincode::error::DecodeError;

/// Returns how many cells an instruction may address away from the current stack pointer.
///
/// # Note
///
/// A well-formed module never reaches further than the largest stack window it reserves: pushes
/// are covered by the `StackCheck` of their own function, and reads below the stack pointer target
/// parameters that some caller had to push within *its* reserved window. [`N_MAX_STACK_SIZE`] is
/// the floor, because that many cells are addressable without reserving anything at all.
fn max_stack_reach(code: &InstructionSet) -> i64 {
    code.iter()
        .filter_map(|opcode| match opcode {
            Opcode::StackCheck(reserved) => Some(i64::from(*reserved)),
            _ => None,
        })
        .max()
        .unwrap_or(0)
        .max(N_MAX_STACK_SIZE as i64)
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RwasmModuleVerificationError {
    EmptyCodeSection,
    SourcePcOutOfBounds {
        source_pc: u32,
        code_len: usize,
    },
    BranchTargetOutOfBounds {
        pc: usize,
        offset: i32,
        code_len: usize,
    },
    ZeroBranchOffset {
        pc: usize,
    },
    BranchTableTargetsOutOfBounds {
        pc: usize,
        targets: u32,
        code_len: usize,
    },
    CallTargetOutOfBounds {
        pc: usize,
        target: u32,
        code_len: usize,
    },
    ElementTargetOutOfBounds {
        index: usize,
        target: u32,
        code_len: usize,
    },
    LocalDepthOutOfBounds {
        pc: usize,
        depth: u32,
    },
    DataSegmentOutOfBounds {
        pc: usize,
        segment: u32,
    },
    ElementSegmentOutOfBounds {
        pc: usize,
        segment: u32,
    },
    TableIndexOutOfBounds {
        pc: usize,
        table: u16,
    },
    MissingTableIndexPayload {
        pc: usize,
    },
    InvalidTableIndexPayload {
        pc: usize,
    },
    /// An opcode pops more values than the value stack can ever hold at this point.
    StackUnderflow {
        pc: usize,
        height: i64,
        pops: u32,
    },
    /// An opcode pushes past the maximum height the value stack can ever reach.
    StackOverflow {
        pc: usize,
        height: i64,
        pushes: u32,
    },
    /// Execution can run past the end of the code section.
    FallsThroughCodeSection {
        pc: usize,
    },
}

#[derive(Debug)]
pub enum RwasmModuleError {
    Decode(DecodeError),
    Verification(RwasmModuleVerificationError),
}

impl From<DecodeError> for RwasmModuleError {
    fn from(error: DecodeError) -> Self {
        Self::Decode(error)
    }
}

impl From<RwasmModuleVerificationError> for RwasmModuleError {
    fn from(error: RwasmModuleVerificationError) -> Self {
        Self::Verification(error)
    }
}

impl RwasmModuleInner {
    pub fn verify(&self) -> Result<(), RwasmModuleVerificationError> {
        verify_module(self)
    }
}

impl RwasmModule {
    pub fn verify(&self) -> Result<(), RwasmModuleVerificationError> {
        self.inner.verify()
    }
}

fn verify_module(module: &RwasmModuleInner) -> Result<(), RwasmModuleVerificationError> {
    let code = &module.code_section;
    let code_len = code.len();
    if code_len == 0 {
        return Err(RwasmModuleVerificationError::EmptyCodeSection);
    }
    if module.source_pc as usize >= code_len {
        return Err(RwasmModuleVerificationError::SourcePcOutOfBounds {
            source_pc: module.source_pc,
            code_len,
        });
    }
    for (index, target) in module.elem_section.iter().copied().enumerate() {
        if target != 0 && target as usize >= code_len {
            return Err(RwasmModuleVerificationError::ElementTargetOutOfBounds {
                index,
                target,
                code_len,
            });
        }
    }
    for (pc, opcode) in code.iter().copied().enumerate() {
        verify_opcode(code, pc, opcode)?;
    }
    verify_stack_usage(module)?;
    Ok(())
}

/// The emulated height of the value stack at a given program counter.
///
/// Heights are relative to the entry of the enclosing function, which is why they can legitimately
/// turn negative: a callee reads its parameters from below its own entry stack pointer and drops
/// them again before returning.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Height {
    /// The program counter has not been reached yet.
    Unvisited,
    Known(i64),
    /// The height depends on the signature of a host function, which is not part of the module.
    Unknown,
}

impl Height {
    /// Merges two heights reaching the same program counter.
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unvisited, other) => other,
            (current, Self::Unvisited) => current,
            (Self::Known(lhs), Self::Known(rhs)) if lhs == rhs => Self::Known(lhs),
            _ => Self::Unknown,
        }
    }
}

/// Verifies that no reachable opcode addresses the value stack outside of its bounds.
///
/// # Note
///
/// This is an abstract interpretation of the whole code section: every function entry seeds an
/// emulated stack height of zero, which is then propagated along all control flow edges. Opcodes
/// are rejected when they provably reach further than [`max_stack_reach`] cells away from the
/// stack pointer.
///
/// The pass is deliberately conservative. rWasm does not record function signatures, so the
/// emulated height becomes [`Height::Unknown`] after a call, and from there on only the checks
/// that hold for every possible height are applied. Whatever slips through is still caught at
/// runtime by the bounds checks in [`ValueStackPtr`].
///
/// [`ValueStackPtr`]: crate::ValueStackPtr
fn verify_stack_usage(module: &RwasmModuleInner) -> Result<(), RwasmModuleVerificationError> {
    let code = &module.code_section;
    let reach = max_stack_reach(code);
    let mut heights = vec![Height::Unvisited; code.len()];
    let mut worklist: Vec<(usize, Height)> = function_entries(module)
        .into_iter()
        .map(|entry| (entry, Height::Known(0)))
        .collect();

    while let Some((pc, incoming)) = worklist.pop() {
        let current = heights[pc];
        let merged = current.merge(incoming);
        if merged == current {
            continue;
        }
        heights[pc] = merged;
        let opcode = code[pc];
        let height = verify_stack_effect(pc, opcode, merged, reach)?;
        for successor in successors(pc, opcode) {
            if successor >= code.len() {
                return Err(RwasmModuleVerificationError::FallsThroughCodeSection { pc });
            }
            worklist.push((successor, height));
        }
    }
    Ok(())
}

/// Collects every program counter execution can enter a function at.
fn function_entries(module: &RwasmModuleInner) -> Vec<usize> {
    // The constructor runs from the very beginning of the code section, `source_pc` skips it.
    let mut entries = vec![0usize, module.source_pc as usize];
    for opcode in module.code_section.iter().copied() {
        let target = match opcode {
            Opcode::CallInternal(target) | Opcode::ReturnCallInternal(target) => target,
            // A null function reference is never called.
            Opcode::RefFunc(target) if target != 0 => target,
            _ => continue,
        };
        entries.push(target as usize);
    }
    entries.extend(
        module
            .elem_section
            .iter()
            .copied()
            .filter(|target| *target != 0)
            .map(|target| target as usize),
    );
    // Targets are range checked by `verify_opcode` and `verify_module` before we get here.
    entries.retain(|entry| *entry < module.code_section.len());
    entries.sort_unstable();
    entries.dedup();
    entries
}

/// Applies the stack effect of `opcode` to `height` and rejects out-of-bounds accesses.
fn verify_stack_effect(
    pc: usize,
    opcode: Opcode,
    height: Height,
    reach: i64,
) -> Result<Height, RwasmModuleVerificationError> {
    let (pops, pushes) = stack_effect(opcode);
    let Height::Known(height) = height else {
        // Without a height, only the bounds that hold for every possible height apply.
        verify_local_depth(pc, opcode, 0, reach)?;
        return Ok(Height::Unknown);
    };

    // `LocalSet` writes below the value it just popped, every other local opcode addresses the
    // stack as it found it.
    let depth_height = match opcode {
        Opcode::LocalSet(_) => height - 1,
        _ => height,
    };
    verify_local_depth(pc, opcode, depth_height, reach)?;

    if height - i64::from(pops) < -reach {
        return Err(RwasmModuleVerificationError::StackUnderflow { pc, height, pops });
    }
    let height = height - i64::from(pops);
    if height + i64::from(pushes) > reach {
        return Err(RwasmModuleVerificationError::StackOverflow { pc, height, pushes });
    }
    if is_call(opcode) {
        // A call swaps the callee's parameters for its results, and rWasm records neither.
        return Ok(Height::Unknown);
    }
    Ok(Height::Known(height + i64::from(pushes)))
}

/// Returns `true` if `opcode` transfers control to a function whose signature is unknown here.
fn is_call(opcode: Opcode) -> bool {
    matches!(
        opcode,
        Opcode::CallInternal(_) | Opcode::CallIndirect(_) | Opcode::Call(_) | Opcode::ReturnCall(_)
    )
}

/// Rejects local depths that address a cell outside the value stack.
///
/// A depth of zero addresses the free cell above the stack pointer, which is never a valid local.
fn verify_local_depth(
    pc: usize,
    opcode: Opcode,
    height: i64,
    reach: i64,
) -> Result<(), RwasmModuleVerificationError> {
    let (Opcode::LocalGet(depth) | Opcode::LocalSet(depth) | Opcode::LocalTee(depth)) = opcode
    else {
        return Ok(());
    };
    if depth == 0 || i64::from(depth) > height + reach {
        return Err(RwasmModuleVerificationError::LocalDepthOutOfBounds { pc, depth });
    }
    Ok(())
}

/// Returns the program counters execution can continue at after `opcode`.
fn successors(pc: usize, opcode: Opcode) -> Vec<usize> {
    match opcode {
        // Execution either leaves the function or resumes at a program counter that is seeded as
        // a function entry in its own right.
        Opcode::Unreachable
        | Opcode::Trap(_)
        | Opcode::Return
        | Opcode::ReturnCallInternal(_)
        | Opcode::ReturnCallIndirect(_) => vec![],
        Opcode::Br(offset) => vec![branch_target(pc, offset.to_i32())],
        Opcode::BrIfEqz(offset) | Opcode::BrIfNez(offset) => {
            vec![pc + 1, branch_target(pc, offset.to_i32())]
        }
        // A branch table is followed by `targets` pairs of opcodes, and the interpreter jumps to
        // the first opcode of the selected pair.
        Opcode::BrTable(targets) => (0..targets as usize).map(|i| pc + 2 * i + 1).collect(),
        // `CallIndirect` and `TableInit` carry the table index in the following opcode.
        Opcode::CallIndirect(_) | Opcode::TableInit(_) => vec![pc + 2],
        _ => vec![pc + 1],
    }
}

/// Resolves a branch target that [`verify_branch_target`] already proved to be in bounds.
fn branch_target(pc: usize, offset: i32) -> usize {
    (pc as i64 + offset as i64) as usize
}

/// Returns how many values `opcode` pops from and pushes onto the value stack.
///
/// # Note
///
/// Values are counted in stack cells, so the 64 bit opcodes account for the two cells an `i64`
/// or an `f64` occupies.
fn stack_effect(opcode: Opcode) -> (u32, u32) {
    use Opcode::*;
    match opcode {
        // stack/system
        Unreachable
        | Trap(_)
        | Br(_)
        | ConsumeFuel(_)
        | SignatureCheck(_)
        | StackCheck(_)
        | DataDrop(_)
        | ElemDrop(_)
        | Return
        | ReturnCallInternal(_) => (0, 0),
        LocalGet(_) | RefFunc(_) | I32Const(_) | GlobalGet(_) | MemorySize | TableSize(_) => (0, 1),
        LocalSet(_) | Drop | GlobalSet(_) | ConsumeFuelStack | BrIfEqz(_) | BrIfNez(_)
        | BrTable(_) => (1, 0),
        LocalTee(_) => (1, 1),
        Select => (3, 1),
        BulkConst(locals) => (0, locals),
        BulkDrop(locals) => (locals, 0),
        // A call pops the callee's parameters and pushes its results, but rWasm records neither;
        // `verify_stack_effect` turns the height into `Height::Unknown` instead.
        Call(_) | ReturnCall(_) | CallInternal(_) => (0, 0),
        // The indirect opcodes additionally pop the function index off the stack.
        CallIndirect(_) | ReturnCallIndirect(_) => (1, 0),

        // memory
        I32Load(_) | I32Load8S(_) | I32Load8U(_) | I32Load16S(_) | I32Load16U(_) | MemoryGrow => {
            (1, 1)
        }
        I32Store(_) | I32Store8(_) | I32Store16(_) => (2, 0),
        MemoryFill | MemoryCopy | MemoryInit(_) => (3, 0),

        // table
        TableGet(_) => (1, 1),
        TableSet(_) => (2, 0),
        TableGrow(_) => (2, 1),
        TableFill(_) | TableCopy(_, _) | TableInit(_) => (3, 0),

        // alu
        I32Eqz | I32Clz | I32Ctz | I32Popcnt | I32WrapI64 | I32Extend8S | I32Extend16S => (1, 1),
        I32Mul64 | I32Add64 => (2, 2),
        _ if opcode.is_binary_instruction() => (2, 1),

        // fpu
        F32Load(_) | F32Abs | F32Neg | F32Ceil | F32Floor | F32Trunc | F32Nearest | F32Sqrt
        | I32TruncF32S | I32TruncF32U | I32TruncSatF32S | I32TruncSatF32U | F32ConvertI32S
        | F32ConvertI32U => (1, 1),
        F64Load(_) | I64TruncF32S | I64TruncF32U | I64TruncSatF32S | I64TruncSatF32U
        | F64ConvertI32S | F64ConvertI32U | F64PromoteF32 => (1, 2),
        F32Store(_) => (2, 0),
        I32TruncF64S | I32TruncF64U | I32TruncSatF64S | I32TruncSatF64U | F32ConvertI64S
        | F32ConvertI64U | F32DemoteF64 => (2, 1),
        F64Abs | F64Neg | F64Ceil | F64Floor | F64Trunc | F64Nearest | F64Sqrt | I64TruncF64S
        | I64TruncF64U | I64TruncSatF64S | I64TruncSatF64U | F64ConvertI64S | F64ConvertI64U => {
            (2, 2)
        }
        F64Store(_) => (3, 0),
        F32Eq | F32Ne | F32Lt | F32Gt | F32Le | F32Ge | F32Add | F32Sub | F32Mul | F32Div
        | F32Min | F32Max | F32Copysign => (2, 1),
        F64Eq | F64Ne | F64Lt | F64Gt | F64Le | F64Ge => (4, 1),
        F64Add | F64Sub | F64Mul | F64Div | F64Min | F64Max | F64Copysign => (4, 2),

        // Unary integer opcodes not covered above keep the height as it is.
        _ => (1, 1),
    }
}

fn verify_opcode(
    code: &InstructionSet,
    pc: usize,
    opcode: Opcode,
) -> Result<(), RwasmModuleVerificationError> {
    match opcode {
        Opcode::Br(offset) | Opcode::BrIfEqz(offset) | Opcode::BrIfNez(offset) => {
            verify_branch_target(code.len(), pc, offset.to_i32())
        }
        Opcode::BrTable(targets) => verify_branch_table(code.len(), pc, targets),
        Opcode::CallInternal(target) | Opcode::ReturnCallInternal(target) => {
            verify_code_target(code.len(), pc, target)
        }
        Opcode::RefFunc(target) => {
            if target == 0 {
                Ok(())
            } else {
                verify_code_target(code.len(), pc, target)
            }
        }
        Opcode::CallIndirect(_) | Opcode::ReturnCallIndirect(_) => {
            verify_table_index_payload(code, pc)
        }
        Opcode::LocalGet(depth) | Opcode::LocalSet(depth) | Opcode::LocalTee(depth) => {
            if depth == 0 {
                return Err(RwasmModuleVerificationError::LocalDepthOutOfBounds { pc, depth });
            }
            Ok(())
        }
        Opcode::MemoryInit(segment) | Opcode::DataDrop(segment) => {
            if segment as usize >= N_MAX_DATA_SEGMENTS {
                return Err(RwasmModuleVerificationError::DataSegmentOutOfBounds { pc, segment });
            }
            Ok(())
        }
        Opcode::TableInit(segment) => {
            if segment as usize >= N_MAX_ELEM_SEGMENTS {
                return Err(RwasmModuleVerificationError::ElementSegmentOutOfBounds {
                    pc,
                    segment,
                });
            }
            verify_table_index_payload(code, pc)
        }
        Opcode::ElemDrop(segment) => {
            if segment as usize >= N_MAX_ELEM_SEGMENTS {
                return Err(RwasmModuleVerificationError::ElementSegmentOutOfBounds {
                    pc,
                    segment,
                });
            }
            Ok(())
        }
        Opcode::TableSize(table)
        | Opcode::TableGrow(table)
        | Opcode::TableFill(table)
        | Opcode::TableGet(table)
        | Opcode::TableSet(table) => verify_table_index(pc, table),
        Opcode::TableCopy(dst, src) => {
            verify_table_index(pc, dst)?;
            verify_table_index(pc, src)
        }
        _ => Ok(()),
    }
}

fn verify_branch_target(
    code_len: usize,
    pc: usize,
    offset: i32,
) -> Result<(), RwasmModuleVerificationError> {
    if offset == 0 {
        return Err(RwasmModuleVerificationError::ZeroBranchOffset { pc });
    }
    let target = (pc as i64).checked_add(offset as i64).ok_or(
        RwasmModuleVerificationError::BranchTargetOutOfBounds {
            pc,
            offset,
            code_len,
        },
    )?;
    if target < 0 || target as usize >= code_len {
        return Err(RwasmModuleVerificationError::BranchTargetOutOfBounds {
            pc,
            offset,
            code_len,
        });
    }
    Ok(())
}

fn verify_branch_table(
    code_len: usize,
    pc: usize,
    targets: u32,
) -> Result<(), RwasmModuleVerificationError> {
    if targets == 0 {
        return Err(
            RwasmModuleVerificationError::BranchTableTargetsOutOfBounds {
                pc,
                targets,
                code_len,
            },
        );
    }
    let payload_len = (targets as usize).checked_mul(2).ok_or(
        RwasmModuleVerificationError::BranchTableTargetsOutOfBounds {
            pc,
            targets,
            code_len,
        },
    )?;
    let end = pc.checked_add(payload_len).ok_or(
        RwasmModuleVerificationError::BranchTableTargetsOutOfBounds {
            pc,
            targets,
            code_len,
        },
    )?;
    if end >= code_len {
        return Err(
            RwasmModuleVerificationError::BranchTableTargetsOutOfBounds {
                pc,
                targets,
                code_len,
            },
        );
    }
    Ok(())
}

fn verify_code_target(
    code_len: usize,
    pc: usize,
    target: u32,
) -> Result<(), RwasmModuleVerificationError> {
    if target as usize >= code_len {
        return Err(RwasmModuleVerificationError::CallTargetOutOfBounds {
            pc,
            target,
            code_len,
        });
    }
    Ok(())
}

fn verify_table_index_payload(
    code: &InstructionSet,
    pc: usize,
) -> Result<(), RwasmModuleVerificationError> {
    let next = pc
        .checked_add(1)
        .and_then(|index| code.get(index))
        .copied()
        .ok_or(RwasmModuleVerificationError::MissingTableIndexPayload { pc })?;
    let Opcode::TableGet(table) = next else {
        return Err(RwasmModuleVerificationError::InvalidTableIndexPayload { pc });
    };
    verify_table_index(pc, table)
}

fn verify_table_index(pc: usize, table: u16) -> Result<(), RwasmModuleVerificationError> {
    if u32::from(table) >= N_MAX_TABLES {
        return Err(RwasmModuleVerificationError::TableIndexOutOfBounds { pc, table });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{instruction_set, InstructionSet, RwasmModuleBuilder};

    fn module_with_code(code_section: InstructionSet) -> RwasmModuleInner {
        RwasmModuleInner {
            code_section,
            data_section: vec![],
            elem_section: vec![],
            hint_section: vec![],
            source_pc: 0,
        }
    }

    fn verification_error(module: RwasmModuleInner) -> RwasmModuleVerificationError {
        let encoded = bincode::encode_to_vec(&module, bincode::config::legacy()).unwrap();
        let error = RwasmModule::new_verified(&encoded).unwrap_err();
        let RwasmModuleError::Verification(error) = error else {
            panic!("expected verification error, got {error:?}");
        };
        error
    }

    #[test]
    fn regular_decode_does_not_verify() {
        let encoded = bincode::encode_to_vec(
            module_with_code(InstructionSet::new()),
            bincode::config::legacy(),
        )
        .unwrap();
        RwasmModule::new_checked(&encoded).unwrap();
        assert!(matches!(
            RwasmModule::new_verified(&encoded),
            Err(RwasmModuleError::Verification(
                RwasmModuleVerificationError::EmptyCodeSection
            ))
        ));
    }

    #[test]
    fn regular_construction_does_not_verify() {
        let module: RwasmModule = module_with_code(InstructionSet::new()).into();
        assert_eq!(
            module.verify(),
            Err(RwasmModuleVerificationError::EmptyCodeSection)
        );

        let module = RwasmModuleBuilder::new(InstructionSet::new()).build();
        assert_eq!(
            module.verify(),
            Err(RwasmModuleVerificationError::EmptyCodeSection)
        );
    }

    #[test]
    fn rejects_source_pc_outside_code_section() {
        let mut module = module_with_code(instruction_set! { Return });
        module.source_pc = 1;
        assert_eq!(
            verification_error(module),
            RwasmModuleVerificationError::SourcePcOutOfBounds {
                source_pc: 1,
                code_len: 1,
            }
        );
    }

    #[test]
    fn rejects_branch_target_outside_code_section() {
        assert_eq!(
            verification_error(module_with_code(instruction_set! { Br(100) Return })),
            RwasmModuleVerificationError::BranchTargetOutOfBounds {
                pc: 0,
                offset: 100,
                code_len: 2,
            }
        );
    }

    #[test]
    fn rejects_call_target_outside_code_section() {
        assert_eq!(
            verification_error(module_with_code(
                instruction_set! { CallInternal(99) Return }
            )),
            RwasmModuleVerificationError::CallTargetOutOfBounds {
                pc: 0,
                target: 99,
                code_len: 2,
            }
        );
    }

    #[test]
    fn rejects_zero_local_depth() {
        let depth = 0;
        assert_eq!(
            verification_error(module_with_code(
                instruction_set! { LocalGet(depth) Return }
            )),
            RwasmModuleVerificationError::LocalDepthOutOfBounds { pc: 0, depth }
        );
    }

    #[test]
    fn rejects_local_depth_beyond_the_value_stack() {
        let depth = 2 * N_MAX_STACK_SIZE as u32;
        for code in [
            instruction_set! { LocalGet(depth) Return },
            instruction_set! { I32Const(1) LocalSet(depth) Return },
            instruction_set! { I32Const(1) LocalTee(depth) Return },
        ] {
            let pc = code.len() - 2;
            assert_eq!(
                verification_error(module_with_code(code)),
                RwasmModuleVerificationError::LocalDepthOutOfBounds { pc, depth }
            );
        }
    }

    #[test]
    fn rejects_local_depth_beyond_the_value_stack_after_a_host_call() {
        // A host call leaves the emulated height unknown, the constant bound still applies.
        let depth = N_MAX_STACK_SIZE as u32 + 1;
        assert_eq!(
            verification_error(module_with_code(
                instruction_set! { Call(70) LocalGet(depth) Return }
            )),
            RwasmModuleVerificationError::LocalDepthOutOfBounds { pc: 1, depth }
        );
    }

    #[test]
    fn rejects_popping_below_the_value_stack() {
        let mut code = InstructionSet::new();
        for _ in 0..=N_MAX_STACK_SIZE {
            code.op_drop();
        }
        code.op_return();
        assert_eq!(
            verification_error(module_with_code(code)),
            RwasmModuleVerificationError::StackUnderflow {
                pc: N_MAX_STACK_SIZE,
                height: -(N_MAX_STACK_SIZE as i64),
                pops: 1,
            }
        );
    }

    #[test]
    fn rejects_pushing_beyond_the_value_stack() {
        let mut code = InstructionSet::new();
        for _ in 0..=N_MAX_STACK_SIZE {
            code.op_i32_const(1);
        }
        code.op_return();
        assert_eq!(
            verification_error(module_with_code(code)),
            RwasmModuleVerificationError::StackOverflow {
                pc: N_MAX_STACK_SIZE,
                height: N_MAX_STACK_SIZE as i64,
                pushes: 1,
            }
        );
    }

    #[test]
    fn rejects_bulk_operands_beyond_the_value_stack() {
        let operand = N_MAX_STACK_SIZE as u32 + 1;
        assert_eq!(
            verification_error(module_with_code(
                instruction_set! { BulkConst(operand) Return }
            )),
            RwasmModuleVerificationError::StackOverflow {
                pc: 0,
                height: 0,
                pushes: operand,
            }
        );
        assert_eq!(
            verification_error(module_with_code(
                instruction_set! { BulkDrop(operand) Return }
            )),
            RwasmModuleVerificationError::StackUnderflow {
                pc: 0,
                height: 0,
                pops: operand,
            }
        );
    }

    #[test]
    fn accepts_bulk_operands_covered_by_a_stack_reservation() {
        // A Wasm function may declare far more locals than the default stack holds; the module
        // says so through its `StackCheck`, and the reservation itself traps at runtime.
        let operand = N_MAX_STACK_SIZE as u32 + 1;
        let module = module_with_code(instruction_set! {
            StackCheck(operand)
            BulkConst(operand)
            BulkDrop(operand)
            Return
        });
        assert_eq!(module.verify(), Ok(()));
    }

    #[test]
    fn rejects_code_running_past_the_code_section() {
        assert_eq!(
            verification_error(module_with_code(instruction_set! { I32Const(1) Drop })),
            RwasmModuleVerificationError::FallsThroughCodeSection { pc: 1 }
        );
    }

    #[test]
    fn accepts_locals_addressed_below_the_function_entry() {
        // A callee reads its parameters from below its own entry stack pointer, so a local depth
        // greater than the emulated height is perfectly normal.
        let module = module_with_code(instruction_set! {
            CallInternal(2)
            Return
            LocalGet(1)
            LocalSet(2)
            Return
        });
        assert_eq!(module.verify(), Ok(()));
    }

    #[test]
    fn rejects_missing_table_index_payload() {
        assert_eq!(
            verification_error(module_with_code(instruction_set! { CallIndirect(0) })),
            RwasmModuleVerificationError::MissingTableIndexPayload { pc: 0 }
        );
    }

    #[test]
    fn rejects_section_index_outside_limits() {
        let segment = N_MAX_DATA_SEGMENTS as u32;
        assert_eq!(
            verification_error(module_with_code(
                instruction_set! { MemoryInit(segment) Return }
            )),
            RwasmModuleVerificationError::DataSegmentOutOfBounds { pc: 0, segment }
        );
    }

    #[test]
    fn accepts_verified_encoded_module() {
        let module = module_with_code(instruction_set! { I32Const(1) Return });
        let encoded = bincode::encode_to_vec(module, bincode::config::legacy()).unwrap();
        RwasmModule::new_verified(&encoded).unwrap();
    }
}
