use alloc::{collections::BTreeMap, string::String, vec::Vec};
use rwasm::{
    core::{UntypedValue, ValueType},
    module::ImportName,
    FuncType,
};

pub trait ImportHandler {
    // sys calls
    fn sys_halt(&mut self, _exit_code: u32) {}
    fn sys_write(&mut self, _offset: u32, _length: u32) {}
    fn sys_read(&mut self, _offset: u32, _length: u32) {}
    // evm calls
    fn evm_return(&mut self, _offset: u32, _length: u32) {}
}

#[derive(Default, Debug)]
pub struct DefaultImportHandler {
    pub input: Vec<UntypedValue>,
    exit_code: u32,
    output: Vec<UntypedValue>,
    output_len: u32,
    pub state: u32,
}

impl ImportHandler for DefaultImportHandler {
    fn sys_halt(&mut self, exit_code: u32) {
        self.exit_code = exit_code;
    }

    fn sys_write(&mut self, _offset: u32, _length: u32) {}
    fn sys_read(&mut self, _offset: u32, _length: u32) {}

    fn evm_return(&mut self, _offset: u32, _length: u32) {}
}

impl DefaultImportHandler {
    pub fn new(input: Vec<UntypedValue>) -> Self {
        Self {
            input,
            ..Default::default()
        }
    }

    pub fn next_input(&mut self) -> Option<UntypedValue> {
        self.input.pop()
    }

    pub fn exit_code(&self) -> u32 {
        self.exit_code
    }

    pub fn output(&self) -> &Vec<UntypedValue> {
        &self.output
    }

    pub fn output_len(&self) -> u32 {
        self.output_len
    }

    pub fn clear_ouput(&mut self, new_output_len: u32) {
        self.output = vec![];
        self.output_len = new_output_len;
    }

    pub fn add_result(&mut self, result: UntypedValue) {
        self.output.push(result);
    }
}
