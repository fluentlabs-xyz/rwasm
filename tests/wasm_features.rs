//! The language the validator accepts must not be wider than the one the translator implements.
//!
//! An operator that validates but has no translation is skipped silently: no opcode is emitted and
//! neither `stack_height` nor `stack_types` is updated, while the validator's operand stack has
//! moved on. From there the compiler computes `drop_keep` amounts and `local.get`/`local.set`
//! depths against a stack height that does not describe the code it emitted. Sometimes that trips
//! an assert in `compute_drop_keep`; otherwise it miscompiles quietly.
//!
//! Two gates keep the sets aligned, and these tests cover both:
//!
//! 1. `CompilationConfig::wasm_features` denies every proposal the translator cannot handle, so
//!    the validator rejects the module before the translator sees it.
//! 2. The wildcard arm of `impl_visit_operator!` returns `NotSupportedOpcode`, so an operator that
//!    slips past the first gate still fails loudly instead of being skipped.

use rwasm::{CompilationConfig, CompilationError, RwasmModule};

fn compile(wat_str: &str) -> Result<(), CompilationError> {
    let wasm = wat::parse_str(wat_str).expect("valid WAT");
    let config = CompilationConfig::default().with_entrypoint_name("main".into());
    RwasmModule::compile(config, &wasm).map(|_| ())
}

/// The reported CRIT-2 repro: SIMD passed validation, was skipped by the translator, and the
/// resulting desync panicked in `compute_drop_keep`. It must be a compilation error instead.
#[test]
fn test_simd_operator_is_rejected_instead_of_desyncing_the_stack() {
    let error = compile(
        r#"
        (module
          (memory (export "memory") 1)
          (func (export "main") (result i32)
            v128.const i32x4 1 2 3 4
            i32x4.extract_lane 0))
        "#,
    )
    .expect_err("a SIMD operator must not compile");

    // The operator gate rejects `v128.const` before the validator's type check sees a `v128`,
    // which is exactly the arm that used to validate-and-ignore.
    assert!(
        matches!(error, CompilationError::NotSupportedOpcode),
        "unexpected error: {error:?}"
    );
}

/// Every proposal the translator does not implement is rejected, whichever gate catches it first.
///
/// A `wasmparser` upgrade that promotes one of these to on-by-default would otherwise widen the
/// accepted language silently; here it fails.
#[test]
fn test_disabled_proposals_are_rejected() {
    // (proposal, module using it, expected fragment of the validator's message)
    let cases = [
        (
            "simd (v128 local)",
            r#"(module (func (export "main") (local v128) nop))"#,
            "SIMD support is not enabled",
        ),
        (
            "simd (v128 parameter)",
            r#"(module (func (export "main") (param v128) nop))"#,
            "SIMD support is not enabled",
        ),
        (
            "simd (v128 global)",
            r#"(module (global v128 (v128.const i32x4 0 0 0 0)) (func (export "main") nop))"#,
            "SIMD support is not enabled",
        ),
        (
            // Every relaxed-SIMD operator takes or returns a `v128`, so the SIMD gate is what
            // rejects it; there is no shape that reaches the relaxed_simd flag on its own.
            "relaxed_simd",
            r#"(module (func (export "main") (param v128) (result v128)
                 local.get 0 local.get 0 i32x4.relaxed_trunc_f32x4_s))"#,
            "SIMD support is not enabled",
        ),
        (
            "threads",
            r#"(module (memory 1 1 shared) (func (export "main") (result i32)
                 i32.const 0 i32.atomic.load))"#,
            "threads must be enabled for shared memories",
        ),
        (
            "multi_memory",
            r#"(module (memory 1) (memory 1) (func (export "main") nop))"#,
            "multiple memories",
        ),
        (
            "memory64",
            r#"(module (memory i64 1) (func (export "main") nop))"#,
            "memory64 must be enabled for 64-bit memories",
        ),
        (
            "exceptions",
            r#"(module (tag $e (param i32)) (func (export "main") nop))"#,
            "exceptions proposal not enabled",
        ),
    ];

    for (proposal, wat_str, expected) in cases {
        let error = compile(wat_str)
            .err()
            .unwrap_or_else(|| panic!("`{proposal}` must not compile"));
        let message = format!("{error}");
        assert!(
            message.contains(expected),
            "`{proposal}` was rejected, but not for the expected reason: {message}"
        );
    }
}

/// Pins the feature set itself, including the proposals no core module can express in WAT.
///
/// `wasm_features` lists every field explicitly rather than inheriting `Default::default()`, so a
/// `wasmparser` upgrade that adds a field fails to compile and one that flips a default fails
/// here, instead of quietly handing the translator a language it does not implement.
#[test]
fn test_wasm_features_denies_every_unimplemented_proposal() {
    let features = CompilationConfig::default().wasm_features();

    assert!(!features.simd, "simd must stay disabled");
    assert!(!features.relaxed_simd, "relaxed_simd must stay disabled");
    assert!(!features.threads, "threads must stay disabled");
    assert!(!features.multi_memory, "multi_memory must stay disabled");
    assert!(!features.memory64, "memory64 must stay disabled");
    assert!(!features.exceptions, "exceptions must stay disabled");
    assert!(
        !features.component_model,
        "component_model must stay disabled"
    );
    assert!(
        !features.memory_control,
        "memory_control must stay disabled"
    );

    // The proposals the translator does implement, pinned so the gate cannot be tightened by
    // accident either.
    assert!(features.mutable_global);
    assert!(features.saturating_float_to_int);
    assert!(features.sign_extension);
    assert!(features.multi_value);
    assert!(features.bulk_memory);
    assert!(features.reference_types);
    assert!(features.tail_call);
    assert!(features.extended_const);
    assert!(features.floats);
}

/// The stricter wildcard arm must not have narrowed the language that already worked.
#[test]
fn test_supported_proposals_still_compile() {
    // MVP, sign extension, saturating float-to-int, bulk memory, reference types and multi-value
    // all in one module.
    compile(
        r#"
        (module
          (memory (export "memory") 1)
          (table 1 funcref)
          (func $pair (result i32 i32) i32.const 1 i32.const 2)
          (func (export "main") (result i32)
            i32.const 0 i32.const 0 i32.const 0 memory.fill
            i32.const 7 i32.extend8_s
            f32.const 1.5 i32.trunc_sat_f32_s
            i32.add
            call $pair
            i32.add
            i32.add))
        "#,
    )
    .expect("supported proposals must still compile");
}

/// Tail calls are translated, so they must survive the wildcard arm as well.
#[test]
fn test_tail_call_still_compiles() {
    compile(
        r#"
        (module
          (func $callee (result i32) i32.const 1)
          (func (export "main") (result i32) return_call $callee))
        "#,
    )
    .expect("tail calls must still compile");
}
