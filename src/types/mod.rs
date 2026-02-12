mod branch_offset;
mod constructor_params;
mod error;
mod func_ref;
mod global_variable;
mod hint_type;
mod host_error;
mod import_name;
mod nan_preserving_float;
mod opcode;
mod trap_code;
mod units;
mod untyped_value;
mod value;

pub use branch_offset::*;
pub use constructor_params::*;
pub use error::*;
pub use func_ref::*;
pub use global_variable::*;
pub use hint_type::*;
pub use host_error::*;
pub use import_name::*;
pub use nan_preserving_float::*;
pub use opcode::*;
pub use trap_code::*;
pub use units::*;
pub use untyped_value::*;
pub use value::*;

/// A default stack size we use for stack allocation.
///
/// This value can't be less than 6, because 4 elements we need for an entrypoint and 1 element
/// we need for running e2e testing suite where one parameter can be passed into the test.
///
/// We keep value 32 since it's the most optimal.
pub const N_DEFAULT_STACK_SIZE: usize = 32;
pub const N_MAX_STACK_SIZE: usize = 8192;
pub const N_MAX_RECURSION_DEPTH: usize = 1024;

/// This constant is driven by WebAssembly standard, default
/// memory page size is 64kB
pub const N_BYTES_PER_MEMORY_PAGE: u32 = 65536;

/// We have a hard limit for max possible memory used
/// that is equal to 1024 pages (64mB)
///
/// TODO(dmitry): "should we revisit the limit?"
///
/// For SVM runtime we temporarily increase up to 128mB
#[cfg(not(feature = "more-max-pages"))]
pub const N_MAX_MEMORY_PAGES: u32 = 1024;
#[cfg(feature = "more-max-pages")]
pub const N_MAX_MEMORY_PAGES: u32 = 1024 * 10;

/// A default memory index in a Wasm binary.
/// According to Wasm validation rules, this value is always 0,
/// since Wasm doesn't support multiple memory segments yet
pub const DEFAULT_MEMORY_INDEX: u32 = 0;

pub const N_MAX_DATA_SEGMENTS: usize = 100_000;
pub const N_MAX_ELEM_SEGMENTS: usize = 100_000;

pub const N_MAX_DATA_SEGMENTS_BITS: usize =
    (N_MAX_DATA_SEGMENTS + usize::BITS as usize - 1) / usize::BITS as usize;
pub const N_MAX_ELEM_SEGMENTS_BITS: usize =
    (N_MAX_ELEM_SEGMENTS + usize::BITS as usize - 1) / usize::BITS as usize;

/// For null RefFunc/ExternRef types we use 0. We can do this
/// because 0 offset is reserved under an entrypoint that can't be re-called
pub const NULL_FUNC_IDX: u32 = 0u32;

/// Placeholder for the function index of a snippet.
/// The actual index is resolved in later compilation stages
/// once the snippet's final location is known.
pub const SNIPPET_FUNC_IDX_UNRESOLVED: u32 = u32::MAX;

/// That maximum possible number of tables allowed, the limited is driven from Wasm standards
pub const N_MAX_TABLES: u32 = 100;

/// The maximum limit of elements in total can be fit into one table.
/// It means in total you can have `100*1024=102_400` elements.
///
/// The original standard allows `100_000` element segments with an unlimited number of elements
/// inside.
pub const N_MAX_TABLE_SIZE: u32 = 1024;

pub type InstrLoc = u32;
pub type LabelRef = u32;
pub type FuncTypeIdx = u32;
pub type SignatureIdx = u32;
pub type MemoryIdx = u32;
pub type GlobalIdx = u32;
/// Max table size can't exceed 100 elements, so it easily fits into u16
pub type TableIdx = u16;
pub type FuncIdx = u32;
pub type DataSegmentIdx = u32;
pub type ElementSegmentIdx = u32;
pub type CompiledFunc = u32;
pub type LocalDepth = u32;
pub type BranchTableTargets = u32;
pub type MaxStackHeight = u32;
pub type SysFuncIdx = u32;
pub type AddressOffset = u32;
pub type BlockFuel = u32;
