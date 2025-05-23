use super::{stack::StackLimits, DropKeep};
use crate::{
    core::{ImportLinker, UntypedValue},
    engine::bytecode::Instruction,
};
use alloc::{
    boxed::Box,
    string::{String, ToString},
};
use core::{mem::size_of, num::NonZeroU64};
use wasmparser::WasmFeatures;

/// The default number of stacks kept in the cache at most.
const DEFAULT_CACHED_STACKS: usize = 2;

#[derive(Debug, Clone)]
pub struct StateRouterConfig {
    /// List of states to be router based on the state
    pub states: Box<[(String, u32)]>,
    /// Instruction that describes how we determine an input state
    pub opcode: Instruction,
}

#[derive(Debug, Clone)]
pub struct RwasmConfig {
    /// State router is used to choose one of the function based on the index provided.
    /// P.S: this flag doesn't work if you have WASM's start entry point
    pub state_router: Option<StateRouterConfig>,
    /// Entrypoint that stores bytecode for module init
    /// P.S: this flag doesn't work if you have WASM's start entry point
    pub entrypoint_name: Option<String>,
    /// Import linker that stores mapping from function to special identifiers that is used
    /// to remember unique external calls ids. We need this to simplify a proving process to
    /// forward external calls to corresponding circuits.
    pub import_linker: Option<ImportLinker>,
    /// Do we need to wrap input functions to convert them from ExternRef to FuncRef (we need it to
    /// simplify tables sometimes)? Its needed only for rWASM mode where we replace all external
    /// calls with import linker mapping.
    pub wrap_import_functions: bool,
    /// An option for translating a drop keeps into SetLocal/GetLocal opcodes,
    /// right now under a flag because the function is unstable
    pub translate_drop_keep: bool,
    /// An option to disable malformed entrypoint func type check. We need this check for e2e tests
    /// where we manage stack manually.
    pub allow_malformed_entrypoint_func_type: bool,
    /// A mode for 32-bit stack alignment
    /// that disables all 64-bit instructions and replace them with 32-bit ones
    pub use_32bit_mode: bool,
    /// Should fuel-charging instructions be injected before each builtin call
    pub builtins_consume_fuel: bool,
}

impl Default for RwasmConfig {
    fn default() -> Self {
        Self {
            state_router: None,
            entrypoint_name: Some("main".to_string()),
            import_linker: None,
            wrap_import_functions: false,
            translate_drop_keep: false,
            allow_malformed_entrypoint_func_type: false,
            use_32bit_mode: false,
            builtins_consume_fuel: true,
        }
    }
}

impl RwasmConfig {
    pub fn with_state_router(mut self, state_router_config: StateRouterConfig) -> Self {
        self.state_router = Some(state_router_config);
        self
    }

    pub fn with_entrypoint_name(mut self, entrypoint_name: String) -> Self {
        self.entrypoint_name = Some(entrypoint_name);
        self
    }

    pub fn with_import_linker(mut self, linker: ImportLinker) -> Self {
        self.import_linker = Some(linker);
        self
    }

    pub fn with_wrap_import_functions(mut self, wrap_import_functions: bool) -> Self {
        self.wrap_import_functions = wrap_import_functions;
        self
    }

    pub fn with_translate_drop_keep(mut self, translate_drop_keep: bool) -> Self {
        self.translate_drop_keep = translate_drop_keep;
        self
    }

    pub fn with_allow_malformed_entrypoint_func_type(mut self) -> Self {
        self.allow_malformed_entrypoint_func_type = true;
        self
    }

    pub fn with_use_32bit_mode(mut self) -> Self {
        self.use_32bit_mode = true;
        self
    }

    pub fn with_builtins_consume_fuel(mut self, builtins_consume_fuel: bool) -> Self {
        self.builtins_consume_fuel = builtins_consume_fuel;
        self
    }
}

