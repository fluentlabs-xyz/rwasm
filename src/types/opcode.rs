use crate::{
    types::{
        AddressOffset, BlockFuel, BranchOffset, BranchTableTargets, CompiledFunc, DataSegmentIdx,
        ElementSegmentIdx, GlobalIdx, LocalDepth, SignatureIdx, TableIdx, UntypedValue,
    },
    MaxStackHeight, NumLocals, SysFuncIdx, TrapCode,
};
use alloc::{format, vec::Vec};
use bincode::{Decode, Encode};

const FPU_OPCODE_OFFSET: u32 = 1000;

macro_rules! define_opcode_enum {
    (
        $(
            $(#[$meta:meta])*
            $(@ $kind:ident)?
            $variant:ident $(($($field:ident : $field_ty:ty),+ $(,)?))? => $code:expr,
        )*
    ) => {
        #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[repr(u16)]
        pub enum Opcode {
            $(
                $(#[$meta])*
                $variant $(($($field_ty),+))?,
            )*
        }

        impl Opcode {
            pub fn code(&self) -> u32 {
                match self {
                    $(
                        $(#[$meta])*
                        Self::$variant $( ( $(define_opcode_enum!(@ignore $field)),+ ) )? => {
                            define_opcode_enum!(@code $($kind)? $code)
                        }
                    )*
                }
            }
        }

        impl Encode for Opcode {
            fn encode<__E: bincode::enc::Encoder>(
                &self,
                encoder: &mut __E,
            ) -> Result<(), bincode::error::EncodeError> {
                match self {
                    $(
                        $(#[$meta])*
                        Self::$variant $( ( $($field),+ ) )? => {
                            Encode::encode(&define_opcode_enum!(@code $($kind)? $code), encoder)?;
                            $(
                                $(
                                    Encode::encode($field, encoder)?;
                                )+
                            )?
                            Ok(())
                        }
                    )*
                }
            }
        }

        impl<__Context> Decode<__Context> for Opcode {
            fn decode<__D: bincode::de::Decoder<Context = __Context>>(
                decoder: &mut __D,
            ) -> Result<Self, bincode::error::DecodeError> {
                let code = u32::decode(decoder)?;
                match code {
                    $(
                        $(#[$meta])*
                        code if code == define_opcode_enum!(@code $($kind)? $code) => {
                            Ok(Self::$variant $( ( $(<$field_ty as Decode<__Context>>::decode(decoder)?),+ ) )?)
                        }
                    )*
                    _ => Err(bincode::error::DecodeError::Other("rwasm: invalid opcode")),
                }
            }
        }
    };
    (@ignore $field:ident) => {
        _
    };
    (@code fpu $code:expr) => {
        FPU_OPCODE_OFFSET + $code
    };
    (@code $code:expr) => {
        $code
    };
}

define_opcode_enum! {
    // stack/system
    Unreachable => 0u32,
    Trap(trap_code: TrapCode) => 1u32,
    LocalGet(depth: LocalDepth) => 2u32,
    LocalSet(depth: LocalDepth) => 3u32,
    LocalTee(depth: LocalDepth) => 4u32,
    Br(offset: BranchOffset) => 5u32,
    BrIfEqz(offset: BranchOffset) => 6u32,
    BrIfNez(offset: BranchOffset) => 7u32,
    BrTable(targets: BranchTableTargets) => 8u32,
    ConsumeFuel(fuel: BlockFuel) => 9u32,
    ConsumeFuelStack => 10u32,
    Return => 11u32,
    ReturnCallInternal(func: CompiledFunc) => 12u32,
    ReturnCall(func: SysFuncIdx) => 13u32,
    ReturnCallIndirect(signature: SignatureIdx) => 14u32,
    CallInternal(func: CompiledFunc) => 15u32,
    Call(func: SysFuncIdx) => 16u32,
    CallIndirect(signature: SignatureIdx) => 17u32,
    SignatureCheck(signature: SignatureIdx) => 18u32,
    StackCheck(height: MaxStackHeight) => 19u32,
    RefFunc(func: CompiledFunc) => 20u32,
    I32Const(value: UntypedValue) => 21u32,
    Drop => 22u32,
    Select => 23u32,
    GlobalGet(global: GlobalIdx) => 24u32,
    GlobalSet(global: GlobalIdx) => 25u32,

    // memory
    I32Load(offset: AddressOffset) => 26u32,
    I32Load8S(offset: AddressOffset) => 27u32,
    I32Load8U(offset: AddressOffset) => 28u32,
    I32Load16S(offset: AddressOffset) => 29u32,
    I32Load16U(offset: AddressOffset) => 30u32,
    I32Store(offset: AddressOffset) => 31u32,
    I32Store8(offset: AddressOffset) => 32u32,
    I32Store16(offset: AddressOffset) => 33u32,
    MemorySize => 34u32,
    MemoryGrow => 35u32,
    MemoryFill => 36u32,
    MemoryCopy => 37u32,
    MemoryInit(segment: DataSegmentIdx) => 38u32,
    DataDrop(segment: DataSegmentIdx) => 39u32,

    // table
    TableSize(table: TableIdx) => 40u32,
    TableGrow(table: TableIdx) => 41u32,
    TableFill(table: TableIdx) => 42u32,
    TableGet(table: TableIdx) => 43u32,
    TableSet(table: TableIdx) => 44u32,
    TableCopy(dst: TableIdx, src: TableIdx) => 45u32,
    TableInit(segment: ElementSegmentIdx) => 46u32,
    ElemDrop(segment: ElementSegmentIdx) => 47u32,

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
    BulkConst(locals: NumLocals) => 82u32,
    BulkDrop(locals: NumLocals) => 83u32,

    // fpu
    @fpu F32Load(offset: AddressOffset) => 0u32,
    @fpu F64Load(offset: AddressOffset) => 1u32,
    @fpu F32Store(offset: AddressOffset) => 2u32,
    @fpu F64Store(offset: AddressOffset) => 3u32,
    @fpu F32Eq => 4u32,
    @fpu F32Ne => 5u32,
    @fpu F32Lt => 6u32,
    @fpu F32Gt => 7u32,
    @fpu F32Le => 8u32,
    @fpu F32Ge => 9u32,
    @fpu F64Eq => 10u32,
    @fpu F64Ne => 11u32,
    @fpu F64Lt => 12u32,
    @fpu F64Gt => 13u32,
    @fpu F64Le => 14u32,
    @fpu F64Ge => 15u32,
    @fpu F32Abs => 16u32,
    @fpu F32Neg => 17u32,
    @fpu F32Ceil => 18u32,
    @fpu F32Floor => 19u32,
    @fpu F32Trunc => 20u32,
    @fpu F32Nearest => 21u32,
    @fpu F32Sqrt => 22u32,
    @fpu F32Add => 23u32,
    @fpu F32Sub => 24u32,
    @fpu F32Mul => 25u32,
    @fpu F32Div => 26u32,
    @fpu F32Min => 27u32,
    @fpu F32Max => 28u32,
    @fpu F32Copysign => 29u32,
    @fpu F64Abs => 30u32,
    @fpu F64Neg => 31u32,
    @fpu F64Ceil => 32u32,
    @fpu F64Floor => 33u32,
    @fpu F64Trunc => 34u32,
    @fpu F64Nearest => 35u32,
    @fpu F64Sqrt => 36u32,
    @fpu F64Add => 37u32,
    @fpu F64Sub => 38u32,
    @fpu F64Mul => 39u32,
    @fpu F64Div => 40u32,
    @fpu F64Min => 41u32,
    @fpu F64Max => 42u32,
    @fpu F64Copysign => 43u32,
    @fpu I32TruncF32S => 44u32,
    @fpu I32TruncF32U => 45u32,
    @fpu I32TruncF64S => 46u32,
    @fpu I32TruncF64U => 47u32,
    @fpu I64TruncF32S => 48u32,
    @fpu I64TruncF32U => 49u32,
    @fpu I64TruncF64S => 50u32,
    @fpu I64TruncF64U => 51u32,
    @fpu F32ConvertI32S => 52u32,
    @fpu F32ConvertI32U => 53u32,
    @fpu F32ConvertI64S => 54u32,
    @fpu F32ConvertI64U => 55u32,
    @fpu F32DemoteF64 => 56u32,
    @fpu F64ConvertI32S => 57u32,
    @fpu F64ConvertI32U => 58u32,
    @fpu F64ConvertI64S => 59u32,
    @fpu F64ConvertI64U => 60u32,
    @fpu F64PromoteF32 => 61u32,
    @fpu I32TruncSatF32S => 62u32,
    @fpu I32TruncSatF32U => 63u32,
    @fpu I32TruncSatF64S => 64u32,
    @fpu I32TruncSatF64U => 65u32,
    @fpu I64TruncSatF32S => 66u32,
    @fpu I64TruncSatF32U => 67u32,
    @fpu I64TruncSatF64S => 68u32,
    @fpu I64TruncSatF64U => 69u32,
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
        assert_eq!(data, [2, 0, 0, 0, 7, 0, 0, 0]);

        let (decoded, decoded_len): (Opcode, usize) =
            bincode::decode_from_slice(&data, bincode::config::legacy()).unwrap();
        assert_eq!(decoded, opcode);
        assert_eq!(decoded_len, data.len());
    }

    #[test]
    fn test_opcode_encoding_uses_explicit_code() {
        let opcode = Opcode::TableCopy(3, 7);
        let data = bincode::encode_to_vec(opcode, bincode::config::legacy()).unwrap();
        assert_eq!(&data[..4], &opcode.code().to_le_bytes());

        let (decoded, decoded_len): (Opcode, usize) =
            bincode::decode_from_slice(&data, bincode::config::legacy()).unwrap();
        assert_eq!(decoded, opcode);
        assert_eq!(decoded_len, data.len());
    }

    #[test]
    fn test_fpu_opcode_encoding_uses_offset() {
        let opcode = Opcode::F32Load(7);
        let data = bincode::encode_to_vec(opcode, bincode::config::legacy()).unwrap();
        assert_eq!(opcode.code(), 1000);
        assert_eq!(&data[..4], &1000u32.to_le_bytes());

        let (decoded, decoded_len): (Opcode, usize) =
            bincode::decode_from_slice(&data, bincode::config::legacy()).unwrap();
        assert_eq!(decoded, opcode);
        assert_eq!(decoded_len, data.len());
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
