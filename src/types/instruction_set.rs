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
    Opcode,
    OpcodeData,
    SignatureIdx,
    StackAlloc,
    TableIdx,
    UntypedValue,
};
use alloc::vec::Vec;
use bincode::{
    de::Decoder,
    enc::Encoder,
    error::{AllowedEnumVariants, DecodeError, EncodeError},
    Decode,
    Encode,
};
use core::ops::{Deref, DerefMut};
use num_enum::TryFromPrimitive;

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub struct InstructionSet {
    pub instr: Vec<(Opcode, OpcodeData)>,
}

impl Deref for InstructionSet {
    type Target = Vec<(Opcode, OpcodeData)>;

    fn deref(&self) -> &Self::Target {
        &self.instr
    }
}

impl DerefMut for InstructionSet {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.instr
    }
}

impl Default for InstructionSet {
    fn default() -> Self {
        Self {
            instr: vec![(Opcode::Return, OpcodeData::DropKeep(DropKeep::default()))],
        }
    }
}

macro_rules! impl_opcode {
    ($opcode:ident($data_type:ident)) => {
        paste::paste! {
            pub fn [< op_ $opcode:snake >]<I: Into<$data_type>>(&mut self, value: I) -> u32 {
                self.push(
                    Opcode::$opcode,
                    OpcodeData::$data_type(value.into()),
                )
            }
        }
    };
    ($opcode:ident) => {
        paste::paste! {
            pub fn [< op_ $opcode:snake >](&mut self) -> u32 {
                self.push(Opcode::$opcode, OpcodeData::EmptyData)
            }
        }
    };
}