/// Configuration for an [`Engine`].
///
/// [`Engine`]: [`crate::Engine`]
#[derive(Debug, Clone)]
pub struct Config {
    /// The limits set on the value stack and call stack.
    stack_limits: StackLimits,
    /// The amount of Wasm stacks to keep in cache at most.
    cached_stacks: usize,
    /// Is `true` if the `mutable-global` Wasm proposal is enabled.
    mutable_global: bool,
    /// Is `true` if the `sign-extension` Wasm proposal is enabled.
    sign_extension: bool,
    /// Is `true` if the `saturating-float-to-int` Wasm proposal is enabled.
    saturating_float_to_int: bool,
    /// Is `true` if the [`multi-value`] Wasm proposal is enabled.
    multi_value: bool,
    /// Is `true` if the [`bulk-memory`] Wasm proposal is enabled.
    bulk_memory: bool,
    /// Is `true` if the [`reference-types`] Wasm proposal is enabled.
    reference_types: bool,
    /// Is `true` if the [`tail-call`] Wasm proposal is enabled.
    tail_call: bool,
    /// Is `true` if the [`extended-const`] Wasm proposal is enabled.
    extended_const: bool,
    /// Is `true` if Wasm instructions on `f32` and `f64` types are allowed.
    floats: bool,
    /// Is `true` if `wasmi` executions shall consume fuel.
    consume_fuel: bool,
    /// The fuel consumption mode of the `wasmi` [`Engine`](crate::Engine).
    fuel_consumption_mode: FuelConsumptionMode,
    /// The configured fuel costs of all `wasmi` bytecode instructions.
    fuel_costs: FuelCosts,
    /// Translate into rWASM compatible binary
    rwasm_config: Option<RwasmConfig>,
}

/// The fuel consumption mode of the `wasmi` [`Engine`].
///
/// This mode affects when fuel is charged for Wasm bulk-operations.
/// Affected Wasm instructions are:
///
/// - `memory.{grow, copy, fill}`
/// - `data.init`
/// - `table.{grow, copy, fill}`
/// - `element.init`
///
/// The default fuel consumption mode is [`FuelConsumptionMode::Lazy`].
///
/// [`Engine`]: crate::Engine
#[derive(Debug, Default, Copy, Clone)]
pub enum FuelConsumptionMode {
    /// Fuel consumption for bulk-operations is lazy.
    ///
    /// Lazy fuel consumption means that fuel for bulk-operations
    /// is checked before executing the instruction but only consumed
    /// if the executed instruction suceeded. The reason for this is
    /// that bulk-operations fail fast and therefore do not cost
    /// a lot of compute power in case of failure.
    ///
    /// # Note
    ///
    /// Lazy fuel consumption makes sense as default mode since the
    /// affected bulk-operations usually are very costly if they are
    /// successful. Therefore users generally want to avoid having to
    /// using more fuel than what was actually used, especially if there
    /// is an underlying cost model associated to the used fuel.
    #[default]
    Lazy,
    /// Fuel consumption for bulk-operations is eager.
    ///
    /// Eager fuel consumption means that fuel for bulk-operations
    /// is always consumed before executing the instruction independent
    /// of it suceeding or failing.
    ///
    /// # Note
    ///
    /// A use case for when a user might prefer eager fuel consumption
    /// is when the fuel **required** to perform an execution should be identical
    /// to the actual fuel **consumed** by an execution. Otherwise it can be confusing
    /// that the execution consumed `x` gas while it needs `x + gas_for_bulk_op` to
    /// not run out of fuel.
    Eager,
}

