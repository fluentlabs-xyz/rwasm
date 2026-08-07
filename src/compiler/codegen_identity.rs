//! Codegen identity: a stable fingerprint of everything that changes emitted rwasm bytecode.
//!
//! The same wasm input does not compile to the same rwasm bytecode under every build:
//! [`CompilationConfig`] flags decide which functions are emitted and where fuel charges are
//! injected, and the `fpu` cargo feature decides whether float instructions are lowered to real
//! opcodes or to `Trap(IllegalOpcode)`. None of that is recorded in the serialized module (see
//! `docs/module-format.md`), so two differently built compilers silently produce different bytes
//! for one contract.
//!
//! [`CompilationConfig::codegen_identity`] hashes those inputs into a single 32-byte value.
//! Hosts that distribute or address compiled modules by hash should pin this identity next to the
//! bytecode and reject a module whose producer identity does not match the local one — a clean
//! error instead of a silent behavioral difference.

use crate::{
    intrinsic::Intrinsic, CompilationConfig, ImportLinker, ImportName, Opcode, StateRouterConfig,
};
use alloc::vec::Vec;
use rwasm_fuel_policy::SyscallFuelParams;
use tiny_keccak::{Hasher, Keccak};
use wasmparser::ValType;

/// Domain separator for the codegen identity hash.
///
/// Bump the trailing version whenever the preimage layout below changes; it keeps identities
/// produced by different rwasm releases from colliding.
const CODEGEN_IDENTITY_DOMAIN: &[u8] = b"rwasm.codegen-identity.v1";

/// The `fpu` cargo feature is enabled.
///
/// Float instructions are lowered to real opcodes instead of `Trap(IllegalOpcode)`, so every
/// module compiled from float-using wasm differs from a default build. See the note on
/// `impl_fpu_opcode!` in `src/isa/mod.rs`: `fpu` exists for the e2e/fuzz suites only.
pub const CODEGEN_FEATURE_FPU: u64 = 1 << 0;

/// Returns the set of compile-time cargo features that affect emitted bytecode.
///
/// Only features that change codegen belong here; features that change the host-side surface
/// (`std`, `serde`, `wasmtime`, …) do not affect the bytes and are deliberately excluded.
pub const fn codegen_feature_set() -> u64 {
    let mut features = 0u64;
    if cfg!(feature = "fpu") {
        features |= CODEGEN_FEATURE_FPU;
    }
    features
}

impl CompilationConfig {
    /// Returns a 32-byte fingerprint of every input that affects emitted bytecode: the
    /// codegen-relevant fields of this config plus the compile-time feature set of the compiling
    /// binary ([`codegen_feature_set`]).
    ///
    /// Two compilers agreeing on this value compile any given wasm input to identical rwasm bytes;
    /// two compilers disagreeing on it may not. The value is not part of the module wire format,
    /// so a host that cares about reproducibility must carry it alongside the bytecode itself.
    pub fn codegen_identity(&self) -> [u8; 32] {
        let mut hasher = IdentityHasher::new();

        hasher.bytes(CODEGEN_IDENTITY_DOMAIN);
        hasher.u64(codegen_feature_set());

        hasher.opt(self.state_router.as_ref(), IdentityHasher::state_router);
        hasher.opt(self.entrypoint_name.as_deref(), |hasher, name| {
            hasher.bytes(name.as_bytes())
        });
        hasher.opt(self.import_linker.as_deref(), IdentityHasher::import_linker);
        hasher.opt(self.default_imported_global_value.as_ref(), |hasher, v| {
            hasher.u64(*v as u64)
        });

        hasher.bool(self.allow_malformed_entrypoint_func_type);
        hasher.bool(self.builtins_consume_fuel);
        hasher.bool(self.consume_fuel);
        hasher.bool(self.code_snippets);
        hasher.bool(self.consume_fuel_for_bulk_ops);
        hasher.bool(self.consume_fuel_for_params_and_locals);
        hasher.bool(self.allow_func_ref_function_types);
        hasher.bool(self.allow_start_section);
        hasher.u32(self.max_allowed_memory_pages);

        hasher.finalize()
    }
}

/// Keccak-256 over a length-prefixed preimage.
///
/// Every variable-length element is written with its length first, so no two distinct configs can
/// serialize into the same byte stream.
struct IdentityHasher {
    keccak: Keccak,
}

