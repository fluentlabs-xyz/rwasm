use crate::module::ImportName;
use core::{fmt, fmt::Display};

/// This constant is driven by WebAssembly standard, default
/// memory page size is 64kB
pub const N_BYTES_PER_MEMORY_PAGE: u32 = 65536;

/// We have a hard limit for max possible memory used
/// that is equal to ~64mB
pub const N_MAX_MEMORY_PAGES: u32 = 1024;

/// To optimize proving process we have to limit max
/// number of pages, tables, etc. We found 1024 is enough.
pub const N_MAX_TABLES: u32 = 1024;

pub const N_MAX_STACK_HEIGHT: usize = 4096;
pub const N_MAX_RECURSION_DEPTH: usize = 1024;

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
