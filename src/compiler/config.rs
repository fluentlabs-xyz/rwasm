use crate::{ImportLinker, Opcode, N_DEFAULT_MAX_CODE_LEN, N_DEFAULT_MAX_MEMORY_PAGES};
use alloc::{boxed::Box, sync::Arc};
use wasmparser::WasmFeatures;

#[derive(Debug, Clone)]
/// Configuration for dispatching to different entry functions based on a runtime state value.
/// The router maps state tags to function indices and optionally provides an opcode to compute the tag.
pub struct StateRouterConfig {
    /// List of states to be router based on the state.
    pub states: Box<[(Box<str>, u32)]>,
    /// Instruction that describes how we determine an input state.
    /// Keep it None only if you already have a state element on the stack, because after execution
    /// it's being dropped.
    pub opcode: Option<Opcode>,
}

#[derive(Clone, Debug)]
/// Controls how a Wasm module is lowered into rwasm bytecode.
/// Options affect entry routing, import linking, fuel metering, and validation relaxations for tests.
pub struct CompilationConfig {
    /// State router is used to choose one of the function based on the index provided.
    /// P.S: this flag doesn't work if you have WASM's start entry point.
    pub state_router: Option<StateRouterConfig>,
    /// Entrypoint that stores bytecode for module init.
    /// P.S: this flag doesn't work if you have WASM's start entry point.
    pub entrypoint_name: Option<Box<str>>,
    /// Import linker that stores mapping from function to special identifiers that is used
    /// to remember unique external calls ids. We need this to simplify a proving process to
    /// forward external calls to corresponding circuits.
    pub import_linker: Option<Arc<ImportLinker>>,
    /// An option to disable malformed entrypoint func type check. We need this check for e2e tests
    /// where we manage stack manually.
    ///
    /// WARNING: only for trusted environment, can cause stack overflow/underflow
    pub allow_malformed_entrypoint_func_type: bool,
    /// Should fuel-charging instructions be injected before each builtin call.
    pub builtins_consume_fuel: bool,
    /// We don't support imported global, but you can set a default value for these values instead.
    /// Thus is required by testing suite.
    pub default_imported_global_value: Option<i64>,
    /// Enable fuel metering (always eager mode)
    pub consume_fuel: bool,
    /// Enable replacement with optimized code snippets
    pub code_snippets: bool,
    /// Enable extra dynamic fuel checks for bulk memory/table instructions.
    ///
    /// Secure production configs enable this with fuel metering. Wasmtime
    /// replacement paths must either charge the same dynamic fuel or opt out
    /// explicitly after rejecting the divergent config at a higher layer.
    ///
    /// Note: This flag is not supported by wasmtime; see
    /// [`CompilationConfig::default_strategy_compatible`].
    pub consume_fuel_for_bulk_ops: bool,
    /// Enable fuel metering for params and locals
    ///
    /// Note: This flag is not supported by wasmtime; see
    /// [`CompilationConfig::default_strategy_compatible`].
    pub consume_fuel_for_params_and_locals: bool,
    /// Allow function types with funcref and externref (needed only for e2e testing suite, but
    /// practically inside a blockchain environment it's not possible)
    ///
    /// WARNING: the flag can be removed one funcref/externref type mapping is
    /// implemented for wasmtime
    pub allow_func_ref_function_types: bool,
    /// Allow a start section inside rWasm module. Be aware that a start section is called during resource
    /// init for rWasm VM.
    pub allow_start_section: bool,
    /// The maximum number of memory pages that can be allocated by the module.
    pub max_allowed_memory_pages: u32,
    /// Maximum entries in the Wasm function-type section, including duplicates (default: 4096).
    /// Checked before type validation allocates memory for the declared count. Every declared
    /// type costs constant work in the compiler, so raising this limit scales compile time
    /// linearly; coordinate it with the host's compilation budget. It does not change emitted
    /// bytecode.
    pub max_allowed_function_types: u32,
    /// Maximum number of instructions in the compiled module (default:
    /// [`N_DEFAULT_MAX_CODE_LEN`]). Checked while code is emitted, so a module that would exceed
    /// it is rejected with [`crate::CompilationError::CodeSizeExceeded`] before the excess is
    /// allocated. It does not change emitted bytecode; it decides which inputs compile at all.
    pub max_code_len: u32,
}

