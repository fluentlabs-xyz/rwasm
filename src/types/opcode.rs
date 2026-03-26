use crate::{
    types::{
        AddressOffset, BlockFuel, BranchOffset, BranchTableTargets, CompiledFunc, DataSegmentIdx,
        ElementSegmentIdx, GlobalIdx, LocalDepth, SignatureIdx, TableIdx, UntypedValue,
    },
    MaxStackHeight, NumLocals, SysFuncIdx, TrapCode,
};
use alloc::{format, vec::Vec};
use bincode::{Decode, Encode};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u16)]
pub enum Opcode {
    // stack/system
    Unreachable,
    Trap(TrapCode),
    LocalGet(LocalDepth),
    LocalSet(LocalDepth),
    LocalTee(LocalDepth),
    Br(BranchOffset),
    BrIfEqz(BranchOffset),
    BrIfNez(BranchOffset),
    BrTable(BranchTableTargets),
    ConsumeFuel(BlockFuel),
    ConsumeFuelStack,
    Return,
    ReturnCallInternal(CompiledFunc),
    ReturnCall(SysFuncIdx),
    ReturnCallIndirect(SignatureIdx),
    CallInternal(CompiledFunc),
    Call(SysFuncIdx),
    CallIndirect(SignatureIdx),
    SignatureCheck(SignatureIdx),
    StackCheck(MaxStackHeight),
    RefFunc(CompiledFunc),
    I32Const(UntypedValue),
    Drop,
    Select,
    GlobalGet(GlobalIdx),
    GlobalSet(GlobalIdx),

    // memory
    I32Load(AddressOffset),
    I32Load8S(AddressOffset),
    I32Load8U(AddressOffset),
    I32Load16S(AddressOffset),
    I32Load16U(AddressOffset),
    I32Store(AddressOffset),
    I32Store8(AddressOffset),
    I32Store16(AddressOffset),
    MemorySize,
    MemoryGrow,
    MemoryFill,
    MemoryCopy,
    MemoryInit(DataSegmentIdx),
    DataDrop(DataSegmentIdx),

    // table
    TableSize(TableIdx),
    TableGrow(TableIdx),
    TableFill(TableIdx),
    TableGet(TableIdx),
    TableSet(TableIdx),
    TableCopy(TableIdx, TableIdx),
    TableInit(ElementSegmentIdx),
    ElemDrop(ElementSegmentIdx),

    // alu
    I32Eqz,
    I32Eq,
    I32Ne,
    I32LtS,
    I32LtU,
    I32GtS,
    I32GtU,
    I32LeS,
    I32LeU,
    I32GeS,
    I32GeU,
    I32Clz,
    I32Ctz,
    I32Popcnt,
    I32Add,
    I32Sub,
    I32Mul,
    I32DivS,
    I32DivU,
    I32RemS,
    I32RemU,
    I32And,
    I32Or,
    I32Xor,
    I32Shl,
    I32ShrS,
    I32ShrU,
    I32Rotl,
    I32Rotr,
    I32WrapI64,
    I32Extend8S,
    I32Extend16S,
    I32Mul64,
    I32Add64,
    BulkConst(NumLocals),
    BulkDrop(NumLocals),

