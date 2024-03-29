/// This constant is driven by WebAssembly standard, default
/// memory page size is 64kB
pub const N_BYTES_PER_MEMORY_PAGE: u32 = 65536;

/// We have a hard limit for max possible memory used
/// that is equal to ~64mB
pub const N_MAX_MEMORY_PAGES: u32 = 1024;
/// To optimize proving process we have to limit max
/// number of pages, tables, etc. We found 1024 is enough.
pub const N_MAX_TABLES: usize = 1024;
pub const N_MAX_TABLE_ELEMENTS: u32 = 1024;

pub const N_MAX_STACK_HEIGHT: usize = 4096;
pub const N_MAX_RECURSION_DEPTH: usize = 1024;

/// Max possible amount of data segments
pub const N_MAX_DATA_SEGMENTS: usize = 1024;
pub const N_MAX_ELEM_SEGMENTS: usize = 1024;
pub const N_MAX_GLOBALS: usize = 1024;
