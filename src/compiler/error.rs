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
    NotSupportedExportType,
    UnresolvedImportFunction,
    MalformedImportFunctionType,
    NonDefaultMemoryIndex,
    ConstEvaluationFailed,
    NotSupportedLocalType,
    NotSupportedGlobalType,
    MemorySegmentsOverflow,
    MissingEntrypoint,
    MalformedFuncType,
}

impl From<BinaryReaderError> for CompilationError {
    fn from(err: BinaryReaderError) -> Self {
        CompilationError::MalformedWasmBinary(err)
    }
}
