use crate::{
    CompilationConfig, CompilationError, ConstructorParams, HintType, InstructionSet, ModuleParser,
    Opcode,
};
use alloc::{sync::Arc, vec, vec::Vec};
use bincode::{
    de::Decoder,
    enc::Encoder,
    error::{DecodeError, EncodeError},
    Decode, Encode,
};
use core::ops::Deref;

/// Represents a compiled rWasm module.
///
/// An `RwasmModule` encapsulates the executable code, static data, and element (function/table
/// reference) information needed for execution within the rWasm virtual machine.
///
/// It's compiled from Wasm
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Default, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct RwasmModule {
    inner: Arc<RwasmModuleInner>,
}

fn _check() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RwasmModule>();
}

impl RwasmModule {
    pub fn new_or_empty(sink: &[u8]) -> (Self, usize) {
        if sink.is_empty() {
            (Self::empty(), 0)
        } else {
            Self::new(sink)
        }
    }

    pub fn compile(
        config: CompilationConfig,
        wasm_binary: &[u8],
    ) -> Result<(Self, ConstructorParams), CompilationError> {
        let mut parser = ModuleParser::new(config);
        parser.parse(wasm_binary)?;
        parser.finalize(wasm_binary)
    }

    pub fn empty() -> Self {
        RwasmModuleInner {
            code_section: InstructionSet::default(),
            data_section: vec![],
            elem_section: vec![],
            hint_section: vec![],
        }
        .into()
    }

    pub fn with_one_function(code_section: InstructionSet) -> Self {
        RwasmModuleInner {
            code_section,
            data_section: vec![],
            elem_section: vec![],
            hint_section: vec![],
        }
        .into()
    }

    pub fn new(sink: &[u8]) -> (Self, usize) {
        Self::new_checked(sink).unwrap_or_else(|_| unreachable!("rwasm: malformed rwasm binary"))
    }

    pub fn new_checked(sink: &[u8]) -> Result<(Self, usize), DecodeError> {
        let (inner, bytes_read): (RwasmModuleInner, usize) =
            bincode::decode_from_slice(sink, bincode::config::legacy())?;
        Ok((inner.into(), bytes_read))
    }

    pub fn serialize(&self) -> Vec<u8> {
        bincode::encode_to_vec(&*self.inner, bincode::config::legacy())
            .unwrap_or_else(|_| unreachable!("rwasm: failed to serialize module"))
    }

    pub fn hint_type(&self) -> HintType {
        HintType::from_ref(&self.hint_section)
    }
}

impl From<RwasmModuleInner> for RwasmModule {
    fn from(value: RwasmModuleInner) -> Self {
        Self {
            inner: Arc::new(value),
        }
    }
}

impl Deref for RwasmModule {
    type Target = RwasmModuleInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Default, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct RwasmModuleInner {
    /// The main instruction set (bytecode) for this module that includes an entrypoint
    /// and all required functions.
    ///
    /// The source program counter offset is always 0.
    pub code_section: InstructionSet,

    /// Linear read-only memory data initialized when the module is instantiated.
    pub data_section: Vec<u8>,

    /// Table initializers, function refs for the module's table section.
    pub elem_section: Vec<u32>,

    /// A hint section that stores original bytecode that used as a compiler input.
    /// It can be Wasm, EVM bytecode or anything else.
    /// Use this section signature bytes to determine the type of the file,
    /// always fallback to EVM if it can't be extracted.
    pub hint_section: Vec<u8>,
}

/// Rwasm magic bytes 0xef52 (0x52 stands for 'R' in ASCII)
const RWASM_MAGIC_BYTE_0: u8 = 0xef;
const RWASM_MAGIC_BYTE_1: u8 = 0x52;

/// Rwasm binary version
const RWASM_VERSION_V1: u8 = 0x01;

impl Encode for RwasmModuleInner {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        Encode::encode(&RWASM_MAGIC_BYTE_0, encoder)?;
        Encode::encode(&RWASM_MAGIC_BYTE_1, encoder)?;
        Encode::encode(&RWASM_VERSION_V1, encoder)?;
        Encode::encode(&self.code_section, encoder)?;
        Encode::encode(&self.data_section, encoder)?;
        Encode::encode(&self.elem_section, encoder)?;
        Encode::encode(&self.hint_section, encoder)?;
        Ok(())
    }
}

impl<Context> Decode<Context> for RwasmModuleInner {
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
        let data_section: Vec<u8> = Decode::decode(decoder)?;
        let elem_section: Vec<u32> = Decode::decode(decoder)?;
        let wasm_section: Vec<u8> = Decode::decode(decoder)?;
        Ok(Self {
            code_section,
            data_section,
            elem_section,
            hint_section: wasm_section,
        })
    }
}

impl core::fmt::Display for RwasmModule {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "RwasmModule {{")?;
        let mut func_num = 0;
        writeln!(f, " .function_begin_{} (#{})", 0, func_num)?;
        for (pos, opcode) in self.code_section.iter().copied().enumerate() {
            if let Some(Opcode::SignatureCheck(_)) = self.code_section.get(pos) {
                writeln!(f, " .function_end\n")?;
                func_num += 1;
                writeln!(f, " .function_begin_{} (#{})", pos, func_num)?;
            }
            write!(f, "  {:04}: {}", pos, opcode)?;
            writeln!(f)?;
        }
        writeln!(f, " .function_end\n")?;
        writeln!(f, " .ro_data: {:x?},", self.data_section.as_slice())?;
        writeln!(f, " .ro_elem: {:?},", self.elem_section.as_slice())?;
        writeln!(f, "}}")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{instruction_set, RwasmModuleInner};

    #[test]
    fn test_module_encoding() {
        let module = RwasmModuleInner {
            code_section: instruction_set! {
                I32Const(100)
                I32Const(20)
                I32Add
                I32Const(3)
                I32Add
                Drop
            },
            data_section: Default::default(),
            elem_section: vec![5, 6, 7, 8, 9],
            hint_section: vec![],
        };
        let encoded_module = bincode::encode_to_vec(&module, bincode::config::legacy()).unwrap();
        let module2: RwasmModuleInner;
        (module2, _) =
            bincode::decode_from_slice(&encoded_module, bincode::config::legacy()).unwrap();
        assert_eq!(module, module2);
    }

    #[test]
    fn test_endianness() {
        let module = vec![1, 2, 3];
        let encoded_module = bincode::encode_to_vec(&module, bincode::config::legacy()).unwrap();
        println!("{:?}", encoded_module);
        let slc = unsafe {
            core::slice::from_raw_parts(encoded_module.as_ptr().offset(8) as *const u32, 3)
        };
        println!("{:?}", slc);
    }
}
