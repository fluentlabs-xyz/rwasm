use crate::{ImportLinker, Instruction};
use wasmparser::WasmFeatures;

#[derive(Debug, Clone)]
pub struct StateRouterConfig {
    /// List of states to be router based on the state
    pub states: Box<[(String, u32)]>,
    /// Instruction that describes how we determine an input state
    pub opcode: Instruction,
}

#[derive(Debug, Clone)]
pub struct CompilerConfig {
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
}

impl Default for CompilerConfig {
    fn default() -> Self {
        Self {
            state_router: None,
            entrypoint_name: Some("main".to_string()),
            import_linker: None,
            wrap_import_functions: false,
            translate_drop_keep: false,
            allow_malformed_entrypoint_func_type: false,
            use_32bit_mode: false,
        }
    }
}

impl CompilerConfig {
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

    pub fn wasm_features(&self) -> WasmFeatures {
        WasmFeatures::default()
    }
}
