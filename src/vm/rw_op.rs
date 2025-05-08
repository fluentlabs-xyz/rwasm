use crate::Opcode;
use alloc::vec::Vec;

#[derive(Debug, Copy, Clone)]
pub enum RwOp {
    StackWrite(u32),
    StackRead(u32),
    GlobalWrite(u32),
    GlobalRead(u32),
    MemoryWrite {
        offset: u32,
        length: u32,
        signed: bool,
    },
    MemoryRead {
        offset: u32,
        length: u32,
        signed: bool,
    },
    MemorySizeWrite,
    MemorySizeRead,
    TableSizeRead(u32),
    TableSizeWrite(u32),
    TableElemRead(u32),
    TableElemWrite(u32),
    DataWrite(u32),
    DataRead(u32),
}

impl Opcode {
    pub fn get_rw_count(&self) -> usize {
        let mut rw_count = 0;
        for rw_op in self.get_rw_ops() {
            match rw_op {
                RwOp::MemoryWrite { length, .. } => rw_count += length as usize,
                RwOp::MemoryRead { length, .. } => rw_count += length as usize,
                _ => rw_count += 1,
            }
        }
        rw_count
    }

    pub fn get_rw_ops(&self) -> Vec<RwOp> {
        let mut stack_ops = Vec::new();
        match *self {
            Opcode::LocalGet(local_depth) => {
                stack_ops.push(RwOp::StackRead(local_depth.to_usize() as u32));
                stack_ops.push(RwOp::StackWrite(0));
            }
            Opcode::LocalSet(local_depth) => {
                stack_ops.push(RwOp::StackRead(0));
                // local depth can't be zero otherwise this op is useless
                if local_depth.to_usize() > 0 {
                    stack_ops.push(RwOp::StackWrite(local_depth.to_usize() as u32 - 1));
                } else {
                    stack_ops.push(RwOp::StackWrite(0));
                }
            }
            Opcode::LocalTee(local_depth) => {
                stack_ops.push(RwOp::StackRead(0));
                // local depth can't be zero otherwise this op is useless
                if local_depth.to_usize() > 0 {
                    stack_ops.push(RwOp::StackWrite(local_depth.to_usize() as u32 - 1));
                } else {
                    stack_ops.push(RwOp::StackWrite(0));
                }
            }
            Opcode::Br(_) => {}
            Opcode::BrIfEqz(_) | Opcode::BrIfNez(_) => {
                stack_ops.push(RwOp::StackRead(0));
            }
            Opcode::BrAdjust(_) => {}
            Opcode::BrAdjustIfNez(_) | Opcode::BrTable(_) => {
                stack_ops.push(RwOp::StackRead(0));
            }
            Opcode::Unreachable | Opcode::ConsumeFuel(_) | Opcode::Return(_) => {}
            Opcode::ReturnIfNez(_) => {
                stack_ops.push(RwOp::StackRead(0));
            }
            Opcode::ReturnCallInternal(_) | Opcode::ReturnCall(_) => {}
            Opcode::ReturnCallIndirect(_) => {
                stack_ops.push(RwOp::StackRead(0));
            }
            Opcode::CallInternal(_) => {}
            Opcode::Call(_) => {}
            Opcode::CallIndirect(_) => {
                stack_ops.push(RwOp::StackRead(0));
            }
            Opcode::Drop => {
                stack_ops.push(RwOp::StackRead(0));
            }
            Opcode::Select => {
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackWrite(0));
            }
            Opcode::GlobalGet(val) => {
                stack_ops.push(RwOp::GlobalRead(val.to_u32()));
                stack_ops.push(RwOp::StackWrite(0));
            }
            Opcode::GlobalSet(val) => {
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::GlobalWrite(val.to_u32()));
            }
            Opcode::I32Load(val)
            | Opcode::I64Load(val)
            | Opcode::F32Load(val)
            | Opcode::F64Load(val)
            | Opcode::I32Load8S(val)
            | Opcode::I32Load8U(val)
            | Opcode::I32Load16S(val)
            | Opcode::I32Load16U(val)
            | Opcode::I64Load8S(val)
            | Opcode::I64Load8U(val)
            | Opcode::I64Load16S(val)
            | Opcode::I64Load16U(val)
            | Opcode::I64Load32S(val)
            | Opcode::I64Load32U(val) => {
                let (_, commit_byte_len, signed) = Self::load_instr_meta(self);
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::MemoryRead {
                    offset: val.into_inner(),
                    length: commit_byte_len as u32,
                    signed,
                });
                stack_ops.push(RwOp::StackWrite(0));
            }
            Opcode::I32Store(val)
            | Opcode::I64Store(val)
            | Opcode::F32Store(val)
            | Opcode::F64Store(val)
            | Opcode::I32Store8(val)
            | Opcode::I32Store16(val)
            | Opcode::I64Store8(val)
            | Opcode::I64Store16(val)
            | Opcode::I64Store32(val) => {
                let length = Self::store_instr_meta(self);
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::MemoryWrite {
                    offset: val.into_inner(),
                    length: length as u32,
                    signed: false,
                });
            }
            Opcode::MemorySize => {
                stack_ops.push(RwOp::MemorySizeRead);
                stack_ops.push(RwOp::StackWrite(0));
            }
            Opcode::MemoryGrow => {
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackWrite(0));
                stack_ops.push(RwOp::MemorySizeWrite);
            }
            Opcode::MemoryFill | Opcode::MemoryCopy => {
                // unreachable!("not implemented here")
            }
            Opcode::MemoryInit(_) => {}
            Opcode::DataDrop(_) => {}

            Opcode::TableSize(table_idx) => {
                stack_ops.push(RwOp::TableSizeRead(table_idx.to_u32()));
                stack_ops.push(RwOp::StackWrite(0));
            }
            Opcode::TableGrow(table_idx) => {
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::TableSizeWrite(table_idx.to_u32()));
                stack_ops.push(RwOp::StackWrite(0));
            }
            Opcode::TableFill(table_idx) => {
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::TableSizeRead(table_idx.to_u32()));
            }
            Opcode::TableGet(_) => {
                panic!("custom function is used");
            }
            Opcode::TableSet(table_idx) => {
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::TableElemWrite(table_idx.to_u32()));
                stack_ops.push(RwOp::TableSizeRead(table_idx.to_u32()));
            }
            Opcode::TableCopy(_) => {
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackRead(0));
            }
            Opcode::TableInit(_) => {}

            Opcode::ElemDrop(_) => {}
            Opcode::RefFunc(_) => {
                stack_ops.push(RwOp::StackWrite(0));
            }
            Opcode::ConstRef(_) => stack_ops.push(RwOp::StackWrite(0)),

            Opcode::I32Eqz
            | Opcode::I32Eq
            | Opcode::I64Eqz
            | Opcode::I64Eq
            | Opcode::I32Ne
            | Opcode::I64Ne => {
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackWrite(0));
            }
            Opcode::I32LtS
            | Opcode::I32LtU
            | Opcode::I32GtS
            | Opcode::I32GtU
            | Opcode::I32LeS
            | Opcode::I32LeU
            | Opcode::I32GeS
            | Opcode::I32GeU
            | Opcode::I64LtS
            | Opcode::I64LtU
            | Opcode::I64GtS
            | Opcode::I64GtU
            | Opcode::I64LeS
            | Opcode::I64LeU
            | Opcode::I64GeS
            | Opcode::I64GeU
            | Opcode::F32Eq
            | Opcode::F32Lt
            | Opcode::F32Gt
            | Opcode::F32Le
            | Opcode::F32Ge
            | Opcode::F32Ne
            | Opcode::F64Eq
            | Opcode::F64Ne
            | Opcode::F64Lt
            | Opcode::F64Gt
            | Opcode::F64Le
            | Opcode::F64Ge => {
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackWrite(0));
            }

            Opcode::I32Clz
            | Opcode::I64Clz
            | Opcode::I32Ctz
            | Opcode::I64Ctz
            | Opcode::I32Popcnt
            | Opcode::I64Popcnt => {
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackWrite(0));
            }

            Opcode::I32Add
            | Opcode::I32Sub
            | Opcode::I32Mul
            | Opcode::I32DivS
            | Opcode::I32DivU
            | Opcode::I32RemS
            | Opcode::I32RemU
            | Opcode::I32And
            | Opcode::I32Or
            | Opcode::I32Xor
            | Opcode::I32Shl
            | Opcode::I32ShrS
            | Opcode::I32ShrU
            | Opcode::I32Rotl
            | Opcode::I32Rotr
            | Opcode::I64Add
            | Opcode::I64Sub
            | Opcode::I64Mul
            | Opcode::I64DivS
            | Opcode::I64DivU
            | Opcode::I64RemS
            | Opcode::I64RemU
            | Opcode::I64And
            | Opcode::I64Or
            | Opcode::I64Xor
            | Opcode::I64Shl
            | Opcode::I64ShrS
            | Opcode::I64ShrU
            | Opcode::I64Rotl
            | Opcode::I64Rotr
            | Opcode::F32Add
            | Opcode::F32Sub
            | Opcode::F32Mul
            | Opcode::F32Div
            | Opcode::F32Min
            | Opcode::F32Max
            | Opcode::F32Copysign
            | Opcode::F64Add
            | Opcode::F64Sub
            | Opcode::F64Mul
            | Opcode::F64Div
            | Opcode::F64Min
            | Opcode::F64Max
            | Opcode::F64Copysign => {
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackWrite(0));
            }

            Opcode::I32WrapI64
            | Opcode::I32TruncF32S
            | Opcode::I32TruncF32U
            | Opcode::I32TruncF64S
            | Opcode::I32TruncF64U
            | Opcode::I64ExtendI32S
            | Opcode::I64ExtendI32U
            | Opcode::I64TruncF32S
            | Opcode::I64TruncF32U
            | Opcode::I64TruncF64S
            | Opcode::I64TruncF64U
            | Opcode::F32ConvertI32S
            | Opcode::F32ConvertI32U
            | Opcode::F32ConvertI64S
            | Opcode::F32ConvertI64U
            | Opcode::F32DemoteF64
            | Opcode::F64ConvertI32S
            | Opcode::F64ConvertI32U
            | Opcode::F64ConvertI64S
            | Opcode::F64ConvertI64U
            | Opcode::F64PromoteF32
            | Opcode::I32Extend8S
            | Opcode::I32Extend16S
            | Opcode::I64Extend8S
            | Opcode::I64Extend16S
            | Opcode::I64Extend32S
            | Opcode::I32TruncSatF32S
            | Opcode::I32TruncSatF32U
            | Opcode::I32TruncSatF64S
            | Opcode::I32TruncSatF64U
            | Opcode::I64TruncSatF32S
            | Opcode::I64TruncSatF32U
            | Opcode::I64TruncSatF64S
            | Opcode::I64TruncSatF64U => {
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackWrite(0));
            }

            Opcode::F32Sqrt => {
                stack_ops.push(RwOp::StackRead(0));
                stack_ops.push(RwOp::StackWrite(0));
            }

            _ => unreachable!("not supported rws for opcode: {:?}", self),
        }
        stack_ops
    }

    pub fn get_stack_diff(&self) -> i32 {
        let mut stack_diff = 0;
        for rw_op in self.get_rw_ops() {
            match rw_op {
                RwOp::StackWrite(_) => stack_diff += 1,
                RwOp::StackRead(_) => stack_diff -= 1,
                _ => {}
            }
        }
        stack_diff
    }
}