impl InstructionSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, opcode: Opcode, data: OpcodeData) -> u32 {
        let idx = self.instr.len() as u32;
        self.instr.push((opcode, data));
        idx
    }

    pub fn clear(&mut self) {
        self.instr.clear();
    }

    fn is_return_last(&self) -> bool {
        self.instr
            .last()
            .map(|instr| match instr.0 {
                Opcode::Return => true,
                _ => false,
            })
            .unwrap_or_default()
    }

    pub fn finalize(&mut self, inject_return: bool) {
        // inject return in the end (it's used mostly for unit tests)
        if inject_return && !self.is_return_last() {
            self.op_return(DropKeep::default());
        }
    }

    impl_opcode!(LocalGet(LocalDepth));
    impl_opcode!(LocalSet(LocalDepth));
    impl_opcode!(LocalTee(LocalDepth));
    impl_opcode!(Br(BranchOffset));
    impl_opcode!(BrIfEqz(BranchOffset));
    impl_opcode!(BrIfNez(BranchOffset));
    impl_opcode!(BrAdjust(BranchOffset));
    impl_opcode!(BrAdjustIfNez(BranchOffset));
    impl_opcode!(BrTable(BranchTableTargets));
    impl_opcode!(Unreachable);
    impl_opcode!(ConsumeFuel(BlockFuel));
    impl_opcode!(Return(DropKeep));
    impl_opcode!(ReturnIfNez(DropKeep));
    impl_opcode!(ReturnCallInternal(CompiledFunc));
    impl_opcode!(ReturnCall(FuncIdx));
    impl_opcode!(ReturnCallIndirect(SignatureIdx));
    impl_opcode!(CallInternal(CompiledFunc));
    impl_opcode!(Call(FuncIdx));
    impl_opcode!(CallIndirect(SignatureIdx));
    impl_opcode!(SignatureCheck(SignatureIdx));
    impl_opcode!(Drop);
    impl_opcode!(Select);
    impl_opcode!(GlobalGet(GlobalIdx));
    impl_opcode!(GlobalSet(GlobalIdx));
    impl_opcode!(I32Load(AddressOffset));
    impl_opcode!(I64Load(AddressOffset));
    impl_opcode!(F32Load(AddressOffset));
    impl_opcode!(F64Load(AddressOffset));
    impl_opcode!(I32Load8S(AddressOffset));
    impl_opcode!(I32Load8U(AddressOffset));
    impl_opcode!(I32Load16S(AddressOffset));
    impl_opcode!(I32Load16U(AddressOffset));
    impl_opcode!(I64Load8S(AddressOffset));
    impl_opcode!(I64Load8U(AddressOffset));
    impl_opcode!(I64Load16S(AddressOffset));
    impl_opcode!(I64Load16U(AddressOffset));
    impl_opcode!(I64Load32S(AddressOffset));
    impl_opcode!(I64Load32U(AddressOffset));
    impl_opcode!(I32Store(AddressOffset));
    impl_opcode!(I64Store(AddressOffset));
    impl_opcode!(F32Store(AddressOffset));
    impl_opcode!(F64Store(AddressOffset));
    impl_opcode!(I32Store8(AddressOffset));
    impl_opcode!(I32Store16(AddressOffset));
    impl_opcode!(I64Store8(AddressOffset));
    impl_opcode!(I64Store16(AddressOffset));
    impl_opcode!(I64Store32(AddressOffset));
    impl_opcode!(MemorySize);
    impl_opcode!(MemoryGrow);
    impl_opcode!(MemoryFill);
    impl_opcode!(MemoryCopy);
    impl_opcode!(MemoryInit(DataSegmentIdx));
    impl_opcode!(DataDrop(DataSegmentIdx));
    impl_opcode!(TableSize(TableIdx));
    impl_opcode!(TableGrow(TableIdx));
    impl_opcode!(TableFill(TableIdx));
    impl_opcode!(TableGet(TableIdx));
    impl_opcode!(TableSet(TableIdx));
    impl_opcode!(TableCopy(TableIdx));
    impl_opcode!(TableInit(TableIdx));
    pub fn op_table_init_checked(&mut self, table_idx: TableIdx, elem_idx: ElementSegmentIdx) {
        self.push(Opcode::TableInit, OpcodeData::ElementSegmentIdx(elem_idx));
        self.push(Opcode::TableGet, OpcodeData::TableIdx(table_idx));
    }
    impl_opcode!(ElemDrop(ElementSegmentIdx));
    impl_opcode!(RefFunc(FuncIdx));
    impl_opcode!(I32Const(UntypedValue));
    impl_opcode!(I64Const(UntypedValue));
    impl_opcode!(F32Const(UntypedValue));
    impl_opcode!(F64Const(UntypedValue));
    impl_opcode!(I32Eqz);
    impl_opcode!(I32Eq);
    impl_opcode!(I32Ne);
    impl_opcode!(I32LtS);
    impl_opcode!(I32LtU);
    impl_opcode!(I32GtS);
    impl_opcode!(I32GtU);
    impl_opcode!(I32LeS);
    impl_opcode!(I32LeU);
    impl_opcode!(I32GeS);
    impl_opcode!(I32GeU);
    impl_opcode!(I64Eqz);
    impl_opcode!(I64Eq);
    impl_opcode!(I64Ne);
    impl_opcode!(I64LtS);
    impl_opcode!(I64LtU);
    impl_opcode!(I64GtS);
    impl_opcode!(I64GtU);
    impl_opcode!(I64LeS);
    impl_opcode!(I64LeU);
    impl_opcode!(I64GeS);
    impl_opcode!(I64GeU);
    impl_opcode!(F32Eq);
    impl_opcode!(F32Ne);
    impl_opcode!(F32Lt);
    impl_opcode!(F32Gt);
    impl_opcode!(F32Le);
    impl_opcode!(F32Ge);
    impl_opcode!(F64Eq);
    impl_opcode!(F64Ne);
    impl_opcode!(F64Lt);
    impl_opcode!(F64Gt);
    impl_opcode!(F64Le);
    impl_opcode!(F64Ge);
    impl_opcode!(I32Clz);
    impl_opcode!(I32Ctz);
    impl_opcode!(I32Popcnt);
    impl_opcode!(I32Add);
    impl_opcode!(I32Sub);
    impl_opcode!(I32Mul);
    impl_opcode!(I32DivS);
    impl_opcode!(I32DivU);
    impl_opcode!(I32RemS);
    impl_opcode!(I32RemU);
    impl_opcode!(I32And);
    impl_opcode!(I32Or);
    impl_opcode!(I32Xor);
    impl_opcode!(I32Shl);
    impl_opcode!(I32ShrS);
    impl_opcode!(I32ShrU);
    impl_opcode!(I32Rotl);
    impl_opcode!(I32Rotr);
    impl_opcode!(I64Clz);
    impl_opcode!(I64Ctz);
    impl_opcode!(I64Popcnt);
    impl_opcode!(I64Add);
    impl_opcode!(I64Sub);
    impl_opcode!(I64Mul);
    impl_opcode!(I64DivS);
    impl_opcode!(I64DivU);
    impl_opcode!(I64RemS);
    impl_opcode!(I64RemU);
    impl_opcode!(I64And);
    impl_opcode!(I64Or);
    impl_opcode!(I64Xor);
    impl_opcode!(I64Shl);
    impl_opcode!(I64ShrS);
    impl_opcode!(I64ShrU);
    impl_opcode!(I64Rotl);
    impl_opcode!(I64Rotr);
    impl_opcode!(F32Abs);
    impl_opcode!(F32Neg);
    impl_opcode!(F32Ceil);
    impl_opcode!(F32Floor);
    impl_opcode!(F32Trunc);
    impl_opcode!(F32Nearest);
    impl_opcode!(F32Sqrt);
    impl_opcode!(F32Add);
    impl_opcode!(F32Sub);
    impl_opcode!(F32Mul);
    impl_opcode!(F32Div);
    impl_opcode!(F32Min);
    impl_opcode!(F32Max);
    impl_opcode!(F32Copysign);
    impl_opcode!(F64Abs);
    impl_opcode!(F64Neg);
    impl_opcode!(F64Ceil);
    impl_opcode!(F64Floor);
    impl_opcode!(F64Trunc);
    impl_opcode!(F64Nearest);
    impl_opcode!(F64Sqrt);
    impl_opcode!(F64Add);
    impl_opcode!(F64Sub);
    impl_opcode!(F64Mul);
    impl_opcode!(F64Div);
    impl_opcode!(F64Min);
    impl_opcode!(F64Max);
    impl_opcode!(F64Copysign);
    impl_opcode!(I32WrapI64);
    impl_opcode!(I32TruncF32S);
    impl_opcode!(I32TruncF32U);
    impl_opcode!(I32TruncF64S);
    impl_opcode!(I32TruncF64U);
    impl_opcode!(I64ExtendI32S);
    impl_opcode!(I64ExtendI32U);
    impl_opcode!(I64TruncF32S);
    impl_opcode!(I64TruncF32U);
    impl_opcode!(I64TruncF64S);
    impl_opcode!(I64TruncF64U);
    impl_opcode!(F32ConvertI32S);
    impl_opcode!(F32ConvertI32U);
    impl_opcode!(F32ConvertI64S);
    impl_opcode!(F32ConvertI64U);
    impl_opcode!(F32DemoteF64);
    impl_opcode!(F64ConvertI32S);
    impl_opcode!(F64ConvertI32U);
    impl_opcode!(F64ConvertI64S);
    impl_opcode!(F64ConvertI64U);
    impl_opcode!(F64PromoteF32);
    impl_opcode!(I32Extend8S);
    impl_opcode!(I32Extend16S);
    impl_opcode!(I64Extend8S);
    impl_opcode!(I64Extend16S);
    impl_opcode!(I64Extend32S);
    impl_opcode!(I32TruncSatF32S);
    impl_opcode!(I32TruncSatF32U);
    impl_opcode!(I32TruncSatF64S);
    impl_opcode!(I32TruncSatF64U);
    impl_opcode!(I64TruncSatF32S);
    impl_opcode!(I64TruncSatF32U);
    impl_opcode!(I64TruncSatF64S);
    impl_opcode!(I64TruncSatF64U);
    impl_opcode!(StackAlloc(StackAlloc));
}