/// The default config maximizes rwasm-side metering: it enables
/// `consume_fuel_for_bulk_ops` and `consume_fuel_for_params_and_locals`, which only the rwasm
/// translator implements. Use [`CompilationConfig::default_strategy_compatible`] when the module
/// may execute on either strategy and fuel accounting must not depend on which one was picked.
impl Default for CompilationConfig {
    fn default() -> Self {
        Self {
            state_router: None,
            entrypoint_name: None,
            import_linker: None,
            allow_malformed_entrypoint_func_type: false,
            builtins_consume_fuel: false,
            default_imported_global_value: None,
            consume_fuel: true,
            consume_fuel_for_bulk_ops: true,
            consume_fuel_for_params_and_locals: true,
            code_snippets: true,
            allow_func_ref_function_types: false,
            allow_start_section: false,
            max_allowed_memory_pages: N_DEFAULT_MAX_MEMORY_PAGES,
            max_allowed_function_types: 4096,
            max_code_len: N_DEFAULT_MAX_CODE_LEN,
        }
    }
}

impl CompilationConfig {
    /// Like [`CompilationConfig::default`], but with identical fuel semantics on the rwasm and
    /// Wasmtime strategies.
    ///
    /// The plain default enables two fuel injections that only the rwasm translator implements:
    /// dynamic bulk-op fuel (`consume_fuel_for_bulk_ops`) and params/locals fuel
    /// (`consume_fuel_for_params_and_locals`). The Wasmtime engine ignores both — its
    /// `rwasm-fuel-policy` schedule charges bulk ops a flat entity cost and charges nothing for
    /// locals — so under [`crate::StrategyDefinition::new`] the same module burns different fuel
    /// depending on which strategy the crate was built with. This constructor disables the
    /// rwasm-only injections so both strategies charge from the same schedule.
    ///
    /// Use this whenever the produced module may run on either strategy; use
    /// [`CompilationConfig::default`] when execution is pinned to the rwasm VM and the extra
    /// metering is wanted.
    ///
    /// **Not for untrusted code.** Without `consume_fuel_for_bulk_ops` a bulk memory or table
    /// operation costs a flat entity cost however much it touches — 64 MiB of `memory.fill` for
    /// a handful of fuel — on both engines, so a guest can buy unbounded host work per fuel unit.
    /// Untrusted Wasm belongs on the rwasm VM with [`CompilationConfig::default`]; the Wasmtime
    /// strategy is for trusted (system) code until the engine meters bulk operations itself
    /// (<https://github.com/fluentlabs-xyz/wasmtime/pull/12>).
    pub fn default_strategy_compatible() -> Self {
        Self {
            consume_fuel_for_bulk_ops: false,
            consume_fuel_for_params_and_locals: false,
            ..Self::default()
        }
    }

    /// Returns `true` if this config charges the same fuel on the rwasm and Wasmtime strategies.
    ///
    /// See [`CompilationConfig::default_strategy_compatible`] for which flags diverge.
    /// [`crate::StrategyDefinition::new`], [`crate::StrategyDefinition::new_as_wasmtime`] and
    /// [`crate::for_each_strategy`] reject a config for which this returns `false`; only
    /// [`crate::StrategyDefinition::new_as_rwasm`] and [`crate::RwasmModule::compile`] accept the
    /// rwasm-only injections, since the rwasm VM is the one strategy that implements them.
    pub fn is_strategy_compatible(&self) -> bool {
        !self.consume_fuel_for_bulk_ops && !self.consume_fuel_for_params_and_locals
    }