impl IdentityHasher {
    fn new() -> Self {
        Self {
            keccak: Keccak::v256(),
        }
    }

    fn finalize(self) -> [u8; 32] {
        let mut output = [0u8; 32];
        self.keccak.finalize(&mut output);
        output
    }

    fn u8(&mut self, value: u8) {
        self.keccak.update(&[value]);
    }

    fn u32(&mut self, value: u32) {
        self.keccak.update(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.keccak.update(&value.to_le_bytes());
    }

    fn bool(&mut self, value: bool) {
        self.u8(value as u8);
    }

    fn bytes(&mut self, value: &[u8]) {
        self.u64(value.len() as u64);
        self.keccak.update(value);
    }

    /// Hashes an optional value as a presence tag followed by the value itself.
    fn opt<T>(&mut self, value: Option<T>, hash_value: impl FnOnce(&mut Self, T)) {
        match value {
            Some(value) => {
                self.u8(1);
                hash_value(self, value);
            }
            None => self.u8(0),
        }
    }

    fn opcode(&mut self, opcode: &Opcode) {
        let encoded: Vec<u8> = bincode::encode_to_vec(opcode, bincode::config::legacy())
            .unwrap_or_else(|_| unreachable!("rwasm: failed to encode opcode"));
        self.bytes(&encoded);
    }

    fn val_types(&mut self, val_types: &[ValType]) {
        self.u64(val_types.len() as u64);
        for val_type in val_types {
            self.u8(match val_type {
                ValType::I32 => 0,
                ValType::I64 => 1,
                ValType::F32 => 2,
                ValType::F64 => 3,
                ValType::V128 => 4,
                ValType::FuncRef => 5,
                ValType::ExternRef => 6,
            });
        }
    }

    /// State order is part of the routing code, so states are hashed in declaration order.
    fn state_router(&mut self, state_router: &StateRouterConfig) {
        self.u64(state_router.states.len() as u64);
        for (name, func_idx) in state_router.states.iter() {
            self.bytes(name.as_bytes());
            self.u32(*func_idx);
        }
        self.opt(state_router.opcode.as_ref(), Self::opcode);
    }

    /// Entries are hashed in sorted import-name order because the linker itself is backed by a
    /// hash map, whose iteration order is not stable across runs.
    fn import_linker(&mut self, import_linker: &ImportLinker) {
        let symbols = import_linker.find_symbols();
        self.u64(symbols.len() as u64);
        for import_name in symbols.iter() {
            self.import_name(import_name);
            let entity = import_linker
                .resolve_by_import_name(import_name)
                .unwrap_or_else(|| unreachable!("rwasm: import linker symbol without an entity"));
            self.u32(entity.sys_func_idx);
            self.val_types(entity.params);
            self.val_types(entity.result);
            self.syscall_fuel_params(&entity.syscall_fuel_param);
            self.opt(entity.intrinsic.as_ref(), Self::intrinsic);
        }
    }

    fn import_name(&mut self, import_name: &ImportName) {
        self.bytes(import_name.module().as_bytes());
        self.bytes(import_name.name().as_bytes());
    }

    fn intrinsic(&mut self, intrinsic: &Intrinsic) {
        match intrinsic {
            Intrinsic::Replace(opcodes) => {
                self.u8(0);
                self.u64(opcodes.len() as u64);
                for opcode in opcodes {
                    self.opcode(opcode);
                }
            }
            Intrinsic::Remove => self.u8(1),
        }
    }

    fn syscall_fuel_params(&mut self, params: &SyscallFuelParams) {
        match params {
            SyscallFuelParams::None => self.u8(0),
            SyscallFuelParams::Const(fuel) => {
                self.u8(1);
                self.u64(*fuel);
            }
            SyscallFuelParams::LinearFuel(params) => {
                self.u8(2);
                self.u32(params.base_fuel);
                self.u32(params.param_index);
                self.u32(params.word_cost);
            }
            SyscallFuelParams::QuadraticFuel(params) => {
                self.u8(3);
                self.u32(params.local_depth);
                self.u32(params.word_cost);
                self.u32(params.divisor);
                self.u32(params.fuel_denom_rate);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{boxed::Box, sync::Arc};
    use rwasm_fuel_policy::LinearFuelParams;

    fn import_linker(names: &[(&str, &str, u32)]) -> ImportLinker {
        let mut import_linker = ImportLinker::default();
        for (module, field, sys_func_idx) in names {
            import_linker.insert_function(
                ImportName::new(module, field),
                *sys_func_idx,
                SyscallFuelParams::LinearFuel(LinearFuelParams {
                    base_fuel: 1,
                    param_index: 0,
                    word_cost: 2,
                }),
                &[ValType::I32],
                &[ValType::I64],
            );
        }
        import_linker
    }

    /// Pins the preimage layout: the identity of the default config must not drift silently,
    /// and it must differ between an `fpu` build and a default one. Update these digests together
    /// with the `CODEGEN_IDENTITY_DOMAIN` version whenever the preimage changes on purpose.
    #[test]
    fn default_identity_is_pinned() {
        let expected = if cfg!(feature = "fpu") {
            hex_literal::hex!("551dea720f2815bc82b4cd8ff7a4fcc4a68ea457c2f4b2793fc190ff8d2ee277")
        } else {
            hex_literal::hex!("72e113e7f5d3ac82ea0588d7cda7eaab3ea613154090b4a1b8d5f7621f475a66")
        };
        assert_eq!(CompilationConfig::default().codegen_identity(), expected);
    }

    #[test]
    fn identity_is_stable_for_equal_configs() {
        assert_eq!(
            CompilationConfig::default().codegen_identity(),
            CompilationConfig::default().codegen_identity()
        );
    }

    #[test]
    fn identity_tracks_codegen_relevant_flags() {
        let default_identity = CompilationConfig::default().codegen_identity();
        let modified = [
            CompilationConfig::default().with_code_snippets(false),
            CompilationConfig::default().with_consume_fuel(false),
            CompilationConfig::default().with_consume_fuel_for_bulk_ops(false),
            CompilationConfig::default().with_consume_fuel_for_params_and_locals(false),
            CompilationConfig::default().with_builtins_consume_fuel(true),
            CompilationConfig::default().with_allow_malformed_entrypoint_func_type(true),
            CompilationConfig::default().with_allow_func_ref_function_types(true),
            CompilationConfig::default().with_allow_start_section(true),
            CompilationConfig::default().with_max_allowed_memory_pages(1),
            CompilationConfig::default().with_default_imported_global_value(0),
            CompilationConfig::default().with_entrypoint_name("main".into()),
            CompilationConfig::default().with_state_router(StateRouterConfig {
                states: Box::new([("deploy".into(), 0)]),
                opcode: Some(Opcode::I32Const(0.into())),
            }),
            CompilationConfig::default().with_import_linker(Arc::new(import_linker(&[(
                "env",
                "keccak256",
                0,
            )]))),
        ];
        for config in modified {
            assert_ne!(
                config.codegen_identity(),
                default_identity,
                "identity must change for config {config:?}"
            );
        }
    }

    #[test]
    fn identity_ignores_import_linker_insertion_order() {
        let forward = import_linker(&[("env", "a", 0), ("env", "b", 1), ("other", "a", 2)]);
        let backward = import_linker(&[("other", "a", 2), ("env", "b", 1), ("env", "a", 0)]);
        let identity = |linker: ImportLinker| {
            CompilationConfig::default()
                .with_import_linker(Arc::new(linker))
                .codegen_identity()
        };
        assert_eq!(identity(forward), identity(backward));
    }

    #[test]
    fn identity_separates_state_router_ordering() {
        let router = |states: [(&str, u32); 2]| {
            CompilationConfig::default()
                .with_state_router(StateRouterConfig {
                    states: states
                        .iter()
                        .map(|(name, idx)| ((*name).into(), *idx))
                        .collect(),
                    opcode: None,
                })
                .codegen_identity()
        };
        assert_ne!(
            router([("deploy", 0), ("main", 1)]),
            router([("main", 1), ("deploy", 0)])
        );
    }

    #[test]
    fn feature_set_reports_fpu() {
        assert_eq!(
            codegen_feature_set() & CODEGEN_FEATURE_FPU != 0,
            cfg!(feature = "fpu")
        );
    }
}
