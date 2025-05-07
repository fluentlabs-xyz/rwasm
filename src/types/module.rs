use crate::types::InstructionSet;

#[derive(Default, Debug, PartialEq)]
pub struct RwasmModule {
    pub code_section: InstructionSet,
    pub memory_section: Vec<u8>,
    pub element_section: Vec<u32>,
    pub source_pc: u32,
    pub func_section: Vec<u32>,
}

impl RwasmModule {
    pub fn new_or_empty(sink: &[u8]) -> Self {
        if sink.is_empty() {
            Self::empty()
        } else {
            Self::new(sink)
        }
    }

    pub fn empty() -> Self {
        Self {
            code_section: InstructionSet::default(),
            memory_section: vec![],
            element_section: vec![],
            source_pc: 0,
            func_section: vec![0],
        }
    }

    pub fn new(sink: &[u8]) -> Self {
        let module: RwasmModule;
        (module, _) = bincode::decode_from_slice(sink, bincode::config::legacy())
            .unwrap_or_else(|_| unreachable!("rwasm: malformed rwasm binary"));
        module
    }

    pub fn instantiate(&mut self) {
        let source_pc = self
            .func_section
            .last()
            .copied()
            .expect("rwasm: empty function section");
        self.source_pc = source_pc;
    }
}