    /// Returns the WebAssembly features the validator accepts.
    ///
    /// The set is an explicit union of flags, never derived from `WasmFeatures::default()`.
    /// The validator decides which language the compiler is handed, and the translator
    /// implements a strictly smaller set: an operator that validates but has no translation
    /// would leave the emulated value stack out of sync with the emitted code, which yields
    /// wrong `DropKeep` amounts and `local.get`/`local.set` depths rather than an error. With an
    /// explicit union, a `wasmparser` upgrade that adds a proposal or turns one on by default
    /// leaves it disabled here; a proposal is enabled by adding it to this list, to the operator
    /// gate in `FuncBuilder` and to the Wasmtime engine configuration together.
    /// `tests/wasm_features.rs` pins the set.
    pub fn wasm_features(&self) -> WasmFeatures {
        // Proposals the translator implements. `REFERENCE_TYPES` carries the overlong
        // `call_indirect` table-index encoding (`CALL_INDIRECT_OVERLONG`) and `BULK_MEMORY` the
        // `memory.copy`/`memory.fill` subset (`BULK_MEMORY_OPT`), as in wasmparser's own sets.
        WasmFeatures::MUTABLE_GLOBAL
            | WasmFeatures::SATURATING_FLOAT_TO_INT
            | WasmFeatures::SIGN_EXTENSION
            | WasmFeatures::MULTI_VALUE
            | WasmFeatures::BULK_MEMORY
            | WasmFeatures::REFERENCE_TYPES
            | WasmFeatures::TAIL_CALL
            | WasmFeatures::EXTENDED_CONST
            // `i64.add128`, `i64.sub128`, `i64.mul_wide_s` and `i64.mul_wide_u` lower to the
            // `I64Add128`, `I64Sub128`, `I64MulWideS` and `I64MulWideU` opcodes
            | WasmFeatures::WIDE_ARITHMETIC
            // Not proposals: floats are translated, and `GC_TYPES` is the validator's gate for
            // `externref` (GC instructions and types stay behind `GC`, which is off).
            | WasmFeatures::FLOATS
            | WasmFeatures::GC_TYPES
        // Off: `SIMD`, `RELAXED_SIMD`, `THREADS`, `SHARED_EVERYTHING_THREADS`, `MULTI_MEMORY`,
        // `MEMORY64`, `EXCEPTIONS`, `LEGACY_EXCEPTIONS`, `COMPONENT_MODEL` and its `CM_*`
        // sub-features, `FUNCTION_REFERENCES`, `GC`, `CUSTOM_DESCRIPTORS`, `MEMORY_CONTROL`,
        // `CUSTOM_PAGE_SIZES`, `COMPACT_IMPORTS` and `STACK_SWITCHING`.
    }

    pub fn with_state_router(mut self, state_router: StateRouterConfig) -> Self {
        self.state_router = Some(state_router);
        self
    }

    pub fn with_entrypoint_name(mut self, name: Box<str>) -> Self {
        self.entrypoint_name = Some(name);
        self
    }

    pub fn with_import_linker(mut self, import_linker: Arc<ImportLinker>) -> Self {
        self.import_linker = Some(import_linker);
        self
    }

    pub fn with_allow_malformed_entrypoint_func_type(
        mut self,
        allow_malformed_entrypoint_func_type: bool,
    ) -> Self {
        self.allow_malformed_entrypoint_func_type = allow_malformed_entrypoint_func_type;
        self
    }

    pub fn with_builtins_consume_fuel(mut self, builtins_consume_fuel: bool) -> Self {
        self.builtins_consume_fuel = builtins_consume_fuel;
        self
    }

    pub fn with_default_imported_global_value(
        mut self,
        default_imported_global_value: i64,
    ) -> Self {
        self.default_imported_global_value = Some(default_imported_global_value);
        self
    }

    pub fn with_consume_fuel(mut self, consume_fuel: bool) -> Self {
        self.consume_fuel = consume_fuel;
        if !consume_fuel {
            self.consume_fuel_for_bulk_ops = false;
            self.consume_fuel_for_params_and_locals = false;
            self.builtins_consume_fuel = false;
        }
        self
    }

    pub fn with_consume_fuel_for_bulk_ops(mut self, v: bool) -> Self {
        self.consume_fuel_for_bulk_ops = self.consume_fuel && v;
        self
    }

    pub fn with_consume_fuel_for_params_and_locals(mut self, v: bool) -> Self {
        self.consume_fuel_for_params_and_locals = v;
        self
    }

    pub fn with_code_snippets(mut self, v: bool) -> Self {
        self.code_snippets = v;
        self
    }

    pub fn with_allow_func_ref_function_types(
        mut self,
        allow_func_ref_function_types: bool,
    ) -> Self {
        self.allow_func_ref_function_types = allow_func_ref_function_types;
        self
    }

    pub fn with_allow_start_section(mut self, allow_start_section: bool) -> Self {
        self.allow_start_section = allow_start_section;
        self
    }

    pub fn with_max_allowed_memory_pages(mut self, max_allowed_memory_pages: u32) -> Self {
        self.max_allowed_memory_pages = max_allowed_memory_pages;
        self
    }

    pub fn with_max_allowed_function_types(mut self, max_allowed_function_types: u32) -> Self {
        self.max_allowed_function_types = max_allowed_function_types;
        self
    }

    pub fn with_max_code_len(mut self, max_code_len: u32) -> Self {
        self.max_code_len = max_code_len;
        self
    }
}