#[macro_export]
macro_rules! instruction_set_internal {
    // Nothing left to do
    ($code:ident, ) => {};
    ($code:ident, $x:ident ($v:expr) $($rest:tt)*) => {
        _ = crate::types::Opcode::$x;
        paste::paste! {
            $code.[< op_ $x:snake >]($v);
        }
        $crate::instruction_set_internal!($code, $($rest)*);
    };
    // Default opcode without any inputs
    ($code:ident, $x:ident $($rest:tt)*) => {
        _ = crate::types::Opcode::$x;
        paste::paste! {
            $code.[< op_ $x:snake >]();
        }
        $crate::instruction_set_internal!($code, $($rest)*);
    };
    // Function calls
    ($code:ident, .$function:ident ($($args:expr),* $(,)?) $($rest:tt)*) => {
        $code.$function($($args,)*);
        $crate::instruction_set_internal!($code, $($rest)*);
    };
}

#[macro_export]
macro_rules! instruction_set {
    ($($args:tt)*) => {{
        let mut code = $crate::types::InstructionSet::new();
        $crate::instruction_set_internal!(code, $($args)*);
        code
    }};
}

impl Encode for InstructionSet {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        let length = self.instr.len() as u64;
        Encode::encode(&length, encoder)?;
        for instr in &self.instr {
            let instr_value = instr.0 as u8;
            Encode::encode(&instr_value, encoder)?;
            encode_instruction_data(&instr.1, encoder)?;
        }
        Ok(())
    }
}

impl<Context> Decode<Context> for InstructionSet {
    fn decode<D: Decoder<Context = Context>>(decoder: &mut D) -> Result<Self, DecodeError> {
        let length: u64 = Decode::decode(decoder)?;
        let mut instr: Vec<(Opcode, OpcodeData)> = Vec::with_capacity(length as usize);
        for _ in 0..length as usize {
            let instr_value: u8 = Decode::decode(decoder)?;
            let opcode = Opcode::try_from_primitive(instr_value)
                .map_err(|_| instruction_not_found_err(instr_value))?;
            let opcode_data = decode_instruction_data(&opcode, decoder)?;
            instr.push((opcode, opcode_data));
        }
        Ok(Self { instr })
    }
}

