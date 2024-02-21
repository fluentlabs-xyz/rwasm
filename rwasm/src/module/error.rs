use super::{ImportName, ReadError};
use crate::engine::TranslationError;
use core::{
    fmt,
    fmt::{Debug, Display},
};
use wasmparser::BinaryReaderError as ParserError;

#[derive(Debug)]
pub enum RwasmBuilderError {
    MissingEntrypoint,
    MissingFunction,
    OutOfBuffer,
    NotSupportedImport,
    UnknownImport(ImportName),
    MemoryUsageTooBig,
    DropKeepOutOfBounds,
    ExportedGlobalsAreDisabled,
    NotSupportedGlobalExpr,
    OnlyFuncRefAllowed,
}

impl Display for RwasmBuilderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEntrypoint => write!(f, "MissingEntrypoint"),
            Self::MissingFunction => write!(f, "MissingFunction"),
            Self::OutOfBuffer => write!(f, "OutOfBuffer"),
            Self::NotSupportedImport => write!(f, "NotSupportedImport"),
            Self::UnknownImport(_) => write!(f, "UnknownImport"),
            Self::MemoryUsageTooBig => write!(f, "MemoryUsageTooBig"),
            Self::DropKeepOutOfBounds => write!(f, "DropKeepOutOfBounds"),
            Self::ExportedGlobalsAreDisabled => write!(f, "ExportedGlobalsAreDisabled"),
            Self::NotSupportedGlobalExpr => write!(f, "NotSupportedGlobalExpr"),
            Self::OnlyFuncRefAllowed => write!(f, "OnlyFuncRefAllowed"),
        }
    }
}

/// Errors that may occur upon reading, parsing and translating Wasm modules.
#[derive(Debug)]
pub enum ModuleError {
    /// Encountered when there is a problem with the Wasm input stream.
    Read(ReadError),
    /// Encountered when there is a Wasm parsing error.
    Parser(ParserError),
    /// Encountered when there is a Wasm to `wasmi` translation error.
    Translation(TranslationError),
    /// Error that happens during rWASM build
    Rwasm(RwasmBuilderError),
}

impl Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModuleError::Read(error) => Display::fmt(error, f),
            ModuleError::Parser(error) => Display::fmt(error, f),
            ModuleError::Translation(error) => Display::fmt(error, f),
            ModuleError::Rwasm(error) => Display::fmt(error, f),
        }
    }
}

impl From<ReadError> for ModuleError {
    fn from(error: ReadError) -> Self {
        Self::Read(error)
    }
}

impl From<ParserError> for ModuleError {
    fn from(error: ParserError) -> Self {
        Self::Parser(error)
    }
}

impl From<TranslationError> for ModuleError {
    fn from(error: TranslationError) -> Self {
        Self::Translation(error)
    }
}

impl From<RwasmBuilderError> for ModuleError {
    fn from(error: RwasmBuilderError) -> Self {
        Self::Rwasm(error)
    }
}