    // fpu
    #[cfg(feature = "fpu")]
    F32Load(AddressOffset),
    #[cfg(feature = "fpu")]
    F64Load(AddressOffset),
    #[cfg(feature = "fpu")]
    F32Store(AddressOffset),
    #[cfg(feature = "fpu")]
    F64Store(AddressOffset),
    #[cfg(feature = "fpu")]
    F32Eq,
    #[cfg(feature = "fpu")]
    F32Ne,
    #[cfg(feature = "fpu")]
    F32Lt,
    #[cfg(feature = "fpu")]
    F32Gt,
    #[cfg(feature = "fpu")]
    F32Le,
    #[cfg(feature = "fpu")]
    F32Ge,
    #[cfg(feature = "fpu")]
    F64Eq,
    #[cfg(feature = "fpu")]
    F64Ne,
    #[cfg(feature = "fpu")]
    F64Lt,
    #[cfg(feature = "fpu")]
    F64Gt,
    #[cfg(feature = "fpu")]
    F64Le,
    #[cfg(feature = "fpu")]
    F64Ge,
    #[cfg(feature = "fpu")]
    F32Abs,
    #[cfg(feature = "fpu")]
    F32Neg,
    #[cfg(feature = "fpu")]
    F32Ceil,
    #[cfg(feature = "fpu")]
    F32Floor,
    #[cfg(feature = "fpu")]
    F32Trunc,
    #[cfg(feature = "fpu")]
    F32Nearest,
    #[cfg(feature = "fpu")]
    F32Sqrt,
    #[cfg(feature = "fpu")]
    F32Add,
    #[cfg(feature = "fpu")]
    F32Sub,
    #[cfg(feature = "fpu")]
    F32Mul,
    #[cfg(feature = "fpu")]
    F32Div,
    #[cfg(feature = "fpu")]
    F32Min,
    #[cfg(feature = "fpu")]
    F32Max,
    #[cfg(feature = "fpu")]
    F32Copysign,
    #[cfg(feature = "fpu")]
    F64Abs,
    #[cfg(feature = "fpu")]
    F64Neg,
    #[cfg(feature = "fpu")]
    F64Ceil,
    #[cfg(feature = "fpu")]
    F64Floor,
    #[cfg(feature = "fpu")]
    F64Trunc,
    #[cfg(feature = "fpu")]
    F64Nearest,
    #[cfg(feature = "fpu")]
    F64Sqrt,
    #[cfg(feature = "fpu")]
    F64Add,
    #[cfg(feature = "fpu")]
    F64Sub,
    #[cfg(feature = "fpu")]
    F64Mul,
    #[cfg(feature = "fpu")]
    F64Div,
    #[cfg(feature = "fpu")]
    F64Min,
    #[cfg(feature = "fpu")]
    F64Max,
    #[cfg(feature = "fpu")]
    F64Copysign,
    #[cfg(feature = "fpu")]
    I32TruncF32S,
    #[cfg(feature = "fpu")]
    I32TruncF32U,
    #[cfg(feature = "fpu")]
    I32TruncF64S,
    #[cfg(feature = "fpu")]
    I32TruncF64U,
    #[cfg(feature = "fpu")]
    I64TruncF32S,
    #[cfg(feature = "fpu")]
    I64TruncF32U,
    #[cfg(feature = "fpu")]
    I64TruncF64S,
    #[cfg(feature = "fpu")]
    I64TruncF64U,
    #[cfg(feature = "fpu")]
    F32ConvertI32S,
    #[cfg(feature = "fpu")]
    F32ConvertI32U,
    #[cfg(feature = "fpu")]
    F32ConvertI64S,
    #[cfg(feature = "fpu")]
    F32ConvertI64U,
    #[cfg(feature = "fpu")]
    F32DemoteF64,
    #[cfg(feature = "fpu")]
    F64ConvertI32S,
    #[cfg(feature = "fpu")]
    F64ConvertI32U,
    #[cfg(feature = "fpu")]
    F64ConvertI64S,
    #[cfg(feature = "fpu")]
    F64ConvertI64U,
    #[cfg(feature = "fpu")]
    F64PromoteF32,
    #[cfg(feature = "fpu")]
    I32TruncSatF32S,
    #[cfg(feature = "fpu")]
    I32TruncSatF32U,
    #[cfg(feature = "fpu")]
    I32TruncSatF64S,
    #[cfg(feature = "fpu")]
    I32TruncSatF64U,
    #[cfg(feature = "fpu")]
    I64TruncSatF32S,
    #[cfg(feature = "fpu")]
    I64TruncSatF32U,
    #[cfg(feature = "fpu")]
    I64TruncSatF64S,
    #[cfg(feature = "fpu")]
    I64TruncSatF64U,
}

