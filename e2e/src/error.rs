use rwasm::RwasmError;
use std::{error::Error, fmt, fmt::Display};

/// Errors that may occur upon Wasm spec test suite execution.
#[derive(Debug)]
pub enum TestError {
    InstanceNotRegistered { name: String },
    NoModuleInstancesFound,
    // An error that happens with rWasm
    Rwasm(RwasmError),
}

impl Error for TestError {}

impl Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InstanceNotRegistered { name } => {
                write!(f, "missing module instance with name: {name}")
            }
            Self::NoModuleInstancesFound => {
                write!(f, "found no module instances registered so far")
            }
            Self::Rwasm(rwasm_error) => Display::fmt(rwasm_error, f),
        }
    }
}

impl<E> From<E> for TestError
where
    E: Into<RwasmError>,
{
    fn from(error: E) -> Self {
        Self::Rwasm(error.into())
    }
}
