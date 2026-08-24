//! Regression tests for compiling and running real wasm binaries with code
//! snippets disabled (`with_code_snippets(false)`), where every i64 helper is
//! expanded inline (FLU-1179).
//!
//! Before the fix, `translate_to_snippet_call` modeled every inline snippet as
//! a plain binary operator whose result type equals its operand type. The ten
//! i64 comparison snippets return an `i32`, so each inline compare left the
//! translator's `stack_types` and stack-height model skewed, which panicked on
//! real binaries and miscompiled `local.*` depths.

mod common;

use common::{create_import_linker, fluentbase_syscall_handler, HostState};
use rwasm::{
    CompilationConfig, ExecutionEngine, ImportLinker, ImportName, RwasmModule, RwasmStore, Value,
};
use rwasm_fuel_policy::SyscallFuelParams;
use std::{str::from_utf8, sync::Arc};
use wasmparser::{Parser, Payload, Type, TypeRef, ValType};

fn snippets_off_config() -> CompilationConfig {
    CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_consume_fuel(false)
        .with_code_snippets(false)
}

/// Builds an import linker straight from the wasm import section, assigning
/// sequential syscall indices. Mirrors the repro setup from FLU-1179.
fn import_linker_from_wasm(wasm_binary: &[u8]) -> Arc<ImportLinker> {
    let mut import_linker = ImportLinker::default();
    let mut func_types: Vec<(Vec<ValType>, Vec<ValType>)> = Vec::new();
    let mut sys_func_idx = 70u32;
    for payload in Parser::new(0).parse_all(wasm_binary) {
        match payload.expect("valid wasm") {
            Payload::TypeSection(reader) => {
                for ty in reader {
                    let Type::Func(func_type) = ty.expect("valid type");
                    func_types.push((
                        func_type.params().to_vec(),
                        func_type.results().to_vec(),
                    ));
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader {
                    let import = import.expect("valid import");
                    if let TypeRef::Func(type_idx) = import.ty {
                        let (params, results) = func_types[type_idx as usize].clone();
                        // `insert_function` demands 'static slices; leaking is fine in a test.
                        import_linker.insert_function(
                            ImportName::new(import.module, import.name),
                            sys_func_idx,
                            SyscallFuelParams::default(),
                            params.leak(),
                            results.leak(),
                        );
                        sys_func_idx += 1;
                    }
                }
            }
            _ => {}
        }
    }
    Arc::new(import_linker)
}

fn compile_snippets_off(wasm_binary: &[u8]) {
    let config = snippets_off_config().with_import_linker(import_linker_from_wasm(wasm_binary));
    RwasmModule::compile(config, wasm_binary).expect("compilation must succeed");
}

#[test]
fn test_nitro_verifier_compiles_with_snippets_off() {
    compile_snippets_off(include_bytes!("assets/nitro-verifier-stack-ub.wasm"));
}

#[test]
fn test_secp256k1_compiles_with_snippets_off() {
    compile_snippets_off(include_bytes!("assets/secp256k1-stack-ub.wasm"));
}

#[test]
fn test_panic_compiles_with_snippets_off() {
    compile_snippets_off(include_bytes!("assets/panic-stack-ub.wasm"));
}

fn run_fluentbase_binary_snippets_off(wasm_binary: &[u8], host_state: HostState) -> HostState {
    let import_linker = create_import_linker();
    let config = snippets_off_config().with_import_linker(import_linker.clone());
    let (rwasm_module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    let mut store = RwasmStore::new(
        import_linker.clone(),
        host_state,
        fluentbase_syscall_handler,
        None,
        None,
    );
    let instance = import_linker
        .instantiate(&mut store, ExecutionEngine::new(), rwasm_module)
        .unwrap();
    instance.execute(&mut store, &[], &mut []).unwrap();
    use rwasm::StoreTr;
    store.data().clone()
}

#[test]
fn test_wasm_panic_executes_with_snippets_off() {
    let wasm_binary = include_bytes!("assets/panic-stack-ub.wasm");
    let host_state = run_fluentbase_binary_snippets_off(wasm_binary, HostState::default());
    assert_eq!(
        from_utf8(host_state.output.as_slice()).unwrap(),
        "it's panic time"
    )
}

#[test]
fn test_wasm_secp256k1_executes_with_snippets_off() {
    let wasm_binary = include_bytes!("assets/secp256k1-stack-ub.wasm");
    let mut host_state = HostState {
        input: vec![0u8; 1024],
        ..Default::default()
    };
    host_state.input.extend_from_slice(&hex_literal::hex!("a04a451028d0f9284ce82243755e245238ab1e4ecf7b9dd8bf4734d9ecfd0529cf09dd8d0eb3c3968aca8846a249424e5537d3470f979ff902b57914dc77d02316bd29784f668a73cc7a36f4cc5b9ce704481e6cb5b1c2c832af02ca6837ebec044e3b81af9c2234cad09d679ce6035ed1392347ce64ce405f5dcd36228a25de6e47fd35c4215d1edf53e6f83de344615ce719bdb0fd878f6ed76f06dd277956de"));
    run_fluentbase_binary_snippets_off(wasm_binary, host_state);
}

fn execute_main(wat_source: &str, params: &[Value], result: &mut [Value]) {
    let wasm_binary = wat::parse_str(wat_source).unwrap();
    let (rwasm_module, _) = RwasmModule::compile(snippets_off_config(), &wasm_binary).unwrap();
    let mut store = RwasmStore::<()>::default();
    let engine = ExecutionEngine::default();
    engine
        .execute(&mut store, &rwasm_module, params, result)
        .expect("execution must succeed");
}

fn run_i64_binary_op(op: &str, a: i64, b: i64, expected: i64) {
    let wat_source = format!(
        r#"
(module
  (func (export "main") (param i64 i64) (result i64)
    local.get 0
    local.get 1
    {op}
  )
)
"#
    );
    let mut result = [Value::I64(0); 1];
    execute_main(&wat_source, &[Value::I64(a), Value::I64(b)], &mut result);
    assert_eq!(
        result[0].i64().unwrap(),
        expected,
        "mismatch for {op} with inputs ({a}, {b})"
    );
}

fn run_i64_compare_op(op: &str, a: i64, b: i64, expected: bool) {
    // The compare result is consumed by an i64 local.get + i64 arithmetic to
    // exercise `local.*` depths computed over the type stack after an inline
    // compare: before the fix, the compare left a bogus i64 entry there and
    // every later depth was off by one.
    let wat_source = format!(
        r#"
(module
  (func (export "main") (param i64 i64) (result i64)
    local.get 0
    local.get 1
    {op}
    i64.extend_i32_u
    local.get 0
    i64.add
    local.get 1
    i64.sub
  )
)
"#
    );
    let mut result = [Value::I64(0); 1];
    execute_main(&wat_source, &[Value::I64(a), Value::I64(b)], &mut result);
    let expected = (expected as i64).wrapping_add(a).wrapping_sub(b);
    assert_eq!(
        result[0].i64().unwrap(),
        expected,
        "mismatch for {op} with inputs ({a}, {b})"
    );
}

const EDGE_VALUES: &[i64] = &[
    0,
    1,
    -1,
    2,
    63,
    64,
    65,
    i64::MIN,
    i64::MAX,
    i64::MIN + 1,
    0x0000_0001_0000_0000,
    0x7fff_ffff_0000_0001,
    -0x0000_0001_0000_0001,
    0x1234_5678_9abc_def0,
];

#[test]
fn test_i64_arithmetic_ops_with_snippets_off() {
    for &a in EDGE_VALUES {
        for &b in EDGE_VALUES {
            run_i64_binary_op("i64.add", a, b, a.wrapping_add(b));
            run_i64_binary_op("i64.sub", a, b, a.wrapping_sub(b));
            run_i64_binary_op("i64.mul", a, b, a.wrapping_mul(b));
            if b != 0 && !(a == i64::MIN && b == -1) {
                run_i64_binary_op("i64.div_s", a, b, a.wrapping_div(b));
                run_i64_binary_op("i64.rem_s", a, b, a.wrapping_rem(b));
            }
            if b != 0 {
                run_i64_binary_op("i64.div_u", a, b, ((a as u64) / (b as u64)) as i64);
                run_i64_binary_op("i64.rem_u", a, b, ((a as u64) % (b as u64)) as i64);
            }
        }
    }
}

#[test]
fn test_i64_shift_ops_with_snippets_off() {
    for &a in EDGE_VALUES {
        for &b in EDGE_VALUES {
            let shamt = (b as u64 % 64) as u32;
            run_i64_binary_op("i64.shl", a, b, a.wrapping_shl(shamt));
            run_i64_binary_op("i64.shr_s", a, b, a.wrapping_shr(shamt));
            run_i64_binary_op("i64.shr_u", a, b, ((a as u64) >> shamt) as i64);
            run_i64_binary_op("i64.rotl", a, b, a.rotate_left(shamt));
            run_i64_binary_op("i64.rotr", a, b, a.rotate_right(shamt));
        }
    }
}

#[test]
fn test_i64_compare_ops_with_snippets_off() {
    for &a in EDGE_VALUES {
        for &b in EDGE_VALUES {
            run_i64_compare_op("i64.eq", a, b, a == b);
            run_i64_compare_op("i64.ne", a, b, a != b);
            run_i64_compare_op("i64.lt_s", a, b, a < b);
            run_i64_compare_op("i64.lt_u", a, b, (a as u64) < (b as u64));
            run_i64_compare_op("i64.gt_s", a, b, a > b);
            run_i64_compare_op("i64.gt_u", a, b, (a as u64) > (b as u64));
            run_i64_compare_op("i64.le_s", a, b, a <= b);
            run_i64_compare_op("i64.le_u", a, b, (a as u64) <= (b as u64));
            run_i64_compare_op("i64.ge_s", a, b, a >= b);
            run_i64_compare_op("i64.ge_u", a, b, (a as u64) >= (b as u64));
        }
    }
}

/// Direct regression for the `stack_types` skew: keep the i32 compare result
/// on the stack, read i64 locals above it, and branch on the compare result
/// afterward — every step depends on correct type-stack accounting.
#[test]
fn test_local_depths_after_inline_compare() {
    let wat_source = r#"
(module
  (func (export "main") (param i64 i64) (result i64)
    (local i64)
    local.get 0
    local.get 1
    i64.add
    local.set 2
    local.get 0
    local.get 1
    i64.lt_u
    (if (result i64)
      (then local.get 2)
      (else
        local.get 2
        i64.const -1
        i64.mul))
  )
)
"#;
    let cases: &[(i64, i64)] = &[(1, 2), (2, 1), (-1, 1), (i64::MIN, i64::MAX), (5, 5)];
    for &(a, b) in cases {
        let expected = if (a as u64) < (b as u64) {
            a.wrapping_add(b)
        } else {
            a.wrapping_add(b).wrapping_mul(-1)
        };
        let mut result = [Value::I64(0); 1];
        execute_main(wat_source, &[Value::I64(a), Value::I64(b)], &mut result);
        assert_eq!(result[0].i64().unwrap(), expected, "inputs ({a}, {b})");
    }
}
