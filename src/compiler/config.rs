use crate::{ImportLinker, Opcode};
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
    /// Enable fuel metering for params and locals
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
}

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
            consume_fuel_for_params_and_locals: true,
            code_snippets: true,
            allow_func_ref_function_types: false,
            allow_start_section: false,
        }
    }
}

impl CompilationConfig {
    /// Returns the WebAssembly features configuration for the current instance.
    pub fn wasm_features(&self) -> WasmFeatures {
        let mut wasm_features = WasmFeatures::default();
        wasm_features.mutable_global = true;
        wasm_features.saturating_float_to_int = true;
        wasm_features.sign_extension = true;
        wasm_features.multi_value = true;
        wasm_features.bulk_memory = true;
        wasm_features.reference_types = true;
        wasm_features.tail_call = true;
        wasm_features.extended_const = true;
        wasm_features
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
            self.consume_fuel_for_params_and_locals = false;
            self.builtins_consume_fuel = false;
        }
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
}
