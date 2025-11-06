use crate::{
    types::{
        AddressOffset, BlockFuel, BranchOffset, BranchTableTargets, CompiledFunc, DataSegmentIdx,
        ElementSegmentIdx, GlobalIdx, LocalDepth, SignatureIdx, TableIdx, UntypedValue,
    },
    MaxStackHeight, SysFuncIdx, TrapCode,
};
use alloc::{format, vec::Vec};
use bincode::{Decode, Encode};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u16)]
pub enum Opcode {
    // stack/system
    Unreachable = 0x00,
    Trap(TrapCode) = 0x01,
    LocalGet(LocalDepth) = 0x10,
    LocalSet(LocalDepth) = 0x11,
    LocalTee(LocalDepth) = 0x12,
    Br(BranchOffset) = 0x20,
    BrIfEqz(BranchOffset) = 0x21,
    BrIfNez(BranchOffset) = 0x22,
    BrTable(BranchTableTargets) = 0x23,
    ConsumeFuel(BlockFuel) = 0x30,
    ConsumeFuelStack = 0x31,
    Return = 0x40,
    ReturnCallInternal(CompiledFunc) = 0x41,
    ReturnCall(SysFuncIdx) = 0x42,
    ReturnCallIndirect(SignatureIdx) = 0x43,
    CallInternal(CompiledFunc) = 0x44,
    Call(SysFuncIdx) = 0x45,
    CallIndirect(SignatureIdx) = 0x46,
    SignatureCheck(SignatureIdx) = 0x50,
    StackCheck(MaxStackHeight) = 0x51,
    RefFunc(CompiledFunc) = 0x60,
    I32Const(UntypedValue) = 0x61,
    Drop = 0x62,
    Select = 0x63,
    GlobalGet(GlobalIdx) = 0x70,
    GlobalSet(GlobalIdx) = 0x71,

    // memory
    I32Load(AddressOffset) = 0x80,
    I32Load8S(AddressOffset) = 0x81,
    I32Load8U(AddressOffset) = 0x82,
    I32Load16S(AddressOffset) = 0x83,
    I32Load16U(AddressOffset) = 0x84,
    I32Store(AddressOffset) = 0x85,
    I32Store8(AddressOffset) = 0x86,
    I32Store16(AddressOffset) = 0x87,
    MemorySize = 0x88,
    MemoryGrow = 0x89,
    MemoryFill = 0x8a,
    MemoryCopy = 0x8b,
    MemoryInit(DataSegmentIdx) = 0x8c,
    DataDrop(DataSegmentIdx) = 0x8d,

    // table
    TableSize(TableIdx) = 0x90,
    TableGrow(TableIdx) = 0x91,
    TableFill(TableIdx) = 0x92,
    TableGet(TableIdx) = 0x93,
    TableSet(TableIdx) = 0x94,
    TableCopy(TableIdx, TableIdx) = 0x95,
    TableInit(ElementSegmentIdx) = 0x96,
    ElemDrop(ElementSegmentIdx) = 0x97,

    // alu
    I32Eqz = 0xa0,
    I32Eq = 0xa1,
    I32Ne = 0xa2,
    I32LtS = 0xa3,
    I32LtU = 0xa4,
    I32GtS = 0xa5,
    I32GtU = 0xa6,
    I32LeS = 0xa7,
    I32LeU = 0xa8,
    I32GeS = 0xa9,
    I32GeU = 0xaa,
    I32Clz = 0xab,
    I32Ctz = 0xac,
    I32Popcnt = 0xad,
    I32Add = 0xae,
    I32Sub = 0xaf,
    I32Mul = 0xb0,
    I32DivS = 0xb1,
    I32DivU = 0xb2,
    I32RemS = 0xb3,
    I32RemU = 0xb4,
    I32And = 0xb5,
    I32Or = 0xb6,
    I32Xor = 0xb7,
    I32Shl = 0xb8,
    I32ShrS = 0xb9,
    I32ShrU = 0xba,
    I32Rotl = 0xbb,
    I32Rotr = 0xbc,
    I32WrapI64 = 0xbd,
    I32Extend8S = 0xbe,
    I32Extend16S = 0xbf,
    I32Mul64 = 0xc0,
    I32Add64 = 0xc1,