impl core::fmt::Display for Opcode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if f.alternate() {
            let name = format!("{:?}", self);
            let name: Vec<_> = name.split('(').collect();
            write!(f, "{}", name[0])
        } else {
            match self {
                Opcode::I32Const(value) => write!(f, "I32Const({})", value),
                Opcode::ConsumeFuel(value) => write!(f, "ConsumeFuel({})", value),
                Opcode::Br(value) => write!(f, "Br({})", value.to_i32()),
                Opcode::BrIfEqz(value) => write!(f, "BrIfEqz({})", value.to_i32()),
                Opcode::BrIfNez(value) => write!(f, "BrIfNez({})", value.to_i32()),
                _ => write!(f, "{:?}", self),
            }
        }
    }
}

impl Opcode {
    pub fn update_branch_offset<I: Into<BranchOffset>>(&mut self, new_offset: I) {
        match self {
            Opcode::Br(offset) | Opcode::BrIfEqz(offset) | Opcode::BrIfNez(offset) => {
                *offset = new_offset.into();
            }
            _ => unreachable!(),
        }
    }

    pub fn is_alu_instruction(self) -> bool {
        match self {
            Opcode::I32Eq
            | Opcode::I32Eqz
            | Opcode::I32Ne
            | Opcode::I32LtS
            | Opcode::I32LtU
            | Opcode::I32GtU
            | Opcode::I32GtS
            | Opcode::I32LeS
            | Opcode::I32LeU
            | Opcode::I32GeS
            | Opcode::I32GeU
            | Opcode::I32Clz
            | Opcode::I32Ctz
            | Opcode::I32Popcnt
            | Opcode::I32Add
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
            | Opcode::I32Rotr => true,
            _ => false,
        }
    }

    pub fn is_memory_instruction(self) -> bool {
        match self {
            Opcode::I32Load8S(_)
            | Opcode::I32Load8U(_)
            | Opcode::I32Load16S(_)
            | Opcode::I32Load16U(_)
            | Opcode::I32Load(_)
            | Opcode::I32Store8(_)
            | Opcode::I32Store16(_)
            | Opcode::I32Store(_) => true,
            _ => false,
        }
    }

    pub fn is_memory_load_instruction(self) -> bool {
        match self {
            Opcode::I32Load8S(_)
            | Opcode::I32Load8U(_)
            | Opcode::I32Load16S(_)
            | Opcode::I32Load16U(_)
            | Opcode::I32Load(_) => true,
            _ => false,
        }
    }

    pub fn is_memory_store_instruction(self) -> bool {
        match self {
            Opcode::I32Store8(_) | Opcode::I32Store16(_) | Opcode::I32Store(_) => true,
            _ => false,
        }
    }

    pub fn is_ecall_instruction(self) -> bool {
        match self {
            Opcode::Call(_) | Opcode::ReturnCall(_) => true,
            _ => false,
        }
    }

    pub fn is_branch_instruction(self) -> bool {
        match self {
            Opcode::Br(_) | Opcode::BrIfEqz(_) | Opcode::BrIfNez(_) => true,
            _ => false,
        }
    }

    pub fn is_jump_instruction(self) -> bool {
        false
    }

    pub fn is_halt(self) -> bool {
        false
    }

    pub fn is_unary_instruction(self) -> bool {
        match self {
            Opcode::I32Clz | Opcode::I32Ctz | Opcode::I32Popcnt | Opcode::I32Eqz => true,
            _ => false,
        }
    }

    pub fn is_binary_instruction(self) -> bool {
        match self {
            Opcode::I32Eq
            | Opcode::I32Ne
            | Opcode::I32LtS
            | Opcode::I32LtU
            | Opcode::I32GtU
            | Opcode::I32GtS
            | Opcode::I32LeS
            | Opcode::I32LeU
            | Opcode::I32GeS
            | Opcode::I32GeU
            | Opcode::I32Add
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
            | Opcode::I32Rotr => true,
            _ => false,
        }
    }

    pub fn is_nullary(&self) -> bool {
        match self {
            Opcode::Br(_) | Opcode::I32Const(_) => true,
            _ => false,
        }
    }

