use super::{RwasmModule, RwasmModuleInner};
use crate::{InstructionSet, Opcode, N_MAX_DATA_SEGMENTS, N_MAX_ELEM_SEGMENTS, N_MAX_TABLES};
use bincode::error::DecodeError;

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
    Ok(())
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