fn encode_instruction_data<E: Encoder>(
    instruction_data: &OpcodeData,
    encoder: &mut E,
) -> Result<(), EncodeError> {
    match instruction_data {
        OpcodeData::EmptyData => Ok(()),
        OpcodeData::LocalDepth(value) => Encode::encode(&value, encoder),
        OpcodeData::BranchOffset(value) => Encode::encode(&value, encoder),
        OpcodeData::BranchTableTargets(value) => Encode::encode(&value, encoder),
        OpcodeData::BlockFuel(value) => Encode::encode(&value, encoder),
        OpcodeData::DropKeep(value) => Encode::encode(&value, encoder),
        OpcodeData::CompiledFunc(value) => Encode::encode(&value, encoder),
        OpcodeData::FuncIdx(value) => Encode::encode(&value, encoder),
        OpcodeData::SignatureIdx(value) => Encode::encode(&value, encoder),
        OpcodeData::GlobalIdx(value) => Encode::encode(&value, encoder),
        OpcodeData::AddressOffset(value) => Encode::encode(&value, encoder),
        OpcodeData::DataSegmentIdx(value) => Encode::encode(&value, encoder),
        OpcodeData::TableIdx(value) => Encode::encode(&value, encoder),
        OpcodeData::ElementSegmentIdx(value) => Encode::encode(&value, encoder),
        OpcodeData::UntypedValue(value) => Encode::encode(&value, encoder),
        OpcodeData::StackAlloc(value) => Encode::encode(&value, encoder),
    }
}

fn decode_instruction_data<Context, D: Decoder<Context = Context>>(
    instruction: &Opcode,
    decoder: &mut D,
) -> Result<OpcodeData, DecodeError> {
    use Opcode::*;
    let instruction_data = match instruction {
        LocalGet | LocalSet | LocalTee => OpcodeData::LocalDepth(Decode::decode(decoder)?),
        Br | BrIfEqz | BrIfNez | BrAdjust | BrAdjustIfNez => {
            OpcodeData::BranchOffset(Decode::decode(decoder)?)
        }
        BrTable => OpcodeData::BranchTableTargets(Decode::decode(decoder)?),
        ConsumeFuel => OpcodeData::BlockFuel(Decode::decode(decoder)?),
        Return | ReturnIfNez => OpcodeData::DropKeep(Decode::decode(decoder)?),
        ReturnCallInternal | CallInternal => OpcodeData::CompiledFunc(Decode::decode(decoder)?),
        ReturnCall | Call | RefFunc => OpcodeData::FuncIdx(Decode::decode(decoder)?),
        ReturnCallIndirect | CallIndirect | SignatureCheck => {
            OpcodeData::SignatureIdx(Decode::decode(decoder)?)
        }
        GlobalGet | GlobalSet => OpcodeData::GlobalIdx(Decode::decode(decoder)?),
        I32Load | I64Load | F32Load | F64Load | I32Load8S | I32Load8U | I32Load16S | I32Load16U
        | I64Load8S | I64Load8U | I64Load16S | I64Load16U | I64Load32S | I64Load32U | I32Store
        | I64Store | F32Store | F64Store | I32Store8 | I32Store16 | I64Store8 | I64Store16
        | I64Store32 => OpcodeData::AddressOffset(Decode::decode(decoder)?),
        MemoryInit | DataDrop => OpcodeData::DataSegmentIdx(Decode::decode(decoder)?),
        TableSize | TableGrow | TableFill | TableGet | TableSet | TableCopy => {
            OpcodeData::TableIdx(Decode::decode(decoder)?)
        }
        TableInit | ElemDrop => OpcodeData::ElementSegmentIdx(Decode::decode(decoder)?),
        I32Const | I64Const | F32Const | F64Const => {
            OpcodeData::UntypedValue(Decode::decode(decoder)?)
        }
        StackAlloc => OpcodeData::StackAlloc(Decode::decode(decoder)?),
        _ => OpcodeData::EmptyData,
    };
    Ok(instruction_data)
}

fn instruction_not_found_err(instr_value: u8) -> DecodeError {
    static RANGE: AllowedEnumVariants = AllowedEnumVariants::Range { min: 0, max: 0xc6 };
    DecodeError::UnexpectedVariant {
        type_name: "Instruction",
        allowed: &RANGE,
        found: instr_value as u32,
    }
}
