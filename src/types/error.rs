use crate::{types::HostError, CompilationError, TrapCode};
use alloc::boxed::Box;
use core::fmt::Formatter;

#[derive(Debug)]
pub enum RwasmError {
    CompilationError(CompilationError),
    TrapCode(TrapCode),
    HostInterruption(Box<dyn HostError>),
}

impl From<CompilationError> for RwasmError {
    fn from(err: CompilationError) -> Self {
        RwasmError::CompilationError(err)
    }
}
impl From<TrapCode> for RwasmError {
    fn from(err: TrapCode) -> Self {
        RwasmError::TrapCode(err)
    }
}

impl core::fmt::Display for RwasmError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            RwasmError::CompilationError(err) => write!(f, "{}", err),
            RwasmError::TrapCode(err) => write!(f, "{}", err),
            RwasmError::HostInterruption(_) => write!(f, "host interruption"),
        }
    }
}
