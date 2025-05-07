use crate::types::{InstructionSet, Opcode, OpcodeData};
use bincode::{
    de::Decoder,
    enc::Encoder,
    error::{AllowedEnumVariants, DecodeError, EncodeError},
    Decode,
    Encode,
};
use num_enum::TryFromPrimitive;

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