/// Type storing all kinds of fuel costs of instructions.
#[derive(Debug, Copy, Clone)]
pub struct FuelCosts {
    /// The base fuel costs for all instructions.
    pub base: u64,
    /// The fuel cost for instruction operating on Wasm entities.
    ///
    /// # Note
    ///
    /// A Wasm entitiy is one of `func`, `global`, `memory` or `table`.
    /// Those instructions are usually a bit more costly since they need
    /// multiplie indirect accesses through the Wasm instance and store.
    pub entity: u64,
    /// The fuel cost offset for `memory.load` instructions.
    pub load: u64,
    /// The fuel cost offset for `memory.store` instructions.
    pub store: u64,
    /// The fuel cost offset for `call` and `call_indirect` instructions.
    pub call: u64,
    /// Determines how many moved stack values consume one fuel upon a branch or return
    /// instruction.
    ///
    /// # Note
    ///
    /// If this is zero then processing [`DropKeep`] costs nothing.
    pub branch_kept_per_fuel: u64,
    /// Determines how many function locals consume one fuel per function call.
    ///
    /// # Note
    ///
    /// - This is also applied to all function parameters since they are translated to local
    ///   variable slots.
    /// - If this is zero then processing function locals costs nothing.
    pub func_locals_per_fuel: u64,
    /// How many memory bytes can be processed per fuel in a `bulk-memory` instruction.
    ///
    /// # Note
    ///
    /// If this is zero then processing memory bytes costs nothing.
    pub memory_bytes_per_fuel: u64,
    /// How many table elements can be processed per fuel in a `bulk-table` instruction.
    ///
    /// # Note
    ///
    /// If this is zero then processing table elements costs nothing.
    pub table_elements_per_fuel: u64,
}

impl FuelCosts {
    /// Returns the fuel consumption of the amount of items with costs per items.
    fn costs_per(len_items: u64, items_per_fuel: u64) -> u64 {
        NonZeroU64::new(items_per_fuel)
            .map(|items_per_fuel| len_items / items_per_fuel)
            .unwrap_or(0)
    }

    /// Returns the fuel consumption for branches and returns using the given [`DropKeep`].
    pub fn fuel_for_drop_keep(&self, drop_keep: DropKeep) -> u64 {
        if drop_keep.drop() == 0 {
            return 0;
        }
        Self::costs_per(u64::from(drop_keep.keep()), self.branch_kept_per_fuel)
    }

    /// Returns the fuel consumption for calling a function with the amount of local variables.
    ///
    /// # Note
    ///
    /// Function parameters are also treated as local variables.
    pub fn fuel_for_locals(&self, locals: u64) -> u64 {
        Self::costs_per(locals, self.func_locals_per_fuel)
    }

    /// Returns the fuel consumption for processing the amount of memory bytes.
    pub fn fuel_for_bytes(&self, bytes: u64) -> u64 {
        Self::costs_per(bytes, self.memory_bytes_per_fuel)
    }

    /// Returns the fuel consumption for processing the amount of table elements.
    pub fn fuel_for_elements(&self, elements: u64) -> u64 {
        Self::costs_per(elements, self.table_elements_per_fuel)
    }
}

impl Default for FuelCosts {
    fn default() -> Self {
        let memory_bytes_per_fuel = 64;
        let bytes_per_register = size_of::<UntypedValue>() as u64;
        let registers_per_fuel = memory_bytes_per_fuel / bytes_per_register;
        Self {
            base: 1,
            entity: 1,
            load: 1,
            store: 1,
            call: 1,
            func_locals_per_fuel: registers_per_fuel,
            branch_kept_per_fuel: registers_per_fuel,
            memory_bytes_per_fuel,
            table_elements_per_fuel: registers_per_fuel,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            stack_limits: StackLimits::default(),
            cached_stacks: DEFAULT_CACHED_STACKS,
            mutable_global: true,
            sign_extension: true,
            saturating_float_to_int: true,
            multi_value: true,
            bulk_memory: true,
            reference_types: true,
            tail_call: false,
            extended_const: false,
            floats: true,
            consume_fuel: false,
            fuel_costs: FuelCosts::default(),
            fuel_consumption_mode: FuelConsumptionMode::default(),
            rwasm_config: None,
        }
    }
}

impl Config {
    /// Sets the [`StackLimits`] for the [`Config`].
    pub fn set_stack_limits(&mut self, stack_limits: StackLimits) -> &mut Self {
        self.stack_limits = stack_limits;
        self
    }

    /// Returns the [`StackLimits`] of the [`Config`].
    pub(super) fn stack_limits(&self) -> StackLimits {
        self.stack_limits
    }

