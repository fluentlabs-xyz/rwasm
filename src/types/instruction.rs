use crate::types::{
    AddressOffset,
    BlockFuel,
    BranchOffset,
    BranchTableTargets,
    CompiledFunc,
    DataSegmentIdx,
    DropKeep,
    ElementSegmentIdx,
    FuncIdx,
    GlobalIdx,
    LocalDepth,
    SignatureIdx,
    StackAlloc,
    TableIdx,
    UntypedValue,
};
use alloc::{format, vec::Vec};
use core::fmt::{Display, Formatter};

#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    num_enum::IntoPrimitive,
    num_enum::TryFromPrimitive,
)]
#[repr(u8)]
pub enum Opcode {
    // system
    MemorySize = 0x2f,
    MemoryGrow = 0x30,
    MemoryFill = 0x31,
    MemoryCopy = 0x32,
    MemoryInit = 0x33,
    DataDrop = 0x34,
    TableSize = 0x35,
    TableGrow = 0x36,
    TableFill = 0x37,
    TableGet = 0x38,
    TableSet = 0x39,
    TableCopy = 0x3a,
    TableInit = 0x3b,
    ElemDrop = 0x3c,
    // stack
    LocalGet = 0x01,
    LocalSet = 0x02,
    LocalTee = 0x03,
    Drop = 0x14,
    Select = 0x15,
    RefFunc = 0x3d,
    I32Const = 0x3e,
    // I64Const = 0x3f,
    GlobalGet = 0x16,
    GlobalSet = 0x17,
    // control flow
    Unreachable = 0x00,
    ConsumeFuel = 0x0a,
    SignatureCheck = 0x13,
    StackAlloc = 0xc6,
    Br = 0x04,
    BrIfEqz = 0x05,
    BrIfNez = 0x06,
    BrAdjust = 0x07,
    BrAdjustIfNez = 0x08,
    BrTable = 0x09,
    Return = 0x0b,
    ReturnIfNez = 0x0c,
    ReturnCallInternal = 0x0d,
    ReturnCall = 0x0e,
    ReturnCallIndirect = 0x0f,
    CallInternal = 0x10,
    Call = 0x11,
    CallIndirect = 0x12,
    // memory load
    I32Load = 0x18,
    // I64Load = 0x19,
    I32Load8S = 0x1c,
    I32Load8U = 0x1d,
    I32Load16S = 0x1e,
    I32Load16U = 0x1f,
    // I64Load8S = 0x20,
    // I64Load8U = 0x21,
    // I64Load16S = 0x22,
    // I64Load16U = 0x23,
    // I64Load32S = 0x24,
    // I64Load32U = 0x25,
    // memory store
    I32Store = 0x26,
    // I64Store = 0x27,
    I32Store8 = 0x2a,
    I32Store16 = 0x2b,
    // I64Store8 = 0x2c,
    // I64Store16 = 0x2d,
    // I64Store32 = 0x2e,
    // cmp
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
    // I64Eqz = 0x4d,
    // I64Eq = 0x4e,
    // I64Ne = 0x4f,
    // I64LtS = 0x50,
    // I64LtU = 0x51,
    // I64GtS = 0x52,
    // I64GtU = 0x53,
    // I64LeS = 0x54,
    // I64LeU = 0x55,
    // I64GeS = 0x56,
    // I64GeU = 0x57,
    // bitwise
    I32Clz = 0x64,
    I32Ctz = 0x65,
    I32Popcnt = 0x66,
    I32And = 0x6e,
    I32Or = 0x6f,
    I32Xor = 0x70,
    I32Shl = 0x71,
    I32ShrS = 0x72,
    I32ShrU = 0x73,
    I32Rotl = 0x74,
    I32Rotr = 0x75,
    // I64Clz = 0x76,
    // I64Ctz = 0x77,
    // I64Popcnt = 0x78,
    // I64And = 0x80,
    // I64Or = 0x81,
    // I64Xor = 0x82,
    // I64Shl = 0x83,
    // I64ShrS = 0x84,
    // I64ShrU = 0x85,
    // I64Rotl = 0x86,
    // I64Rotr = 0x87,
    // alu
    I32Add = 0x67,
    I32Sub = 0x68,
    I32Mul = 0x69,
    I32DivS = 0x6a,
    I32DivU = 0x6b,
    I32RemS = 0x6c,
    I32RemU = 0x6d,
    // I64Add = 0x79,
    // I64Sub = 0x7a,
    // I64Mul = 0x7b,
    // I64DivS = 0x7c,
    // I64DivU = 0x7d,
    // I64RemS = 0x7e,
    // I64RemU = 0x7f,
    // convert
    // I64ExtendI32S = 0xa9,
    // I64ExtendI32U = 0xaa,
    I32Extend8S = 0xb9,
    I32Extend16S = 0xba,
    // I64Extend8S = 0xbb,
    // I64Extend16S = 0xbc,
    // I64Extend32S = 0xbd,
    // float
    // F32Const = 0x40,
    // F64Const = 0x41,
    // F32Load = 0x1a,
    // F64Load = 0x1b,
    // F32Store = 0x28,
    // F64Store = 0x29,
    // F32Eq = 0x58,
    // F32Ne = 0x59,
    // F32Lt = 0x5a,
    // F32Gt = 0x5b,
    // F32Le = 0x5c,
    // F32Ge = 0x5d,
    // F64Eq = 0x5e,
    // F64Ne = 0x5f,
    // F64Lt = 0x60,
    // F64Gt = 0x61,
    // F64Le = 0x62,
    // F64Ge = 0x63,
    // F32Abs = 0x88,
    // F32Neg = 0x89,
    // F32Ceil = 0x8a,
    // F32Floor = 0x8b,
    // F32Trunc = 0x8c,
    // F32Nearest = 0x8d,
    // F32Sqrt = 0x8e,
    // F32Add = 0x8f,
    // F32Sub = 0x90,
    // F32Mul = 0x91,
    // F32Div = 0x92,
    // F32Min = 0x93,
    // F32Max = 0x94,
    // F32Copysign = 0x95,
    // F64Abs = 0x96,
    // F64Neg = 0x97,
    // F64Ceil = 0x98,
    // F64Floor = 0x99,
    // F64Trunc = 0x9a,
    // F64Nearest = 0x9b,
    // F64Sqrt = 0x9c,
    // F64Add = 0x9d,
    // F64Sub = 0x9e,
    // F64Mul = 0x9f,
    // F64Div = 0xa0,
    // F64Min = 0xa1,
    // F64Max = 0xa2,
    // F64Copysign = 0xa3,
    // I32WrapI64 = 0xa4,
    // I32TruncF32S = 0xa5,
    // I32TruncF32U = 0xa6,
    // I32TruncF64S = 0xa7,
    // I32TruncF64U = 0xa8,
    // I64TruncF32S = 0xab,
    // I64TruncF32U = 0xac,
    // I64TruncF64S = 0xad,
    // I64TruncF64U = 0xae,
    // F32ConvertI32S = 0xaf,
    // F32ConvertI32U = 0xb0,
    // F32ConvertI64S = 0xb1,
    // F32ConvertI64U = 0xb2,
    // F32DemoteF64 = 0xb3,
    // F64ConvertI32S = 0xb4,
    // F64ConvertI32U = 0xb5,
    // F64ConvertI64S = 0xb6,
    // F64ConvertI64U = 0xb7,
    // F64PromoteF32 = 0xb8,
    I32TruncSatF32S = 0xbe,
    I32TruncSatF32U = 0xbf,
    I32TruncSatF64S = 0xc0,
    I32TruncSatF64U = 0xc1,
    // I64TruncSatF32S = 0xc2,
    // I64TruncSatF32U = 0xc3,
    // I64TruncSatF64S = 0xc4,
    // I64TruncSatF64U = 0xc5,
}

