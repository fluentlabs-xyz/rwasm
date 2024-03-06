use crate::module::ImportName;
use core::{fmt, fmt::Display};

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
