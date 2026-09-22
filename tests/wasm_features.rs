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
//! 1. `CompilationConfig::wasm_features` is an explicit union that denies every proposal the
//!    translator cannot handle, so the validator rejects the module before the translator sees
//!    it.
//! 2. The wildcard arm of `impl_visit_operator!` in `FuncBuilder` validates and then rejects an
//!    operator of any proposal it does not forward with `NotSupportedOpcode`, so an operator that
//!    slips past the first gate still fails loudly instead of being skipped.

use rwasm::{CompilationConfig, CompilationError, RwasmModule};
use wasmparser::WasmFeatures;

fn compile(wat_str: &str) -> Result<(), CompilationError> {
    let wasm = wat::parse_str(wat_str).expect("valid WAT");
    let config = CompilationConfig::default().with_entrypoint_name("main".into());
    RwasmModule::compile(config, &wasm).map(|_| ())
}

/// The reported CRIT-2 repro: SIMD passed validation, was skipped by the translator, and the
/// resulting desync panicked in `compute_drop_keep`. It must be a compilation error instead.
///
/// The crate builds `wasmparser` without its `simd` feature, so a SIMD operator is not even
/// decodable: the reader rejects the `0xfd` prefix before validation.
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
    let message = format!("{error}");
    assert!(
        message.contains("SIMD"),
        "a SIMD operator was rejected, but not for the expected reason: {message}"
    );
}