impl Opcode {
    #[inline(always)]
    pub fn is_system_opcode(&self) -> bool {
        use Opcode::*;
        matches!(
            self,
            MemorySize
                | MemoryGrow
                | MemoryFill
                | MemoryCopy
                | MemoryInit
                | DataDrop
                | TableSize
                | TableGrow
                | TableFill
                | TableGet
                | TableSet
                | TableCopy
                | TableInit
                | ElemDrop
        )
    }

    #[inline(always)]
    pub fn is_stack_opcode(&self) -> bool {
        use Opcode::*;
        matches!(
            self,
            LocalGet
                | LocalSet
                | LocalTee
                | Drop
                | Select
                | RefFunc
                | I32Const
                // | I64Const
                | GlobalGet
                | GlobalSet
        )
    }

    #[inline(always)]
    pub fn is_control_flow_opcode(&self) -> bool {
        use Opcode::*;
        matches!(
            self,
            Unreachable
                | ConsumeFuel
                | SignatureCheck
                | StackAlloc
                | Br
                | BrIfEqz
                | BrIfNez
                | BrAdjust
                | BrAdjustIfNez
                | BrTable
                | Return
                | ReturnIfNez
                | ReturnCallInternal
                | ReturnCall
                | ReturnCallIndirect
                | CallInternal
                | Call
                | CallIndirect
        )
    }

