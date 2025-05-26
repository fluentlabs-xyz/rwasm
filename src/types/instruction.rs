use crate::{
    types::{
        AddressOffset,
        BlockFuel,
        BranchOffset,
        BranchTableTargets,
        CompiledFunc,
        DataSegmentIdx,
        ElementSegmentIdx,
        GlobalIdx,
        LocalDepth,
        SignatureIdx,
        TableIdx,
        UntypedValue,
    },
    MaxStackHeight,
    SysFuncIdx,
};
use alloc::{format, vec::Vec};
use bincode::{Decode, Encode};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[repr(u8)]
pub enum Opcode {
    Unreachable = 0x00,
    LocalGet(LocalDepth) = 0x01,
    LocalSet(LocalDepth) = 0x02,
    LocalTee(LocalDepth) = 0x03,
    Br(BranchOffset) = 0x04,
    BrIfEqz(BranchOffset) = 0x05,
    BrIfNez(BranchOffset) = 0x06,
    BrTable(BranchTableTargets) = 0x09,
    ConsumeFuel(BlockFuel) = 0x0a,
    Return = 0x0b,
    ReturnCallInternal(CompiledFunc) = 0x0d,
    ReturnCall(SysFuncIdx) = 0x0e,
    ReturnCallIndirect(SignatureIdx) = 0x0f,
    CallInternal(CompiledFunc) = 0x10,
    Call(SysFuncIdx) = 0x11,
    CallIndirect(SignatureIdx) = 0x12,
    SignatureCheck(SignatureIdx) = 0x13,
    StackCheck(MaxStackHeight) = 0xc6,
    Drop = 0x14,
    Select = 0x15,
    GlobalGet(GlobalIdx) = 0x16,
    GlobalSet(GlobalIdx) = 0x17,
    I32Load(AddressOffset) = 0x18,
    I32Load8S(AddressOffset) = 0x1c,
    I32Load8U(AddressOffset) = 0x1d,
    I32Load16S(AddressOffset) = 0x1e,
    I32Load16U(AddressOffset) = 0x1f,
    I32Store(AddressOffset) = 0x26,
    I32Store8(AddressOffset) = 0x2a,
    I32Store16(AddressOffset) = 0x2b,
    MemorySize = 0x2f,
    MemoryGrow = 0x30,
    MemoryFill = 0x31,
    MemoryCopy = 0x32,
    MemoryInit(DataSegmentIdx) = 0x33,
    DataDrop(DataSegmentIdx) = 0x34,
    TableSize(TableIdx) = 0x35,
    TableGrow(TableIdx) = 0x36,
    TableFill(TableIdx) = 0x37,
    TableGet(TableIdx) = 0x38,
    TableSet(TableIdx) = 0x39,
    TableCopy(TableIdx) = 0x3a,
    TableInit(ElementSegmentIdx) = 0x3b,
    ElemDrop(ElementSegmentIdx) = 0x3c,
    RefFunc(CompiledFunc) = 0x3d,
    I32Const(UntypedValue) = 0x3e,
    I32Eqz = 0x42,
    I32Eq = 0x43,
    I32Ne = 0x44,
    I32LtS = 0x45,
    I32LtU = 0x46,
    I32GtS = 0x47,
    I32GtU = 0x48,
    I32LeS = 0x49,
    I32LeU = 0x4a,
    I32GeS = 0x4b,
    I32GeU = 0x4c,
    I32Clz = 0x64,
    I32Ctz = 0x65,
    I32Popcnt = 0x66,
    I32Add = 0x67,
    I32Sub = 0x68,
    I32Mul = 0x69,
    I32DivS = 0x6a,
    I32DivU = 0x6b,
    I32RemS = 0x6c,
    I32RemU = 0x6d,
    I32And = 0x6e,
    I32Or = 0x6f,
    I32Xor = 0x70,
    I32Shl = 0x71,
    I32ShrS = 0x72,
    I32ShrU = 0x73,
    I32Rotl = 0x74,
    I32Rotr = 0x75,
    I32WrapI64 = 0xa4,
    I32Extend8S = 0xb9,
    I32Extend16S = 0xba,

    // fpu
    F32Load(AddressOffset) = 0x1a,
    F64Load(AddressOffset) = 0x1b,
    F32Store(AddressOffset) = 0x28,
    F64Store(AddressOffset) = 0x29,
    F32Eq = 0x58,
    F32Ne = 0x59,
    F32Lt = 0x5a,
    F32Gt = 0x5b,
    F32Le = 0x5c,
    F32Ge = 0x5d,
    F64Eq = 0x5e,
    F64Ne = 0x5f,
    F64Lt = 0x60,
    F64Gt = 0x61,
    F64Le = 0x62,
    F64Ge = 0x63,
    F32Abs = 0x88,
    F32Neg = 0x89,
    F32Ceil = 0x8a,
    F32Floor = 0x8b,
    F32Trunc = 0x8c,
    F32Nearest = 0x8d,
    F32Sqrt = 0x8e,
    F32Add = 0x8f,
    F32Sub = 0x90,
    F32Mul = 0x91,
    F32Div = 0x92,
    F32Min = 0x93,
    F32Max = 0x94,
    F32Copysign = 0x95,
    F64Abs = 0x96,
    F64Neg = 0x97,
    F64Ceil = 0x98,
    F64Floor = 0x99,
    F64Trunc = 0x9a,
    F64Nearest = 0x9b,
    F64Sqrt = 0x9c,
    F64Add = 0x9d,
    F64Sub = 0x9e,
    F64Mul = 0x9f,
    F64Div = 0xa0,
    F64Min = 0xa1,
    F64Max = 0xa2,
    F64Copysign = 0xa3,
    I32TruncF32S = 0xa5,
    I32TruncF32U = 0xa6,
    I32TruncF64S = 0xa7,
    I32TruncF64U = 0xa8,
    I64TruncF32S = 0xab,
    I64TruncF32U = 0xac,
    I64TruncF64S = 0xad,
    I64TruncF64U = 0xae,
    F32ConvertI32S = 0xaf,
    F32ConvertI32U = 0xb0,
    F32ConvertI64S = 0xb1,
    F32ConvertI64U = 0xb2,
    F32DemoteF64 = 0xb3,
    F64ConvertI32S = 0xb4,
    F64ConvertI32U = 0xb5,
    F64ConvertI64S = 0xb6,
    F64ConvertI64U = 0xb7,
    F64PromoteF32 = 0xb8,
    I32TruncSatF32S = 0xbe,
    I32TruncSatF32U = 0xbf,
    I32TruncSatF64S = 0xc0,
    I32TruncSatF64U = 0xc1,
    I64TruncSatF32S = 0xc2,
    I64TruncSatF32U = 0xc3,
    I64TruncSatF64S = 0xc4,
    I64TruncSatF64U = 0xc5,
}

impl core::fmt::Display for Opcode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let name = format!("{:?}", self);
        let name: Vec<_> = name.split('(').collect();
        write!(f, "{}", name[0])
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
        let data = bincode::encode_to_vec(&opcode, bincode::config::legacy()).unwrap();
        println!("{:?}", data);
    }
}