    pub fn is_call_instruction(self) -> bool {
        match self {
            Opcode::CallIndirect(_)
            | Opcode::CallInternal(_)
            | Opcode::ReturnCallIndirect(_)
            | Opcode::ReturnCallInternal(_)
            | Opcode::Return => true,
            _ => false,
        }
    }

    pub fn is_const_instruction(self) -> bool {
        matches!(self, Opcode::I32Const(_) | Opcode::RefFunc(_))
    }

    pub fn is_local_instruction(self) -> bool {
        matches!(
            self,
            Opcode::LocalGet(_) | Opcode::LocalSet(_) | Opcode::LocalTee(_)
        )
    }

    #[inline]
    pub fn aux_value(&self) -> u32 {
        match self {
            Opcode::Trap(trap_code) => *trap_code as u32,
            Opcode::LocalGet(depth) => *depth,
            Opcode::LocalSet(depth) => *depth,
            Opcode::LocalTee(depth) => *depth,
            Opcode::Br(branch_offset) => branch_offset.to_i32() as u32,
            Opcode::BrIfEqz(branch_offset) => branch_offset.to_i32() as u32,
            Opcode::BrIfNez(branch_offset) => branch_offset.to_i32() as u32,
            Opcode::BrTable(target) => *target,
            Opcode::ConsumeFuel(block_fuel) => *block_fuel,
            Opcode::ReturnCallInternal(func) => *func,
            Opcode::ReturnCall(sys_func_id) => *sys_func_id,
            Opcode::ReturnCallIndirect(func) => *func,
            Opcode::CallInternal(sign_id) => *sign_id,
            Opcode::Call(func) => *func,
            Opcode::CallIndirect(sys_func_id) => *sys_func_id,
            Opcode::SignatureCheck(sys_func_id) => *sys_func_id,
            Opcode::StackCheck(max_height) => *max_height,
            Opcode::RefFunc(func) => *func,
            Opcode::I32Const(untyped_value) => untyped_value.to_bits(),
            Opcode::GlobalGet(idx) => *idx,
            Opcode::GlobalSet(idx) => *idx,
            Opcode::I32Load(offset) => *offset,
            Opcode::I32Load8S(offset) => *offset,
            Opcode::I32Load8U(offset) => *offset,
            Opcode::I32Load16S(offset) => *offset,
            Opcode::I32Load16U(offset) => *offset,
            Opcode::I32Store(offset) => *offset,
            Opcode::I32Store8(offset) => *offset,
            Opcode::I32Store16(offset) => *offset,
            Opcode::MemoryInit(seg_id) => *seg_id,
            Opcode::DataDrop(seg_id) => *seg_id,
            Opcode::TableSize(table_id) => *table_id as u32,
            Opcode::TableGrow(table_id) => *table_id as u32,
            Opcode::TableFill(table_id) => *table_id as u32,
            Opcode::TableGet(table_id) => *table_id as u32,
            Opcode::TableSet(table_id) => *table_id as u32,
            Opcode::TableCopy(dst_table_idx, src_table_idx) => {
                (*dst_table_idx as u32) << 16 | (*src_table_idx as u32)
            }
            Opcode::TableInit(ele_seg_id) => *ele_seg_id,
            Opcode::ElemDrop(ele_seg_id) => *ele_seg_id,
            _ => 0,
        }
    }

    pub fn code(&self) -> u32 {
        // TODO(wangyao): "is it safe?"
        unsafe { *<*const _>::from(self).cast::<u16>() as u32 }
    }
}

#[derive(Default, Debug, PartialEq, Clone, Eq, Hash)]
pub struct OpcodeMeta {
    pub index: usize,
    pub pos: usize,
    pub opcode: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcode_encoding() {
        let opcode = Opcode::LocalGet(7);
        let data = bincode::encode_to_vec(opcode, bincode::config::legacy()).unwrap();
        println!("{:?}", data);
    }

    #[test]
    fn test_opcode_size() {
        assert_eq!(size_of::<Opcode>(), 8);
    }
}
