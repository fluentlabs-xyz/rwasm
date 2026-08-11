mod branch_offset;
pub(crate) mod codec;
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

/// A default number of memory pages 1024 pages (64mB)
pub const N_DEFAULT_MAX_MEMORY_PAGES: u32 = 1024;

/// A hard limit on the maximum number of memory pages that can be allocated.
/// This value is driven from a Wasm standard, the maximum number of memory pages is 32,768.
///
/// # Safety
///
/// The compiler-injected prologues for `memory.grow`/`memory.init` (see [`crate::InstructionSet`]
/// in `src/isa/memory.rs`) compare `size + delta` against this limit with a *signed* `i32.gt_s`
/// and compute fuel with a *wrapping* `i32.add` round-up. Both can be defeated by an operand
/// close to `i32::MAX`/`u32::MAX`: the sum wraps negative and passes the guard, and the fuel
/// round-up wraps down to (nearly) zero fuel.
///
/// This is currently not exploitable only because such inputs always trap on the runtime bounds
/// check that runs behind the guard: `32768 * 65536` fits in `u32`, so no reachable page count
/// can make the wrapped path touch memory or skip metering for work actually performed.
/// Raising this constant, or otherwise letting the guards see values that overflow `i32`, would
/// turn those prologues into a real bounds/metering bypass with no visible change to this code.
/// Change it only together with switching the guards to unsigned comparisons and overflow-safe
/// fuel arithmetic.
pub const N_MAX_ALLOWED_MEMORY_PAGES: u32 = 32768;

/// A default memory index in a Wasm binary.
/// According to Wasm validation rules, this value is always 0,
/// since Wasm doesn't support multiple memory segments yet
pub const DEFAULT_MEMORY_INDEX: u32 = 0;

pub const N_MAX_DATA_SEGMENTS: usize = 100_000;
pub const N_MAX_ELEM_SEGMENTS: usize = 100_000;

pub const N_MAX_DATA_SEGMENTS_BITS: usize =
    N_MAX_DATA_SEGMENTS.div_ceil(usize::BITS as usize);
pub const N_MAX_ELEM_SEGMENTS_BITS: usize =
    N_MAX_ELEM_SEGMENTS.div_ceil(usize::BITS as usize);

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
///
/// # Safety
///
/// The same caveat as for [`N_MAX_ALLOWED_MEMORY_PAGES`] applies: the injected prologues for
/// `table.grow`/`table.init` (`src/isa/table.rs`) use signed `i32.gt_s` on `size + delta` and a
/// wrapping `i32.add` for the fuel round-up. With this limit the guarded sums stay far below
/// `i32::MAX`, so the signedness and the wrap are unobservable, and any input large enough to
/// wrap traps on the runtime table bounds check anyway. Raising this constant to anything near
/// `i32::MAX` requires switching those guards to unsigned comparisons and overflow-safe fuel
/// arithmetic first.
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
pub type NumLocals = u32;
