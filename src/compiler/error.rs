use core::fmt::Formatter;
use wasmparser::BinaryReaderError;

#[derive(Debug)]
pub enum CompilationError {
    BranchOffsetOutOfBounds,
    BlockFuelOutOfBounds,
    NotSupportedExtension,
    DropKeepOutOfBounds,
    BranchTableTargetsOutOfBounds,
    MalformedWasmBinary(BinaryReaderError),
    NotSupportedImportType,
    NotSupportedFuncType,
    UnresolvedImportFunction,
    MalformedImportFunctionType,
    NonDefaultMemoryIndex,
    ConstEvaluationFailed,
    NotSupportedLocalType,
    NotSupportedGlobalType,
    MaxReadonlyDataReached,
    MissingEntrypoint,
    MalformedFuncType,
    MemoryOutOfBounds,
    TableOutOfBounds,
}

impl From<BinaryReaderError> for CompilationError {
    fn from(err: BinaryReaderError) -> Self {
        CompilationError::MalformedWasmBinary(err)
    }
}

impl core::fmt::Display for CompilationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            CompilationError::BranchOffsetOutOfBounds => write!(f, "branch offset out of bounds"),
            CompilationError::BlockFuelOutOfBounds => write!(f, "block fuel out of bounds"),
            CompilationError::NotSupportedExtension => write!(f, "not supported extension"),
            CompilationError::DropKeepOutOfBounds => write!(f, "drop keep out of bounds"),
            CompilationError::BranchTableTargetsOutOfBounds => {
                write!(f, "branch table targets are out of bounds")
            }
            CompilationError::MalformedWasmBinary(err) => {
                write!(f, "malformed wasm binary ({})", err)
            }
            CompilationError::NotSupportedImportType => write!(f, "not supported an import type"),
            CompilationError::NotSupportedFuncType => write!(f, "not supported func type"),
            CompilationError::UnresolvedImportFunction => write!(f, "unresolved import function"),
            CompilationError::MalformedImportFunctionType => {
                write!(f, "MalformedImportFunctionType")
            }
            CompilationError::NonDefaultMemoryIndex => write!(f, "non default memory index"),
            CompilationError::ConstEvaluationFailed => write!(f, "const evaluation failed"),
            CompilationError::NotSupportedLocalType => write!(f, "not supported local type"),
            CompilationError::NotSupportedGlobalType => write!(f, "not supported global type"),
            CompilationError::MaxReadonlyDataReached => write!(f, "memory segments overflow"),
            CompilationError::MissingEntrypoint => write!(f, "missing entrypoint"),
            CompilationError::MalformedFuncType => write!(f, "malformed func type"),
            CompilationError::MemoryOutOfBounds => write!(f, "out of bounds memory access"),
            CompilationError::TableOutOfBounds => write!(f, "out of bounds table access"),
        }
    }
}