/// The four wide-arithmetic operators are translated (`tests/wide_arithmetic.rs` checks what
/// they compute); here they must pass both gates.
#[test]
fn test_wide_arithmetic_operators_compile() {
    let cases = [
        (
            "i64.add128",
            r#"(module (func (export "main") (param i64 i64 i64 i64) (result i64 i64)
                 local.get 0 local.get 1 local.get 2 local.get 3 i64.add128))"#,
        ),
        (
            "i64.sub128",
            r#"(module (func (export "main") (param i64 i64 i64 i64) (result i64 i64)
                 local.get 0 local.get 1 local.get 2 local.get 3 i64.sub128))"#,
        ),
        (
            "i64.mul_wide_s",
            r#"(module (func (export "main") (param i64 i64) (result i64 i64)
                 local.get 0 local.get 1 i64.mul_wide_s))"#,
        ),
        (
            "i64.mul_wide_u",
            r#"(module (func (export "main") (param i64 i64) (result i64 i64)
                 local.get 0 local.get 1 i64.mul_wide_u))"#,
        ),
    ];
    for (operator, wat_str) in cases {
        compile(wat_str).unwrap_or_else(|error| panic!("`{operator}` must compile: {error}"));
    }
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
        (
            "function_references",
            r#"(module (func (export "main") (param (ref func)) nop))"#,
            "function references",
        ),
        (
            "gc",
            r#"(module (type (struct)) (func (export "main") nop))"#,
            "without the gc feature",
        ),
        (
            "tail_call is on, but call_ref is function_references",
            r#"(module (type $t (func)) (func (export "main") (param (ref $t))
                 local.get 0 call_ref $t))"#,
            "function references",
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
/// `wasm_features` is an explicit union, so a `wasmparser` upgrade that turns a proposal on by
/// default changes nothing here. The last assertion is what makes an upgrade that adds a flag
/// fail: every flag this `wasmparser` knows must be in one of the two lists.
#[test]
fn test_wasm_features_denies_every_unimplemented_proposal() {
    let features = CompilationConfig::default().wasm_features();

    // The proposals the translator does implement, pinned so the gate cannot be tightened by
    // accident either.
    let implemented = WasmFeatures::MUTABLE_GLOBAL
        | WasmFeatures::SATURATING_FLOAT_TO_INT
        | WasmFeatures::SIGN_EXTENSION
        | WasmFeatures::MULTI_VALUE
        | WasmFeatures::BULK_MEMORY
        | WasmFeatures::REFERENCE_TYPES
        | WasmFeatures::TAIL_CALL
        | WasmFeatures::EXTENDED_CONST
        | WasmFeatures::WIDE_ARITHMETIC
        | WasmFeatures::FLOATS
        | WasmFeatures::GC_TYPES;
    assert_eq!(features, implemented);

    let denied = WasmFeatures::SIMD
        | WasmFeatures::RELAXED_SIMD
        | WasmFeatures::THREADS
        | WasmFeatures::SHARED_EVERYTHING_THREADS
        | WasmFeatures::MULTI_MEMORY
        | WasmFeatures::MEMORY64
        | WasmFeatures::EXCEPTIONS
        | WasmFeatures::LEGACY_EXCEPTIONS
        | WasmFeatures::COMPONENT_MODEL
        | WasmFeatures::FUNCTION_REFERENCES
        | WasmFeatures::GC
        | WasmFeatures::MEMORY_CONTROL
        | WasmFeatures::CUSTOM_PAGE_SIZES
        | WasmFeatures::STACK_SWITCHING
        | WasmFeatures::CUSTOM_DESCRIPTORS
        | WasmFeatures::COMPACT_IMPORTS
        // component-model sub-features: no core module can express them
        | WasmFeatures::CM_VALUES
        | WasmFeatures::CM_NESTED_NAMES
        | WasmFeatures::CM_ASYNC
        | WasmFeatures::CM_ASYNC_STACKFUL
        | WasmFeatures::CM_MORE_ASYNC_BUILTINS
        | WasmFeatures::CM_THREADING
        | WasmFeatures::CM_ERROR_CONTEXT
        | WasmFeatures::CM_FIXED_LENGTH_LISTS
        | WasmFeatures::CM_GC
        | WasmFeatures::CM_MAP
        | WasmFeatures::CM64;
    assert!(
        !features.intersects(denied),
        "a denied proposal is enabled: {:?}",
        features.intersection(denied)
    );

    // `BULK_MEMORY_OPT` and `CALL_INDIRECT_OVERLONG` are subsets of `BULK_MEMORY` and
    // `REFERENCE_TYPES`; every other flag this wasmparser defines is listed above.
    let considered =
        implemented | denied | WasmFeatures::BULK_MEMORY_OPT | WasmFeatures::CALL_INDIRECT_OVERLONG;
    assert_eq!(
        WasmFeatures::all().difference(considered),
        WasmFeatures::empty(),
        "a wasmparser feature flag is neither implemented nor denied here"
    );
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

/// Reference-typed values keep compiling: `ref.null`, `ref.func`, `ref.is_null`, a `funcref`
/// table and an `externref` local all use the reference-types shapes the translator lowers.
#[test]
fn test_reference_types_still_compile() {
    compile(
        r#"
        (module
          (table 2 funcref)
          (elem declare func $f)
          (func $f)
          (func (export "main") (result i32) (local externref)
            ref.null extern local.set 0
            i32.const 0 ref.func $f table.set 0
            ref.null func ref.is_null
            local.get 0 ref.is_null
            i32.add))
        "#,
    )
    .expect("reference types must still compile");
}

/// Decoding is feature-dependent too: `wasmparser` reads the memory index of `memory.grow` as
/// a single zero byte without `multi_memory` and as a LEB with it, and a fresh `Parser` defaults
/// to every feature. The module parser hands the parser the same set as the validator, so an
/// overlong index is malformed here, as the spec's `binary.wast` and Wasmtime say.
#[test]
fn test_parser_decodes_with_the_validators_feature_set() {
    // `(func (export "main") (result i32) i32.const 0 memory.grow)` with the memory index of
    // `memory.grow` encoded as `0x80 0x00`
    let overlong = b"\0asm\x01\0\0\0\
        \x01\x05\x01\x60\x00\x01\x7f\
        \x03\x02\x01\x00\
        \x05\x03\x01\x00\x01\
        \x07\x08\x01\x04main\x00\x00\
        \x0a\x09\x01\x07\x00\x41\x00\x40\x80\x00\x0b";
    let config = CompilationConfig::default().with_entrypoint_name("main".into());
    let error = RwasmModule::compile(config.clone(), overlong)
        .expect_err("an overlong memory index must not compile");
    let message = format!("{error}");
    assert!(
        message.contains("zero byte expected"),
        "the overlong index was rejected, but not for the expected reason: {message}"
    );
    // the same module with a single zero byte is the valid encoding
    let canonical = b"\0asm\x01\0\0\0\
        \x01\x05\x01\x60\x00\x01\x7f\
        \x03\x02\x01\x00\
        \x05\x03\x01\x00\x01\
        \x07\x08\x01\x04main\x00\x00\
        \x0a\x08\x01\x06\x00\x41\x00\x40\x00\x0b";
    RwasmModule::compile(config, canonical).expect("the canonical encoding compiles");
}