    #[inline(always)]
    pub fn is_memory_load_opcode(&self) -> bool {
        use Opcode::*;
        matches!(
            self,
            I32Load
                // | I64Load
                | I32Load8S
                | I32Load8U
                | I32Load16S
                | I32Load16U /* | I64Load8S
                              * | I64Load8U
                              * | I64Load16S | I64Load16U
                              *               | I64Load32S
                              *               | I64Load32U */
        )
    }

    #[inline(always)]
    pub fn is_memory_store_opcode(&self) -> bool {
        use Opcode::*;
        matches!(
            self,
            I32Store
            // | I64Store
            | I32Store8
            | I32Store16 /* | I64Store8
                          * | I64Store16
                          * | I64Store32 */
        )
    }

    #[inline(always)]
    pub fn is_compare_binary_opcode(&self) -> bool {
        use Opcode::*;
        matches!(
            self,
            I32LtS
                | I32LtU
                | I32GtS
                | I32GtU
                | I32LeS
                | I32LeU
                | I32GeS
                | I32GeU
                // | I64LtS
                // | I64LtU
                // | I64GtS
                // | I64GtU
                // | I64LeS
                // | I64LeU
                // | I64GeS
                // | I64GeU
                | I32Eq
                | I32Ne /* | I64Eq
                         * | I64Ne */
        )
    }

    #[inline(always)]
    pub fn is_compare_unary_opcode(&self) -> bool {
        use Opcode::*;
        matches!(self, I32Eqz
            // | I64Eqz
        )
    }

    #[inline(always)]
    pub fn is_bitwise_unary_opcode(&self) -> bool {
        use Opcode::*;
        matches!(
            self,
            I32Clz | I32Ctz | I32Popcnt //| I64Clz | I64Ctz | I64Popcnt
        )
    }

    #[inline(always)]
    pub fn is_bitwise_binary_opcode(&self) -> bool {
        use Opcode::*;
        matches!(
            self,
            I32And | I32Or | I32Xor | I32Shl | I32ShrS | I32ShrU | I32Rotl | I32Rotr /* | I64And
                                                                                      * | I64Or
                                                                                      * | I64Xor
                                                                                      * | I64Shl
                                                                                      * | I64ShrS
                                                                                      * | I64ShrU
                                                                                      * | I64Rotl
                                                                                      * | I64Rotr */
        )
    }

    #[inline(always)]
    pub fn is_arith_unsigned_opcode(&self) -> bool {
        use Opcode::*;
        matches!(
            self,
            I32Add | I32Sub | I32Mul // | I64Add | I64Sub | I64Mul
        )
    }

    #[inline(always)]
    pub fn is_arith_signed_opcode(&self) -> bool {
        use Opcode::*;
        matches!(
            self,
            I32DivS | I32DivU | I32RemS | I32RemU // | I64DivS | I64DivU | I64RemS | I64RemU
        )
    }

    #[inline(always)]
    pub fn is_convert_opcode(&self) -> bool {
        use Opcode::*;
        matches!(
            self,
            // I32WrapI64
            // | I64ExtendI32S
            // | I64ExtendI32U
            I32Extend8S | I32Extend16S /* | I64Extend8S
                                        * | I64Extend16S
                                        * | I64Extend32S */
        )
    }