    // fpu
    #[cfg(feature = "fpu")]
    F32Load(AddressOffset) = 0xff00,
    #[cfg(feature = "fpu")]
    F64Load(AddressOffset) = 0xff01,
    #[cfg(feature = "fpu")]
    F32Store(AddressOffset) = 0xff02,
    #[cfg(feature = "fpu")]
    F64Store(AddressOffset) = 0xff03,
    #[cfg(feature = "fpu")]
    F32Eq = 0xff04,
    #[cfg(feature = "fpu")]
    F32Ne = 0xff05,
    #[cfg(feature = "fpu")]
    F32Lt = 0xff06,
    #[cfg(feature = "fpu")]
    F32Gt = 0xff07,
    #[cfg(feature = "fpu")]
    F32Le = 0xff08,
    #[cfg(feature = "fpu")]
    F32Ge = 0xff09,
    #[cfg(feature = "fpu")]
    F64Eq = 0xff0a,
    #[cfg(feature = "fpu")]
    F64Ne = 0xff0b,
    #[cfg(feature = "fpu")]
    F64Lt = 0xff0c,
    #[cfg(feature = "fpu")]
    F64Gt = 0xff0d,
    #[cfg(feature = "fpu")]
    F64Le = 0xff0e,
    #[cfg(feature = "fpu")]
    F64Ge = 0xff0f,
    #[cfg(feature = "fpu")]
    F32Abs = 0xff10,
    #[cfg(feature = "fpu")]
    F32Neg = 0xff11,
    #[cfg(feature = "fpu")]
    F32Ceil = 0xff12,
    #[cfg(feature = "fpu")]
    F32Floor = 0xff13,
    #[cfg(feature = "fpu")]
    F32Trunc = 0xff14,
    #[cfg(feature = "fpu")]
    F32Nearest = 0xff15,
    #[cfg(feature = "fpu")]
    F32Sqrt = 0xff16,
    #[cfg(feature = "fpu")]
    F32Add = 0xff17,
    #[cfg(feature = "fpu")]
    F32Sub = 0xff18,
    #[cfg(feature = "fpu")]
    F32Mul = 0xff19,
    #[cfg(feature = "fpu")]
    F32Div = 0xff1a,
    #[cfg(feature = "fpu")]
    F32Min = 0xff1b,
    #[cfg(feature = "fpu")]
    F32Max = 0xff1c,
    #[cfg(feature = "fpu")]
    F32Copysign = 0xff1d,
    #[cfg(feature = "fpu")]
    F64Abs = 0xff1e,
    #[cfg(feature = "fpu")]
    F64Neg = 0xff1f,
    #[cfg(feature = "fpu")]
    F64Ceil = 0xff20,
    #[cfg(feature = "fpu")]
    F64Floor = 0xff21,
    #[cfg(feature = "fpu")]
    F64Trunc = 0xff22,
    #[cfg(feature = "fpu")]
    F64Nearest = 0xff23,
    #[cfg(feature = "fpu")]
    F64Sqrt = 0xff24,
    #[cfg(feature = "fpu")]
    F64Add = 0xff25,
    #[cfg(feature = "fpu")]
    F64Sub = 0xff26,
    #[cfg(feature = "fpu")]
    F64Mul = 0xff27,
    #[cfg(feature = "fpu")]
    F64Div = 0xff28,
    #[cfg(feature = "fpu")]
    F64Min = 0xff29,
    #[cfg(feature = "fpu")]
    F64Max = 0xff2a,
    #[cfg(feature = "fpu")]
    F64Copysign = 0xff2b,
    #[cfg(feature = "fpu")]
    I32TruncF32S = 0xff2c,
    #[cfg(feature = "fpu")]
    I32TruncF32U = 0xff2d,
    #[cfg(feature = "fpu")]
    I32TruncF64S = 0xff2e,
    #[cfg(feature = "fpu")]
    I32TruncF64U = 0xff2f,
    #[cfg(feature = "fpu")]
    I64TruncF32S = 0xff30,
    #[cfg(feature = "fpu")]
    I64TruncF32U = 0xff31,
    #[cfg(feature = "fpu")]
    I64TruncF64S = 0xff32,
    #[cfg(feature = "fpu")]
    I64TruncF64U = 0xff33,
    #[cfg(feature = "fpu")]
    F32ConvertI32S = 0xff34,
    #[cfg(feature = "fpu")]
    F32ConvertI32U = 0xff35,
    #[cfg(feature = "fpu")]
    F32ConvertI64S = 0xff36,
    #[cfg(feature = "fpu")]
    F32ConvertI64U = 0xff37,
    #[cfg(feature = "fpu")]
    F32DemoteF64 = 0xff38,
    #[cfg(feature = "fpu")]
    F64ConvertI32S = 0xff39,
    #[cfg(feature = "fpu")]
    F64ConvertI32U = 0xff3a,
    #[cfg(feature = "fpu")]
    F64ConvertI64S = 0xff3b,
    #[cfg(feature = "fpu")]
    F64ConvertI64U = 0xff3c,
    #[cfg(feature = "fpu")]
    F64PromoteF32 = 0xff3d,
    #[cfg(feature = "fpu")]
    I32TruncSatF32S = 0xff3e,
    #[cfg(feature = "fpu")]
    I32TruncSatF32U = 0xff3f,
    #[cfg(feature = "fpu")]
    I32TruncSatF64S = 0xff40,
    #[cfg(feature = "fpu")]
    I32TruncSatF64U = 0xff41,
    #[cfg(feature = "fpu")]
    I64TruncSatF32S = 0xff42,
    #[cfg(feature = "fpu")]
    I64TruncSatF32U = 0xff43,
    #[cfg(feature = "fpu")]
    I64TruncSatF64S = 0xff44,
    #[cfg(feature = "fpu")]
    I64TruncSatF64U = 0xff45,
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
            | Opcode::I32Rotr
            | Opcode::I32Extend8S
            | Opcode::I32Extend16S => true,
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
            Opcode::Call(_)
            | Opcode::ReturnCall(_)
            | Opcode::TableInit(_)
            | Opcode::TableGrow(_) => true,
            _ => false,
        }
    }

    pub fn is_branch_instruction(self) -> bool {
        match self {
            Opcode::Br(_) | Opcode::BrIfEqz(_) | Opcode::BrIfNez(_) | Opcode::BrTable(_) => true,
            _ => false,
        }
    }

    pub fn is_jump_instruction(self) -> bool {
        match self {
            _ => false,
        }
    }

    pub fn is_halt(self) -> bool {
        match self {
            _ => false,
        }
    }

    pub fn is_unary_instruction(self) -> bool {
        match self {
            Opcode::I32Clz
            | Opcode::I32Ctz
            | Opcode::I32Popcnt
            | Opcode::I32Eqz
            | Opcode::I32Extend8S
            | Opcode::I32Extend16S => true,
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
            | Opcode::Call(_)
            | Opcode::Return
            | Opcode::ReturnCallIndirect(_)
            | Opcode::ReturnCallInternal(_)
            | Opcode::ReturnCall(_) => true,
            _ => false,
        }
    }

    pub fn is_const_instruction(self) -> bool {
        match self {
            Opcode::I32Const(_) | Opcode::RefFunc(_) => true,
            _ => false,
        }
    }

    pub fn is_local_instruction(self) -> bool {
        match self {
            Opcode::LocalGet(_) | Opcode::LocalSet(_) | Opcode::LocalTee(_) => true,
            _ => false,
        }
    }

    pub fn is_state_instrucition(self) -> bool {
        match self {
            Opcode::MemoryCopy
            | Opcode::MemoryGrow
            | Opcode::MemorySize
            | Opcode::ConsumeFuel(_)
            | Opcode::ConsumeFuelStack => true,
            _ => false,
        }
    }

    pub fn is_table_instruction(self) -> bool {
        match self {
            Opcode::TableCopy(_, _)
            | Opcode::TableFill(_)
            | Opcode::TableInit(_)
            | Opcode::TableGet(_)
            | Opcode::TableSize(_)
            | Opcode::TableSet(_)
            | Opcode::TableGrow(_) => true,
            _ => false,
        }
    }

    pub fn is_fat_op(self) -> bool {
        match self {
            Opcode::TableInit(_) | Opcode::TableGrow(_) => true,
            _ => false,
        }
    }

    #[inline]
    pub fn aux_value(&self) -> u32 {
        match self {
            Opcode::Trap(trap_code) => *trap_code as u32,
            Opcode::LocalGet(depth) => *depth as u32,
            Opcode::LocalSet(depth) => *depth as u32,
            Opcode::LocalTee(depth) => *depth as u32,
            Opcode::Br(branch_offset) => branch_offset.to_i32() as u32,
            Opcode::BrIfEqz(branch_offset) => branch_offset.to_i32() as u32,
            Opcode::BrIfNez(branch_offset) => branch_offset.to_i32() as u32,
            Opcode::BrTable(target) => *target as u32,
            Opcode::ConsumeFuel(block_fuel) => *block_fuel,
            Opcode::ReturnCallInternal(func) => *func as u32,
            Opcode::ReturnCall(sys_func_id) => *sys_func_id as u32,
            Opcode::ReturnCallIndirect(func) => *func as u32,
            Opcode::CallInternal(sign_id) => *sign_id as u32,
            Opcode::Call(func) => *func as u32,
            Opcode::CallIndirect(sys_func_id) => *sys_func_id as u32,
            Opcode::SignatureCheck(sys_func_id) => *sys_func_id as u32,
            Opcode::StackCheck(max_height) => *max_height as u32,
            Opcode::RefFunc(func) => *func as u32,
            Opcode::I32Const(untyped_value) => untyped_value.to_bits(),
            Opcode::GlobalGet(idx) => *idx as u32,
            Opcode::GlobalSet(idx) => *idx as u32,
            Opcode::I32Load(offset) => *offset as u32,
            Opcode::I32Load8S(offset) => *offset as u32,
            Opcode::I32Load8U(offset) => *offset as u32,
            Opcode::I32Load16S(offset) => *offset as u32,
            Opcode::I32Load16U(offset) => *offset as u32,
            Opcode::I32Store(offset) => *offset as u32,
            Opcode::I32Store8(offset) => *offset as u32,
            Opcode::I32Store16(offset) => *offset as u32,
            Opcode::MemoryInit(seg_id) => *seg_id as u32,
            Opcode::DataDrop(seg_id) => *seg_id as u32,
            Opcode::TableSize(table_id) => *table_id as u32,
            Opcode::TableGrow(table_id) => *table_id as u32,
            Opcode::TableFill(table_id) => *table_id as u32,
            Opcode::TableGet(table_id) => *table_id as u32,
            Opcode::TableSet(table_id) => *table_id as u32,
            Opcode::TableCopy(dst_table_idx, src_table_idx) => {
                (*dst_table_idx as u32) << 16 | (*src_table_idx as u32)
            }
            Opcode::TableInit(ele_seg_id) => *ele_seg_id as u32,
            Opcode::ElemDrop(ele_seg_id) => *ele_seg_id as u32,
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
        let data = bincode::encode_to_vec(&opcode, bincode::config::legacy()).unwrap();
        println!("{:?}", data);
    }

    #[test]
    fn test_opcode_size() {
        assert_eq!(size_of::<Opcode>(), 8);
    }
}
