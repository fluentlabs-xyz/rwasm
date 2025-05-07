use wasmparser::BinaryReaderError;

#[derive(Debug)]
pub enum CompilerError {
    BinaryReaderError(BinaryReaderError),
}

impl From<BinaryReaderError> for CompilerError {
    fn from(value: BinaryReaderError) -> Self {
        Self::BinaryReaderError(value)
    }
}
