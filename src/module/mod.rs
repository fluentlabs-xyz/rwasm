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
    fn assert_send_sync<T>() {}
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
        let result = parser.finalize(wasm_binary)?;
        Ok(result)
    }

    pub fn empty() -> Self {
        RwasmModuleInner {
            code_section: InstructionSet::default(),
            data_section: vec![],
            elem_section: vec![],
            hint_section: vec![],
            source_pc: 0,
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
    /// The source program counter-offset is always 0.
    pub code_section: InstructionSet,

    /// Linear read-only memory data initialized when the module is instantiated.
    pub data_section: Vec<u8>,

    /// Table initializers, function refs for the module's table section.
    pub elem_section: Vec<u32>,

    /// A hint section that stores original bytecode that used as a compiler input.
    /// It can be Wasm, EVM bytecode, or anything else.
    /// Use this section signature bytes to determine the type of the file,
    /// always fallback to EVM if it can't be extracted.
    pub hint_section: Vec<u8>,

    /// A program counter that points to the original bytecode offset, where the execution starts.
    /// But it ignores start and init sections.
    /// If you want to start with init (like the first function run, then use 0 offset, otherwise this PC).
    ///
    /// Note: For old binaries this is always 0.
    pub source_pc: u32,
}

/// Rwasm magic bytes 0xef52 (0x52 stands for 'R' in ASCII)
pub const RWASM_MAGIC_BYTE_0: u8 = 0xef;
pub const RWASM_MAGIC_BYTE_1: u8 = 0x52;

/// Rwasm binary version
pub const RWASM_VERSION_V1: u8 = 0x01;

impl Encode for RwasmModuleInner {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        Encode::encode(&RWASM_MAGIC_BYTE_0, encoder)?;
        Encode::encode(&RWASM_MAGIC_BYTE_1, encoder)?;
        Encode::encode(&RWASM_VERSION_V1, encoder)?;
        Encode::encode(&self.code_section, encoder)?;
        Encode::encode(&self.data_section, encoder)?;
        Encode::encode(&self.elem_section, encoder)?;
        Encode::encode(&self.hint_section, encoder)?;
        Encode::encode(&self.source_pc, encoder)?;
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
        let source_pc: u32 = match Decode::decode(decoder) {
            Ok(source_pc) => source_pc,
            Err(DecodeError::UnexpectedEnd { additional }) => {
                debug_assert_eq!(
                    additional,
                    size_of::<u32>(),
                    "rwasm: unexpected end of source_pc decoding, expected 4 bytes"
                );
                // This field is optional if it's not presented, then fallback to 0
                0
            }
            Err(err) => return Err(err),
        };
        Ok(Self {
            code_section,
            data_section,
            elem_section,
            hint_section: wasm_section,
            source_pc,
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
            if pos == self.source_pc as usize {
                write!(f, "  <- SOURCE")?;
            }
            writeln!(f)?;
        }
        writeln!(f, " .function_end\n")?;
        writeln!(f, " .ro_data: {:x?},", self.data_section.as_slice())?;
        writeln!(f, " .ro_elem: {:?},", self.elem_section.as_slice())?;
        writeln!(f, " .source_pc: {:?},", self.source_pc)?;
        writeln!(f, "}}")?;
        Ok(())
    }
}

#[derive(Default)]
pub struct RwasmModuleBuilder {
    code_section: InstructionSet,
    data_section: Vec<u8>,
    elem_section: Vec<u32>,
    hint_section: Vec<u8>,
    source_pc: u32,
}

impl RwasmModuleBuilder {
    pub fn new(code_section: InstructionSet) -> Self {
        Self {
            code_section,
            ..Default::default()
        }
    }

    pub fn with_data_section(mut self, data: &[u8]) -> Self {
        self.data_section.extend_from_slice(data);
        self
    }

    pub fn with_elem_section(mut self, elem: &[u32]) -> Self {
        self.elem_section.extend_from_slice(elem);
        self
    }

    pub fn with_hint_section(mut self, hint: &[u8]) -> Self {
        self.hint_section = hint.to_vec();
        self
    }

    pub fn with_source_pc(mut self, source_pc: u32) -> Self {
        self.source_pc = source_pc;
        self
    }

    pub fn build(self) -> RwasmModule {
        RwasmModule {
            inner: Arc::new(RwasmModuleInner {
                code_section: self.code_section,
                data_section: self.data_section,
                elem_section: self.elem_section,
                hint_section: self.hint_section,
                source_pc: self.source_pc,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{instruction_set, RwasmModuleInner};
    use hex_literal::hex;

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
            source_pc: u32::MAX,
        };
        let encoded_module = bincode::encode_to_vec(&module, bincode::config::legacy()).unwrap();
        println!("{}", hex::encode(&encoded_module));
        let module2: RwasmModuleInner;
        (module2, _) =
            bincode::decode_from_slice(&encoded_module, bincode::config::legacy()).unwrap();
        assert_eq!(module, module2);
    }

    #[test]
    fn test_decode_module_wo_source_pc() {
        const LEGACY_MODULE: &[u8] = &hex!("ef52010600000000000000150000006400000015000000140000003e00000015000000030000003e000000160000000000000000000000050000000000000005000000060000000700000008000000090000000000000000000000");
        let module2: RwasmModuleInner;
        (module2, _) =
            bincode::decode_from_slice(&LEGACY_MODULE, bincode::config::legacy()).unwrap();
        assert_eq!(module2.source_pc, 0);
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