    /// Sets the maximum amount of cached stacks for reuse for the [`Config`].
    ///
    /// # Note
    ///
    /// Defaults to 2.
    pub fn set_cached_stacks(&mut self, amount: usize) -> &mut Self {
        self.cached_stacks = amount;
        self
    }

    /// Returns the maximum amount of cached stacks for reuse of the [`Config`].
    pub(super) fn cached_stacks(&self) -> usize {
        self.cached_stacks
    }

    /// Enable or disable the [`mutable-global`] Wasm proposal for the [`Config`].
    ///
    /// # Note
    ///
    /// Enabled by default.
    ///
    /// [`mutable-global`]: https://github.com/WebAssembly/mutable-global
    pub fn wasm_mutable_global(&mut self, enable: bool) -> &mut Self {
        self.mutable_global = enable;
        self
    }

    /// Enable or disable the [`sign-extension`] Wasm proposal for the [`Config`].
    ///
    /// # Note
    ///
    /// Enabled by default.
    ///
    /// [`sign-extension`]: https://github.com/WebAssembly/sign-extension-ops
    pub fn wasm_sign_extension(&mut self, enable: bool) -> &mut Self {
        self.sign_extension = enable;
        self
    }

    /// Enable or disable the [`saturating-float-to-int`] Wasm proposal for the [`Config`].
    ///
    /// # Note
    ///
    /// Enabled by default.
    ///
    /// [`saturating-float-to-int`]:
    /// https://github.com/WebAssembly/nontrapping-float-to-int-conversions
    pub fn wasm_saturating_float_to_int(&mut self, enable: bool) -> &mut Self {
        self.saturating_float_to_int = enable;
        self
    }

    /// Enable or disable the [`multi-value`] Wasm proposal for the [`Config`].
    ///
    /// # Note
    ///
    /// Enabled by default.
    ///
    /// [`multi-value`]: https://github.com/WebAssembly/multi-value
    pub fn wasm_multi_value(&mut self, enable: bool) -> &mut Self {
        self.multi_value = enable;
        self
    }

    /// Enable or disable the [`bulk-memory`] Wasm proposal for the [`Config`].
    ///
    /// # Note
    ///
    /// Enabled by default.
    ///
    /// [`bulk-memory`]: https://github.com/WebAssembly/bulk-memory-operations
    pub fn wasm_bulk_memory(&mut self, enable: bool) -> &mut Self {
        self.bulk_memory = enable;
        self
    }

    /// Enable or disable the [`reference-types`] Wasm proposal for the [`Config`].
    ///
    /// # Note
    ///
    /// Enabled by default.
    ///
    /// [`reference-types`]: https://github.com/WebAssembly/reference-types
    pub fn wasm_reference_types(&mut self, enable: bool) -> &mut Self {
        self.reference_types = enable;
        self
    }

    /// Enable or disable the [`tail-call`] Wasm proposal for the [`Config`].
    ///
    /// # Note
    ///
    /// Disabled by default.
    ///
    /// [`tail-call`]: https://github.com/WebAssembly/tail-calls
    pub fn wasm_tail_call(&mut self, enable: bool) -> &mut Self {
        self.tail_call = enable;
        self
    }

    /// Enable or disable the [`extended-const`] Wasm proposal for the [`Config`].
    ///
    /// # Note
    ///
    /// Disabled by default.
    ///
    /// [`tail-call`]: https://github.com/WebAssembly/extended-const
    pub fn wasm_extended_const(&mut self, enable: bool) -> &mut Self {
        self.extended_const = enable;
        self
    }

    /// Enable or disable Wasm floating point (`f32` and `f64`) instructions and types.
    ///
    /// Enabled by default.
    pub fn floats(&mut self, enable: bool) -> &mut Self {
        self.floats = enable;
        self
    }

