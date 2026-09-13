//! Reproductions for the round-6 finding of `audits/2026-09-13-rwasm-audit-round6.md`: the
//! translator has no bound on emitted code, so a small input compiles to an enormous module.
//!
//! Both tests assert that the emitted code stays within a sane instruction budget for the input,
//! either by rejecting the module with a compile-time size limit or by emitting a bounded amount.
//! They fail at the audited revision (`4c40f702` + the round-5 fuel rework): a `br_table` emits
//! ~2001 instructions (16 KB) per one-byte target and a `br_if` with a 1000-value `DropKeep`
//! emits the same per occurrence, with no limit anywhere in the compiler.
//!
//! The bound used here is 2,000,000 instructions, the order of magnitude the host's
//! `RWASM_MAX_CODE_SIZE` (12 MiB) implies.
//!
//! NOTE: this file was reconstructed after the fact from the measurements in the round-6 report;
//! the two test names and fixtures match the report.

use rwasm::{CompilationConfig, RwasmModule};

const MAX_INSTRUCTIONS: usize = 2_000_000;

fn config() -> CompilationConfig {
    CompilationConfig::default_strategy_compatible()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
}

/// A `br_table` whose targets all name a 1000-result block, so every target needs a
/// `DropKeep`-style trampoline (`2 * keep + 1` instructions) and none of them is shared.
fn br_table_module(targets: usize) -> String {
    let results = "(result i32)".repeat(1000);
    let consts = "(i32.const 0)".repeat(1000);
    let target_list = "$b ".repeat(targets);
    format!(
        r#"(module
             (func (export "main") {results}
               (block $b {results}
                 (i32.const 7)
                 {consts}
                 (i32.const 0)
                 (br_table {target_list}$b))))"#
    )
}

/// One `br_if` per occurrence, each with a 1000-value `DropKeep` branch to the same block.
fn br_if_module(branches: usize) -> String {
    let results = "(result i32)".repeat(1000);
    let consts = "(i32.const 0)".repeat(1000);
    let branch = "(br_if $b (i32.const 0))".repeat(branches);
    format!(
        r#"(module
             (func (export "main") {results}
               (block $b {results}
                 (i32.const 7)
                 {consts}
                 {branch}
                 (br $b))))"#
    )
}

fn assert_bounded(label: &str, wasm: &[u8]) {
    let instructions = match RwasmModule::compile(config(), wasm) {
        Ok((module, _)) => module.code_section.len(),
        // The expected fix rejects an oversized expansion with a dedicated error; the fixture
        // itself must still be valid Wasm, so any *validation* error is a broken test.
        Err(err) => {
            let text = format!("{err:?}");
            assert!(
                !text.contains("MalformedWasmBinary"),
                "{label}: the fixture must be valid Wasm, got {text}"
            );
            return;
        }
    };
    assert!(
        instructions <= MAX_INSTRUCTIONS,
        "{label}: {} B of Wasm compiled to {instructions} rwasm instructions ({:.1} MiB), which \
         grows without bound in the target count",
        wasm.len(),
        (instructions * core::mem::size_of::<rwasm::Opcode>()) as f64 / (1024.0 * 1024.0),
    );
}

#[test]
fn br_table_expansion_is_bounded() {
    let wasm = wat::parse_str(br_table_module(10_000)).unwrap();
    assert_bounded("br_table with 10,000 targets", &wasm);
}

#[test]
fn br_if_expansion_is_bounded() {
    let wasm = wat::parse_str(br_if_module(10_000)).unwrap();
    assert_bounded("10,000 br_if with a 1000-value drop-keep", &wasm);
}
