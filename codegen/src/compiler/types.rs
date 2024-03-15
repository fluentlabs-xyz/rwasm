use crate::{BinaryFormatError, InstructionSet};
use alloc::vec::Vec;
use rwasm::{engine::bytecode::Instruction, module::ImportName, Error};

#[derive(Debug)]
pub enum CompilerError {
    ModuleError(Error),
    MissingEntrypoint,
    MissingFunction,
    NotSupported(&'static str),
    OutOfBuffer,
    BinaryFormat(BinaryFormatError),
    NotSupportedImport,
    UnknownImport(ImportName),
    MemoryUsageTooBig,
    DropKeepOutOfBounds,
    ExportedGlobalsAreDisabled,
    NotSupportedGlobalExpr,
    OnlyFuncRefAllowed,
}

impl CompilerError {
    pub fn into_i32(self) -> i32 {
        match self {
            CompilerError::ModuleError(_) => -1,
            CompilerError::MissingEntrypoint => -2,
            CompilerError::MissingFunction => -3,
            CompilerError::NotSupported(_) => -4,
            CompilerError::OutOfBuffer => -5,
            CompilerError::BinaryFormat(_) => -6,
            CompilerError::NotSupportedImport => -7,
            CompilerError::UnknownImport(_) => -8,
            CompilerError::MemoryUsageTooBig => -9,
            CompilerError::DropKeepOutOfBounds => -10,
            CompilerError::ExportedGlobalsAreDisabled => -10,
            CompilerError::NotSupportedGlobalExpr => -11,
            CompilerError::OnlyFuncRefAllowed => -12,
        }
    }
}

impl Into<i32> for CompilerError {
    fn into(self) -> i32 {
        self.into_i32()
    }
}

#[derive(Debug)]
pub struct Injection {
    pub begin: i32,
    pub end: i32,
    pub origin_len: i32,
}

// #[derive(Debug)]
// pub struct BrTableStatus {
//     pub(crate) injection_instructions: Vec<Instruction>,
//     pub(crate) instr_countdown: u32,
// }

#[derive(Debug)]
pub enum FuncOrExport {
    Export(&'static str),
    Func(u32),
    #[deprecated(note = "will be removed")]
    StateRouter(Vec<FuncOrExport>, InstructionSet),
    #[deprecated(note = "will be removed")]
    Global(Instruction),
    Custom(InstructionSet),
}

impl Default for FuncOrExport {
    fn default() -> Self {
        Self::Export("main")
    }
}

impl FuncOrExport {}

// #[derive(Debug)]
// pub struct FuncSourceMap {
//     pub fn_index: u32,
//     pub fn_name: String,
//     pub position: u32,
//     pub length: u32,
// }

// pub const FUNC_SOURCE_MAP_ENTRYPOINT_NAME: &'static str = "$__entrypoint";
// pub const FUNC_SOURCE_MAP_ENTRYPOINT_IDX: u32 = u32::MAX;