    /// Configures whether `wasmi` will consume fuel during execution to either halt execution as
    /// desired.
    ///
    /// # Note
    ///
    /// This configuration can be used to make `wasmi` instrument its internal bytecode
    /// so that it consumes fuel as it executes. Once an execution runs out of fuel
    /// a [`TrapCode::OutOfFuel`](crate::core::TrapCode::OutOfFuel) trap is raised.
    /// This way users can deterministically halt or yield the execution of WebAssembly code.
    ///
    /// - Use [`Store::add_fuel`](crate::Store::add_fuel) to pour some fuel into the [`Store`]
    ///   before executing some code as the [`Store`] start with no fuel.
    /// - Use [`Caller::consume_fuel`](crate::Caller::consume_fuel) to charge costs for executed
    ///   host functions.
    ///
    /// Disabled by default.
    ///
    /// [`Store`]: crate::Store
    /// [`Engine`]: crate::Engine
    pub fn consume_fuel(&mut self, enable: bool) -> &mut Self {
        self.consume_fuel = enable;
        self
    }

    pub fn builtins_consume_fuel(&mut self, builtins_consume_fuel: bool) -> &mut Self {
        if self.rwasm_config.is_some() {
            self.rwasm_config.as_mut().unwrap().builtins_consume_fuel = builtins_consume_fuel;
        }
        self
    }

    pub fn get_i32_translator(&self) -> bool {
        self.rwasm_config
            .as_ref()
            .map(|v| v.use_32bit_mode)
            .unwrap_or(false)
    }

    /// Returns `true` if the [`Config`] enables fuel consumption by the [`Engine`].
    ///
    /// [`Engine`]: crate::Engine
    pub fn get_consume_fuel(&self) -> bool {
        self.consume_fuel
    }

    pub fn get_builtins_consume_fuel(&self) -> bool {
        self.rwasm_config
            .as_ref()
            .map(|rwasm_config| rwasm_config.builtins_consume_fuel)
            .unwrap_or(false)
    }

    /// Returns the configured [`FuelCosts`].
    pub(crate) fn fuel_costs(&self) -> &FuelCosts {
        &self.fuel_costs
    }

    /// Configures the [`FuelConsumptionMode`] for the [`Engine`].
    ///
    /// # Note
    ///
    /// This has no effect if fuel metering is disabled for the [`Engine`].
    ///
    /// [`Engine`]: crate::Engine
    pub fn fuel_consumption_mode(&mut self, mode: FuelConsumptionMode) -> &mut Self {
        self.fuel_consumption_mode = mode;
        self
    }

    /// Returns the [`FuelConsumptionMode`] for the [`Engine`].
    ///
    /// Returns `None` if fuel metering is disabled for the [`Engine`].
    ///
    /// [`Engine`]: crate::Engine
    pub fn get_fuel_consumption_mode(&self) -> Option<FuelConsumptionMode> {
        self.get_consume_fuel()
            .then_some(self.fuel_consumption_mode)
    }

    pub fn rwasm_config(&mut self, rwasm_config: RwasmConfig) -> &mut Self {
        self.rwasm_config = Some(rwasm_config);
        self
    }

    pub fn get_rwasm_config(&self) -> Option<&RwasmConfig> {
        self.rwasm_config.as_ref()
    }

    pub fn get_rwasm_wrap_import_funcs(&self) -> bool {
        self.rwasm_config
            .as_ref()
            .map(|rwasm_config| rwasm_config.wrap_import_functions)
            .unwrap_or_default()
    }

    /// Returns the [`WasmFeatures`] represented by the [`Config`].
    pub(crate) fn wasm_features(&self) -> WasmFeatures {
        WasmFeatures {
            multi_value: self.multi_value,
            mutable_global: self.mutable_global,
            saturating_float_to_int: self.saturating_float_to_int,
            sign_extension: self.sign_extension,
            bulk_memory: self.bulk_memory,
            reference_types: self.reference_types,
            tail_call: self.tail_call,
            extended_const: self.extended_const,
            floats: self.floats,
            component_model: false,
            simd: false,
            relaxed_simd: false,
            threads: false,
            multi_memory: false,
            exceptions: false,
            memory64: false,
            memory_control: false,
        }
    }
}
