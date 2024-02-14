use crate::BinaryFormatError;

#[derive(Debug)]
pub enum ReducedModuleError {
    MissingEntrypoint,
    NotSupportedOpcode,
    NotSupportedImport,
    NotSupportedMemory(&'static str),
    ParseError(&'static str),
    OutOfBuffer,
    ReachedUnreachable,
    IllegalOpcode(u8),
    ImpossibleJump,
    InternalError(&'static str),
    MemoryOverflow,
    EmptyBytecode,
    BinaryFormat(BinaryFormatError),
}

/// This constant is driven by WebAssembly standard, default
/// memory page size is 64kB
pub const N_BYTES_PER_MEMORY_PAGE: u32 = 65536;

/// We have a hard limit for max possible memory used
/// that is equal to ~64mB
pub const N_MAX_MEMORY_PAGES: u32 = 1024;

/// To optimize proving process we have to limit max
/// number of pages, tables, etc. We found 1024 is enough.
pub const N_MAX_TABLES: u32 = 1024;