    #[inline(always)]
    pub fn is_float_opcode(&self) -> bool {
        false
        // use Opcode::*;
        // matches!(
        //     self,
        /* | F32Const
         * | F64Const
         * | F32Load
         * | F64Load
         * | F32Store
         * | F64Store
         * | F32Eq
         * | F32Ne
         * | F32Lt
         * | F32Gt
         * | F32Le
         * | F32Ge
         * | F64Eq
         * | F64Ne
         * | F64Lt
         * | F64Gt
         * | F64Le
         * | F64Ge
         * | F32Abs
         * | F32Neg
         * | F32Ceil
         * | F32Floor
         * | F32Trunc
         * | F32Nearest
         * | F32Sqrt
         * | F32Add
         * | F32Sub
         * | F32Mul
         * | F32Div
         * | F32Min
         * | F32Max
         * | F32Copysign
         * | F64Abs
         * | F64Neg
         * | F64Ceil
         * | F64Floor
         * | F64Trunc
         * | F64Nearest
         * | F64Sqrt
         * | F64Add
         * | F64Sub
         * | F64Mul
         * | F64Div
         * | F64Min
         * | F64Max
         * | F64Copysign
         * | I32TruncF32S
         * | I32TruncF32U
         * | I64TruncF32S
         * | I64TruncF32U
         * | F32ConvertI32S
         * | F32ConvertI32U
         * | F32ConvertI64S
         * | F32ConvertI64U
         * | F32DemoteF64
         * | F64PromoteF32
         * | I32TruncSatF32S
         * | I32TruncSatF32U
         * | I64TruncSatF32S
         * | I64TruncSatF32U
         * | I32TruncF64S
         * | I32TruncF64U
         * | I64TruncF64S
         * | I64TruncF64U
         * | F64ConvertI32S
         * | F64ConvertI32U
         * | F64ConvertI64S
         * | F64ConvertI64U
         * | I32TruncSatF64S
         * | I32TruncSatF64U
         * | I64TruncSatF64S
         * | I64TruncSatF64U */
        // )
    }
}

impl Display for Opcode {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let name = format!("{:?}", self);
        let name: Vec<_> = name.split('(').collect();
        write!(f, "{}", name[0])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub enum Instruction {
    EmptyData(Opcode),
    LocalDepth(Opcode, LocalDepth),
    BranchOffset(Opcode, BranchOffset),
    BranchTableTargets(Opcode, BranchTableTargets),
    BlockFuel(Opcode, BlockFuel),
    DropKeep(Opcode, DropKeep),
    CompiledFunc(Opcode, CompiledFunc),
    FuncIdx(Opcode, FuncIdx),
    SignatureIdx(Opcode, SignatureIdx),
    GlobalIdx(Opcode, GlobalIdx),
    AddressOffset(Opcode, AddressOffset),
    DataSegmentIdx(Opcode, DataSegmentIdx),
    TableIdx(Opcode, TableIdx),
    ElementSegmentIdx(Opcode, ElementSegmentIdx),
    UntypedValue(Opcode, UntypedValue),
    StackAlloc(Opcode, StackAlloc),
}

#[test]
fn test_opcode_data_size() {
    assert_eq!(size_of::<Instruction>(), 8);
}

impl Instruction {
    pub fn opcode(&self) -> Opcode {
        match self {
            Instruction::EmptyData(opcode) => *opcode,
            Instruction::LocalDepth(opcode, _) => *opcode,
            Instruction::BranchOffset(opcode, _) => *opcode,
            Instruction::BranchTableTargets(opcode, _) => *opcode,
            Instruction::BlockFuel(opcode, _) => *opcode,
            Instruction::DropKeep(opcode, _) => *opcode,
            Instruction::CompiledFunc(opcode, _) => *opcode,
            Instruction::FuncIdx(opcode, _) => *opcode,
            Instruction::SignatureIdx(opcode, _) => *opcode,
            Instruction::GlobalIdx(opcode, _) => *opcode,
            Instruction::AddressOffset(opcode, _) => *opcode,
            Instruction::DataSegmentIdx(opcode, _) => *opcode,
            Instruction::TableIdx(opcode, _) => *opcode,
            Instruction::ElementSegmentIdx(opcode, _) => *opcode,
            Instruction::UntypedValue(opcode, _) => *opcode,
            Instruction::StackAlloc(opcode, _) => *opcode,
        }
    }

    pub fn update_branch_offset<I: Into<BranchOffset>>(&mut self, offset: I) {
        if let Instruction::BranchOffset(_, old_offset) = self {
            *old_offset = offset.into();
        } else {
            unreachable!("rwasm: opcode data is not a branch offset")
        }
    }
}

#[derive(Default, Debug, PartialEq, Clone, Eq, Hash)]
pub struct OpcodeMeta {
    pub index: usize,
    pub pos: usize,
    pub opcode: u8,
}
