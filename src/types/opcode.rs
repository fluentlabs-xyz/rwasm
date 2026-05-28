use crate::{
    types::{
        AddressOffset, BlockFuel, BranchOffset, BranchTableTargets, CompiledFunc, DataSegmentIdx,
        ElementSegmentIdx, GlobalIdx, LocalDepth, SignatureIdx, TableIdx, UntypedValue,
    },
    MaxStackHeight, NumLocals, SysFuncIdx, TrapCode,
};
use alloc::{format, vec::Vec};
use bincode::{Decode, Encode};

macro_rules! define_opcode_enum {
    (
        $(
            $(#[$meta:meta])*
            $variant:ident $(($($field:ty),+ $(,)?))? => $code:expr,
        )*
    ) => {
        #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[repr(u16)]
        pub enum Opcode {
            $(
                $(#[$meta])*
                $variant $(($($field),+))?,
            )*
        }

        impl Opcode {
            pub fn code(&self) -> u32 {
                match self {
                    $(
                        $(#[$meta])*
                        Self::$variant $( ( $(define_opcode_enum!(@ignore $field)),+ ) )? => $code,
                    )*
                }
            }
        }
    };
    (@ignore $field:ty) => {
        _
    };
}

define_opcode_enum! {
    // stack/system
    Unreachable => 0u32,
    Trap(TrapCode) => 1u32,
    LocalGet(LocalDepth) => 2u32,
    LocalSet(LocalDepth) => 3u32,
    LocalTee(LocalDepth) => 4u32,
    Br(BranchOffset) => 5u32,
    BrIfEqz(BranchOffset) => 6u32,
    BrIfNez(BranchOffset) => 7u32,
    BrTable(BranchTableTargets) => 8u32,
    ConsumeFuel(BlockFuel) => 9u32,
    ConsumeFuelStack => 10u32,
    Return => 11u32,
    ReturnCallInternal(CompiledFunc) => 12u32,
    ReturnCall(SysFuncIdx) => 13u32,
    ReturnCallIndirect(SignatureIdx) => 14u32,
    CallInternal(CompiledFunc) => 15u32,
    Call(SysFuncIdx) => 16u32,
    CallIndirect(SignatureIdx) => 17u32,
    SignatureCheck(SignatureIdx) => 18u32,
    StackCheck(MaxStackHeight) => 19u32,
    RefFunc(CompiledFunc) => 20u32,
    I32Const(UntypedValue) => 21u32,
    Drop => 22u32,
    Select => 23u32,
    GlobalGet(GlobalIdx) => 24u32,
    GlobalSet(GlobalIdx) => 25u32,

    // memory
    I32Load(AddressOffset) => 26u32,
    I32Load8S(AddressOffset) => 27u32,
    I32Load8U(AddressOffset) => 28u32,
    I32Load16S(AddressOffset) => 29u32,
    I32Load16U(AddressOffset) => 30u32,
    I32Store(AddressOffset) => 31u32,
    I32Store8(AddressOffset) => 32u32,
    I32Store16(AddressOffset) => 33u32,
    MemorySize => 34u32,
    MemoryGrow => 35u32,
    MemoryFill => 36u32,
    MemoryCopy => 37u32,
    MemoryInit(DataSegmentIdx) => 38u32,
    DataDrop(DataSegmentIdx) => 39u32,

    // table
    TableSize(TableIdx) => 40u32,
    TableGrow(TableIdx) => 41u32,
    TableFill(TableIdx) => 42u32,
    TableGet(TableIdx) => 43u32,
    TableSet(TableIdx) => 44u32,
    TableCopy(TableIdx, TableIdx) => 45u32,
    TableInit(ElementSegmentIdx) => 46u32,
    ElemDrop(ElementSegmentIdx) => 47u32,

    // alu
    I32Eqz => 48u32,
    I32Eq => 49u32,
    I32Ne => 50u32,
    I32LtS => 51u32,
    I32LtU => 52u32,
    I32GtS => 53u32,
    I32GtU => 54u32,
    I32LeS => 55u32,
    I32LeU => 56u32,
    I32GeS => 57u32,
    I32GeU => 58u32,
    I32Clz => 59u32,
    I32Ctz => 60u32,
    I32Popcnt => 61u32,
    I32Add => 62u32,
    I32Sub => 63u32,
    I32Mul => 64u32,
    I32DivS => 65u32,
    I32DivU => 66u32,
    I32RemS => 67u32,
    I32RemU => 68u32,
    I32And => 69u32,
    I32Or => 70u32,
    I32Xor => 71u32,
    I32Shl => 72u32,
    I32ShrS => 73u32,
    I32ShrU => 74u32,
    I32Rotl => 75u32,
    I32Rotr => 76u32,
    I32WrapI64 => 77u32,
    I32Extend8S => 78u32,
    I32Extend16S => 79u32,
    I32Mul64 => 80u32,
    I32Add64 => 81u32,
    BulkConst(NumLocals) => 82u32,
    BulkDrop(NumLocals) => 83u32,

    // fpu
    #[cfg(feature = "fpu")]
    F32Load(AddressOffset) => 84u32,
    #[cfg(feature = "fpu")]
    F64Load(AddressOffset) => 85u32,
    #[cfg(feature = "fpu")]
    F32Store(AddressOffset) => 86u32,
    #[cfg(feature = "fpu")]
    F64Store(AddressOffset) => 87u32,
    #[cfg(feature = "fpu")]
    F32Eq => 88u32,
    #[cfg(feature = "fpu")]
    F32Ne => 89u32,
    #[cfg(feature = "fpu")]
    F32Lt => 90u32,
    #[cfg(feature = "fpu")]
    F32Gt => 91u32,
    #[cfg(feature = "fpu")]
    F32Le => 92u32,
    #[cfg(feature = "fpu")]
    F32Ge => 93u32,
    #[cfg(feature = "fpu")]
    F64Eq => 94u32,
    #[cfg(feature = "fpu")]
    F64Ne => 95u32,
    #[cfg(feature = "fpu")]
    F64Lt => 96u32,
    #[cfg(feature = "fpu")]
    F64Gt => 97u32,
    #[cfg(feature = "fpu")]
    F64Le => 98u32,
    #[cfg(feature = "fpu")]
    F64Ge => 99u32,
    #[cfg(feature = "fpu")]
    F32Abs => 100u32,
    #[cfg(feature = "fpu")]
    F32Neg => 101u32,
    #[cfg(feature = "fpu")]
    F32Ceil => 102u32,
    #[cfg(feature = "fpu")]
    F32Floor => 103u32,
    #[cfg(feature = "fpu")]
    F32Trunc => 104u32,
    #[cfg(feature = "fpu")]
    F32Nearest => 105u32,
    #[cfg(feature = "fpu")]
    F32Sqrt => 106u32,
    #[cfg(feature = "fpu")]
    F32Add => 107u32,
    #[cfg(feature = "fpu")]
    F32Sub => 108u32,
    #[cfg(feature = "fpu")]
    F32Mul => 109u32,
    #[cfg(feature = "fpu")]
    F32Div => 110u32,
    #[cfg(feature = "fpu")]
    F32Min => 111u32,
    #[cfg(feature = "fpu")]
    F32Max => 112u32,
    #[cfg(feature = "fpu")]
    F32Copysign => 113u32,
    #[cfg(feature = "fpu")]
    F64Abs => 114u32,
    #[cfg(feature = "fpu")]
    F64Neg => 115u32,
    #[cfg(feature = "fpu")]
    F64Ceil => 116u32,
    #[cfg(feature = "fpu")]
    F64Floor => 117u32,
    #[cfg(feature = "fpu")]
    F64Trunc => 118u32,
    #[cfg(feature = "fpu")]
    F64Nearest => 119u32,
    #[cfg(feature = "fpu")]
    F64Sqrt => 120u32,
    #[cfg(feature = "fpu")]
    F64Add => 121u32,
    #[cfg(feature = "fpu")]
    F64Sub => 122u32,
    #[cfg(feature = "fpu")]
    F64Mul => 123u32,
    #[cfg(feature = "fpu")]
    F64Div => 124u32,
    #[cfg(feature = "fpu")]
    F64Min => 125u32,
    #[cfg(feature = "fpu")]
    F64Max => 126u32,
    #[cfg(feature = "fpu")]
    F64Copysign => 127u32,
    #[cfg(feature = "fpu")]
    I32TruncF32S => 128u32,
    #[cfg(feature = "fpu")]
    I32TruncF32U => 129u32,
    #[cfg(feature = "fpu")]
    I32TruncF64S => 130u32,
    #[cfg(feature = "fpu")]
    I32TruncF64U => 131u32,
    #[cfg(feature = "fpu")]
    I64TruncF32S => 132u32,
    #[cfg(feature = "fpu")]
    I64TruncF32U => 133u32,
    #[cfg(feature = "fpu")]
    I64TruncF64S => 134u32,
    #[cfg(feature = "fpu")]
    I64TruncF64U => 135u32,
    #[cfg(feature = "fpu")]
    F32ConvertI32S => 136u32,
    #[cfg(feature = "fpu")]
    F32ConvertI32U => 137u32,
    #[cfg(feature = "fpu")]
    F32ConvertI64S => 138u32,
    #[cfg(feature = "fpu")]
    F32ConvertI64U => 139u32,
    #[cfg(feature = "fpu")]
    F32DemoteF64 => 140u32,
    #[cfg(feature = "fpu")]
    F64ConvertI32S => 141u32,
    #[cfg(feature = "fpu")]
    F64ConvertI32U => 142u32,
    #[cfg(feature = "fpu")]
    F64ConvertI64S => 143u32,
    #[cfg(feature = "fpu")]
    F64ConvertI64U => 144u32,
    #[cfg(feature = "fpu")]
    F64PromoteF32 => 145u32,
    #[cfg(feature = "fpu")]
    I32TruncSatF32S => 146u32,
    #[cfg(feature = "fpu")]
    I32TruncSatF32U => 147u32,
    #[cfg(feature = "fpu")]
    I32TruncSatF64S => 148u32,
    #[cfg(feature = "fpu")]
    I32TruncSatF64U => 149u32,
    #[cfg(feature = "fpu")]
    I64TruncSatF32S => 150u32,
    #[cfg(feature = "fpu")]
    I64TruncSatF32U => 151u32,
    #[cfg(feature = "fpu")]
    I64TruncSatF64S => 152u32,
    #[cfg(feature = "fpu")]
    I64TruncSatF64U => 153u32,
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
        matches!(
            self,
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
        )
    }

    pub fn is_memory_instruction(self) -> bool {
        matches!(
            self,
            Opcode::I32Load8S(_)
                | Opcode::I32Load8U(_)
                | Opcode::I32Load16S(_)
                | Opcode::I32Load16U(_)
                | Opcode::I32Load(_)
                | Opcode::I32Store8(_)
                | Opcode::I32Store16(_)
                | Opcode::I32Store(_)
        )
    }

    pub fn is_memory_load_instruction(self) -> bool {
        matches!(
            self,
            Opcode::I32Load8S(_)
                | Opcode::I32Load8U(_)
                | Opcode::I32Load16S(_)
                | Opcode::I32Load16U(_)
                | Opcode::I32Load(_)
        )
    }

    pub fn is_memory_store_instruction(self) -> bool {
        matches!(
            self,
            Opcode::I32Store8(_) | Opcode::I32Store16(_) | Opcode::I32Store(_)
        )
    }

    pub fn is_ecall_instruction(self) -> bool {
        matches!(self, Opcode::Call(_) | Opcode::ReturnCall(_))
    }

    pub fn is_branch_instruction(self) -> bool {
        matches!(
            self,
            Opcode::Br(_) | Opcode::BrIfEqz(_) | Opcode::BrIfNez(_)
        )
    }

    pub fn is_jump_instruction(self) -> bool {
        false
    }

    pub fn is_halt(self) -> bool {
        false
    }

    pub fn is_unary_instruction(self) -> bool {
        matches!(
            self,
            Opcode::I32Clz | Opcode::I32Ctz | Opcode::I32Popcnt | Opcode::I32Eqz
        )
    }

    pub fn is_binary_instruction(self) -> bool {
        matches!(
            self,
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
                | Opcode::I32Rotr
        )
    }

    pub fn is_nullary(&self) -> bool {
        matches!(self, Opcode::Br(_) | Opcode::I32Const(_))
    }

    pub fn is_call_instruction(self) -> bool {
        matches!(
            self,
            Opcode::CallIndirect(_)
                | Opcode::CallInternal(_)
                | Opcode::ReturnCallIndirect(_)
                | Opcode::ReturnCallInternal(_)
                | Opcode::Return
        )
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

    #[test]
    fn test_opcode_code_values() {
        let opcodes = [
            Opcode::Unreachable,
            Opcode::Trap(TrapCode::UnreachableCodeReached),
            Opcode::LocalGet(7),
            Opcode::LocalSet(7),
            Opcode::LocalTee(7),
            Opcode::Br(0.into()),
            Opcode::BrIfEqz(0.into()),
            Opcode::BrIfNez(0.into()),
            Opcode::BrTable(0),
            Opcode::ConsumeFuel(0),
            Opcode::ConsumeFuelStack,
            Opcode::Return,
            Opcode::ReturnCallInternal(0),
            Opcode::ReturnCall(0),
            Opcode::ReturnCallIndirect(0),
            Opcode::CallInternal(0),
            Opcode::Call(0),
            Opcode::CallIndirect(0),
            Opcode::SignatureCheck(0),
            Opcode::StackCheck(0),
            Opcode::RefFunc(0),
            Opcode::I32Const(42.into()),
            Opcode::Drop,
            Opcode::Select,
            Opcode::GlobalGet(0),
            Opcode::GlobalSet(0),
            Opcode::I32Load(0),
            Opcode::I32Load8S(0),
            Opcode::I32Load8U(0),
            Opcode::I32Load16S(0),
            Opcode::I32Load16U(0),
            Opcode::I32Store(0),
            Opcode::I32Store8(0),
            Opcode::I32Store16(0),
            Opcode::MemorySize,
            Opcode::MemoryGrow,
            Opcode::MemoryFill,
            Opcode::MemoryCopy,
            Opcode::MemoryInit(0),
            Opcode::DataDrop(0),
            Opcode::TableSize(0),
            Opcode::TableGrow(0),
            Opcode::TableFill(0),
            Opcode::TableGet(0),
            Opcode::TableSet(0),
            Opcode::TableCopy(0, 1),
            Opcode::TableInit(0),
            Opcode::ElemDrop(0),
            Opcode::I32Eqz,
            Opcode::I32Eq,
            Opcode::I32Ne,
            Opcode::I32LtS,
            Opcode::I32LtU,
            Opcode::I32GtS,
            Opcode::I32GtU,
            Opcode::I32LeS,
            Opcode::I32LeU,
            Opcode::I32GeS,
            Opcode::I32GeU,
            Opcode::I32Clz,
            Opcode::I32Ctz,
            Opcode::I32Popcnt,
            Opcode::I32Add,
            Opcode::I32Sub,
            Opcode::I32Mul,
            Opcode::I32DivS,
            Opcode::I32DivU,
            Opcode::I32RemS,
            Opcode::I32RemU,
            Opcode::I32And,
            Opcode::I32Or,
            Opcode::I32Xor,
            Opcode::I32Shl,
            Opcode::I32ShrS,
            Opcode::I32ShrU,
            Opcode::I32Rotl,
            Opcode::I32Rotr,
            Opcode::I32WrapI64,
            Opcode::I32Extend8S,
            Opcode::I32Extend16S,
            Opcode::I32Mul64,
            Opcode::I32Add64,
            Opcode::BulkConst(3),
            Opcode::BulkDrop(3),
        ];
        for (expected, opcode) in opcodes.iter().enumerate() {
            assert_eq!(opcode.code(), expected as u32, "{opcode:#}");
        }
    }
}
