mod block_fuel;
mod branch_offset;
mod compiled_expr;
mod constructor_params;
mod error;
mod func_ref;
mod global_variable;
mod host_error;
mod import_linker;
mod import_name;
mod instruction;
mod instruction_set;
mod module;
mod nan_preserving_float;
mod trap_code;
mod units;
mod untyped_value;
mod value;

pub const N_DEFAULT_STACK_SIZE: usize = 4096;
pub const N_MAX_STACK_SIZE: usize = 4096;
pub const N_MAX_TABLE_SIZE: usize = 100;
pub const N_MAX_RECURSION_DEPTH: usize = 1024;

pub const N_MAX_DATA_SEGMENTS: usize = 1024;
pub const N_MAX_TABLE_ELEMENTS: usize = 1024;

pub const DEFAULT_MIN_VALUE_STACK_HEIGHT: usize = 1024;
pub const DEFAULT_MAX_VALUE_STACK_HEIGHT: usize = 1024;

/// This constant is driven by WebAssembly standard, default
/// memory page size is 64kB
pub const N_BYTES_PER_MEMORY_PAGE: u32 = 65536;

/// We have a hard limit for max possible memory used
/// that is equal to ~64mB
#[cfg(not(feature = "more-max-pages"))]
pub const N_MAX_MEMORY_PAGES: u32 = 1024;
/// Increased value needed for SVM for now
#[cfg(feature = "more-max-pages")]
pub const N_MAX_MEMORY_PAGES: u32 = 2048;

/// To optimize a proving process, we have to limit the max
/// number of pages, tables, etc. We found 1024 is enough.
pub const N_MAX_TABLES: u32 = 1024;

pub const N_MAX_STACK_HEIGHT: usize = 4096;

pub const DEFAULT_MEMORY_INDEX: u32 = 0;

pub const NULL_FUNC_IDX: u32 = 0u32;

pub type FuncTypeIdx = u32;
pub type SignatureIdx = u32;
pub type MemoryIdx = u32;
pub type GlobalIdx = u32;
pub type TableIdx = u32;
pub type FuncIdx = u32;
pub type DataSegmentIdx = u32;
pub type ElementSegmentIdx = u32;
pub type CompiledFunc = u32;
pub type LocalDepth = u32;
pub type BranchTableTargets = u32;
pub type MaxStackHeight = u32;
pub type SysFuncIdx = u32;
pub type AddressOffset = u32;

pub use block_fuel::*;
pub use branch_offset::*;
pub use compiled_expr::*;
pub use constructor_params::*;
pub use error::*;
pub use func_ref::*;
pub use global_variable::*;
pub use host_error::*;
pub use import_linker::*;
pub use import_name::*;
pub use instruction::*;
pub use instruction_set::*;
pub use module::*;
pub use nan_preserving_float::*;
pub use trap_code::*;
pub use units::*;
pub use untyped_value::*;
pub use value::*;
