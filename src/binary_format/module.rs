use crate::types::{InstructionSet, RwasmModule};
use bincode::{
    de::Decoder,
    enc::Encoder,
    error::{DecodeError, EncodeError},
    Decode,
    Encode,
};

/// Rwasm magic bytes 0xef52
const RWASM_MAGIC_BYTE_0: u8 = 0xef;
const RWASM_MAGIC_BYTE_1: u8 = 0x52;

/// Rwasm binary version that is equal to the 'R' symbol (0x52 in hex)
const RWASM_VERSION_V1: u8 = 0x01;

impl Encode for RwasmModule {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        Encode::encode(&RWASM_MAGIC_BYTE_0, encoder)?;
        Encode::encode(&RWASM_MAGIC_BYTE_1, encoder)?;
        Encode::encode(&RWASM_VERSION_V1, encoder)?;
        Encode::encode(&self.code_section, encoder)?;
        Encode::encode(&self.memory_section, encoder)?;
        Encode::encode(&self.element_section, encoder)?;
        Encode::encode(&self.source_pc, encoder)?;
        Encode::encode(&self.func_section, encoder)?;
        Ok(())
    }
}

impl<Context> Decode<Context> for RwasmModule {
    fn decode<D: Decoder<Context = Context>>(decoder: &mut D) -> Result<Self, DecodeError> {
        let sig0: u8 = Decode::decode(decoder)?;
        let sig1: u8 = Decode::decode(decoder)?;
        if sig0 != RWASM_MAGIC_BYTE_0 || sig1 != RWASM_MAGIC_BYTE_1 {
            return Err(DecodeError::Other("rwasm: invalid magic bytes"));
        }
        let version: u8 = Decode::decode(decoder)?;
        if version != RWASM_VERSION_V1 {
            return Err(DecodeError::Other("rwasm: not supported version"));
        }
        let code_section: InstructionSet = Decode::decode(decoder)?;
        let memory_section: Vec<u8> = Decode::decode(decoder)?;
        let element_section: Vec<u32> = Decode::decode(decoder)?;
        let source_pc: u32 = Decode::decode(decoder)?;
        let func_segments: Vec<u32> = Decode::decode(decoder)?;
        Ok(Self {
            code_section,
            memory_section,
            element_section,
            source_pc,
            func_section: func_segments,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{instruction_set, types::RwasmModule};

    #[test]
    fn test_module_encoding() {
        let module = RwasmModule {
            code_section: instruction_set! {
                I32Const(100)
                I32Const(20)
                I32Add
                I32Const(3)
                I32Add
                Drop
            },
            memory_section: Default::default(),
            func_section: vec![0, 1, 2, 3, 4],
            element_section: vec![5, 6, 7, 8, 9],
            source_pc: 7,
        };
        let encoded_module = bincode::encode_to_vec(&module, bincode::config::legacy()).unwrap();
        let module2: RwasmModule;
        (module2, _) =
            bincode::decode_from_slice(&encoded_module, bincode::config::legacy()).unwrap();
        assert_eq!(module, module2);
    }
}
