use crate::{ImportLinker, Opcode, OpcodeData};
use wasmparser::WasmFeatures;

#[derive(Debug, Clone)]
pub struct StateRouterConfig {
    /// List of states to be router based on the state
    pub states: Box<[(Box<str>, u32)]>,
    /// Instruction that describes how we determine an input state
    pub opcode: Option<(Opcode, OpcodeData)>,
}

#[derive(Default, Clone, Debug)]
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
    pub import_linker: Option<ImportLinker>,
    /// Do we need to wrap input functions to convert them from ExternRef to FuncRef (we need it to
    /// simplify tables sometimes)? It's necessary only for rWASM mode where we replace all
    /// external calls with import linker mapping.
    pub wrap_import_functions: bool,
    /// An option to disable malformed entrypoint func type check. We need this check for e2e tests
    /// where we manage stack manually.
    pub allow_malformed_entrypoint_func_type: bool,
    /// Should fuel-charging instructions be injected before each builtin call.
    pub builtins_consume_fuel: bool,
    /// A boolean field that indicates whether floating-point operations are enabled. It's used
    /// only for running WebAssembly testing suite to make sure rWasm fully satisfies WebAssembly
    /// standards.
    pub enable_floating_point: bool,
}

impl CompilationConfig {
    /// Returns the WebAssembly features configuration for the current instance.
    pub fn wasm_features(&self) -> WasmFeatures {
        let wasm_features = WasmFeatures::default();
        // TODO(dmitry123): "be careful with these flags"
        // wasm_features.floats = self.enable_floating_point;

        // let mut config = rwasm_legacy::Config::default();
        // config
        //     .wasm_mutable_global(false)
        //     .wasm_saturating_float_to_int(false)
        //     .wasm_sign_extension(false)
        //     .wasm_multi_value(false)
        //     .wasm_mutable_global(true)
        //     .wasm_saturating_float_to_int(true)
        //     .wasm_sign_extension(true)
        //     .wasm_multi_value(true)
        //     .wasm_bulk_memory(true)
        //     .wasm_reference_types(true)
        //     .wasm_tail_call(true)
        //     .wasm_extended_const(true);

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

    pub fn with_import_linker(mut self, import_linker: ImportLinker) -> Self {
        self.import_linker = Some(import_linker);
        self
    }

    pub fn with_wrap_import_functions(mut self, wrap_import_functions: bool) -> Self {
        self.wrap_import_functions = wrap_import_functions;
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

    pub fn with_enable_floating_point(mut self, enable_floating_point: bool) -> Self {
        self.enable_floating_point = enable_floating_point;
        self
    }
}
