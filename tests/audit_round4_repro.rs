//! Reproductions for the 2026-09-13 round-4 audit.
//!
//! `R4-1`: a store holds one instance's state, but nothing stops a host from keeping two live
//! [`RwasmInstance`] handles for two different modules on the same store. Executing the first one
//! after the second was instantiated must be rejected before accessing the new instance's state.
//! Wasmtime supports several instances per store; rwasm exposes one current instance and
//! invalidates its previous handles when replacement succeeds.
#![cfg(feature = "wasmtime")]

use rwasm::{
    always_failing_syscall_handler, CompilationConfig, ExecutionEngine, ImportLinker, RwasmModule,
    RwasmStore, StoreTr, StrategyDefinition, TrapCode, Value,
};
use std::sync::Arc;

const INSTANCE_A: &str = r#"(module
     (memory (export "memory") 1)
     (data (i32.const 0) "AAAA")
     (func (export "main") (result i64) (i64.load8_u (i32.const 0))))"#;

const INSTANCE_B: &str = r#"(module
     (memory (export "memory") 1)
     (data (i32.const 0) "BBBB")
     (func (export "main") (result i64) (i64.load8_u (i32.const 0))))"#;

/// Writes `4242` at offset 4 and reads byte 0, so a caller can tell whose memory the instance used.
const WRITER_A: &str = r#"(module
     (memory (export "memory") 1)
     (data (i32.const 0) "AAAA")
     (func (export "main") (result i64)
       (i32.store (i32.const 4) (i32.const 4242))
       (i64.load8_u (i32.const 0))))"#;

fn config(linker: &Arc<ImportLinker>) -> CompilationConfig {
    CompilationConfig::default_strategy_compatible()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(linker.clone())
}

fn module(linker: &Arc<ImportLinker>, wat: &str) -> RwasmModule {
    let wasm = wat::parse_str(wat).expect("the test module parses");
    RwasmModule::compile(config(linker), &wasm)
        .expect("rwasm compiles the test module")
        .0
}

/// A stale handle must not read the replacement's memory. rwasm rejects it; Wasmtime's separate
/// per-instance storage lets its old handle keep reading the original memory.
#[test]
fn a_live_instance_does_not_run_against_another_instances_state() {
    let linker = Arc::new(ImportLinker::default());
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::new(
        linker.clone(),
        (),
        always_failing_syscall_handler,
        Some(1_000_000),
        None,
    );

    let first = linker
        .instantiate(&mut store, engine, module(&linker, INSTANCE_A))
        .expect("module A instantiates");
    let mut before = [Value::I64(-1)];
    first
        .execute(&mut store, &[], &mut before)
        .expect("module A runs");
    assert_eq!(
        before,
        [Value::I64(65)],
        "A reads its own data before B exists"
    );

    let second = linker
        .instantiate(&mut store, engine, module(&linker, INSTANCE_B))
        .expect("module B instantiates");
    let mut b = [Value::I64(-1)];
    second
        .execute(&mut store, &[], &mut b)
        .expect("module B runs");
    assert_eq!(b, [Value::I64(66)], "B reads its own data");

    let mut after = [Value::I64(-1)];
    let outcome = first.execute(&mut store, &[], &mut after);

    // The Wasmtime oracle: two instances in one store keep their own memories.
    let engine = wasmtime::Engine::default();
    let mut wasmtime_store = wasmtime::Store::new(&engine, ());
    let linker = wasmtime::Linker::new(&engine);
    let wasm_a = wasmtime::Module::new(&engine, wat::parse_str(INSTANCE_A).unwrap()).unwrap();
    let wasm_b = wasmtime::Module::new(&engine, wat::parse_str(INSTANCE_B).unwrap()).unwrap();
    let instance_a = linker.instantiate(&mut wasmtime_store, &wasm_a).unwrap();
    let a_func = instance_a
        .get_typed_func::<(), i64>(&mut wasmtime_store, "main")
        .unwrap();
    let instance_b = linker.instantiate(&mut wasmtime_store, &wasm_b).unwrap();
    let b_func = instance_b
        .get_typed_func::<(), i64>(&mut wasmtime_store, "main")
        .unwrap();
    let wasmtime_after = a_func.call(&mut wasmtime_store, ()).unwrap();
    let wasmtime_b = b_func.call(&mut wasmtime_store, ()).unwrap();
    assert_eq!(wasmtime_b, 66);
    assert_eq!(wasmtime_after, 65, "the oracle isolates the two instances");

    assert_eq!(
        outcome,
        Err(TrapCode::IllegalOpcode),
        "the replaced rwasm handle must be rejected before reading the current instance's memory"
    );
}

/// The same aliasing on the write side: A's store at offset 4 must land in A's memory, not in the
/// memory of the instance that now owns the store.
#[test]
fn a_live_instance_does_not_write_into_another_instances_memory() {
    let linker = Arc::new(ImportLinker::default());
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::new(
        linker.clone(),
        (),
        always_failing_syscall_handler,
        Some(1_000_000),
        None,
    );

    let writer = linker
        .instantiate(&mut store, engine, module(&linker, WRITER_A))
        .expect("module A instantiates");
    let owner = linker
        .instantiate(&mut store, engine, module(&linker, INSTANCE_B))
        .expect("module B instantiates");

    let mut result = [Value::I64(-1)];
    assert_eq!(
        writer.execute(&mut store, &[], &mut result),
        Err(TrapCode::IllegalOpcode)
    );

    // Whatever `store` now holds belongs to module B; A's write must not appear in it.
    let mut buffer = [0u8; 4];
    store
        .memory_read(4, &mut buffer)
        .expect("module B has a page");
    assert_eq!(
        buffer, [0; 4],
        "module A's store leaked into the memory of the instance that owns the store (the value \
         read back is A's 4242 little-endian)"
    );

    // The Wasmtime oracle: B's memory is untouched by A's call.
    let engine = wasmtime::Engine::default();
    let mut wasmtime_store = wasmtime::Store::new(&engine, ());
    let wt_linker = wasmtime::Linker::new(&engine);
    let wasm_a = wasmtime::Module::new(&engine, wat::parse_str(WRITER_A).unwrap()).unwrap();
    let wasm_b = wasmtime::Module::new(&engine, wat::parse_str(INSTANCE_B).unwrap()).unwrap();
    let instance_a = wt_linker.instantiate(&mut wasmtime_store, &wasm_a).unwrap();
    let instance_b = wt_linker.instantiate(&mut wasmtime_store, &wasm_b).unwrap();
    let a_func = instance_a
        .get_typed_func::<(), i64>(&mut wasmtime_store, "main")
        .unwrap();
    a_func.call(&mut wasmtime_store, ()).unwrap();
    let memory_b = instance_b
        .get_export(&mut wasmtime_store, "memory")
        .and_then(|export| export.into_memory())
        .expect("B exports its memory");
    assert_eq!(
        &memory_b.data(&wasmtime_store)[4..8],
        &[0, 0, 0, 0],
        "the oracle keeps A's write out of B's memory"
    );
    let _ = owner;
}

/// The `call_indirect` face of the same aliasing: the first instance's table *is* the second
/// instance's table, so the dispatch target is a code offset from the other module's code layout
/// applied to this module's code section. Module A here is 18 instructions long and never
/// initializes its table; module B has 200 functions and an active element segment, so B's
/// instantiation installs an offset ~1000 instructions into B. rwasm therefore fetches far past
/// the end of A's code section (an instruction-window check measured `instruction offset=1011`
/// against an 18-instruction window); Wasmtime traps `IndirectCallToNull` because A's own table
/// entry is null.
#[test]
fn a_live_instance_does_not_dispatch_through_another_instances_table() {
    const DISPATCH_A: &str = r#"(module
         (type $t (func (result i32)))
         (table 1 funcref)
         (func (export "main") (result i64)
           (i64.extend_i32_u (call_indirect (type $t) (i32.const 0)))))"#;

    let mut dispatch_b = String::from(
        "(module (type $t (func (result i32))) (table 1 funcref) (elem (i32.const 0) $f199)\n",
    );
    for i in 0..200 {
        dispatch_b.push_str(&format!("(func $f{i} (result i32) (i32.const {i}))\n"));
    }
    dispatch_b.push_str("(func (export \"main\") (result i64) (i64.const 0)))");

    let linker = Arc::new(ImportLinker::default());
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::new(
        linker.clone(),
        (),
        always_failing_syscall_handler,
        Some(1_000_000),
        None,
    );
    let a_module = module(&linker, DISPATCH_A);
    let b_module = module(&linker, &dispatch_b);
    assert!(
        b_module.code_section.len() > a_module.code_section.len() * 16,
        "the fixture needs B's code section to be much larger than A's ({} vs {})",
        a_module.code_section.len(),
        b_module.code_section.len()
    );

    let first = linker
        .instantiate(&mut store, engine, a_module)
        .expect("module A instantiates");
    let _second = linker
        .instantiate(&mut store, engine, b_module)
        .expect("module B instantiates");

    let mut result = [Value::I64(-1)];
    let outcome = first.execute(&mut store, &[], &mut result);

    // Oracle: Wasmtime keeps A's table, whose entry 0 is null, so the call traps.
    let engine = wasmtime::Engine::default();
    let mut wasmtime_store = wasmtime::Store::new(&engine, ());
    let wasmtime_linker = wasmtime::Linker::new(&engine);
    let wasm_a = wasmtime::Module::new(&engine, wat::parse_str(DISPATCH_A).unwrap()).unwrap();
    let wasm_b = wasmtime::Module::new(&engine, wat::parse_str(&dispatch_b).unwrap()).unwrap();
    let instance_a = wasmtime_linker
        .instantiate(&mut wasmtime_store, &wasm_a)
        .unwrap();
    let _instance_b = wasmtime_linker
        .instantiate(&mut wasmtime_store, &wasm_b)
        .unwrap();
    let func_a = instance_a
        .get_typed_func::<(), i64>(&mut wasmtime_store, "main")
        .unwrap();
    assert!(
        func_a.call(&mut wasmtime_store, ()).is_err(),
        "the oracle traps because A's own table is null"
    );

    assert_eq!(
        outcome,
        Err(TrapCode::IllegalOpcode),
        "the replaced handle must trap before any dispatch through the replacement's table"
    );
}

/// `R4-2`: `execute`/`execute_named` do not validate that the result buffer matches the
/// entrypoint's result count. For `(func (export "main") (result i32))` called with an empty
/// buffer, Wasmtime reports `IllegalOpcode`; rwasm returns `Ok(())` and silently drops the value in
/// release and trips the value-stack assertion at `src/vm/executor.rs:148` in a debug build —
/// although `docs/security-considerations.md` lists `ExecutionEngine::execute` among the panic-free
/// entry points.
#[test]
fn a_result_buffer_of_the_wrong_length_is_reported_not_ignored() {
    const RESULT_I32: &str = r#"(module
         (memory (export "memory") 1)
         (func (export "main") (result i32) (i32.const 7)))"#;
    let linker = Arc::new(ImportLinker::default());
    let engine = ExecutionEngine::new();
    let module = module(&linker, RESULT_I32);
    let mut store = RwasmStore::new(
        linker.clone(),
        (),
        always_failing_syscall_handler,
        Some(1_000_000),
        None,
    );
    let instance = linker
        .instantiate(&mut store, engine, module)
        .expect("the module instantiates");
    let mut empty: [Value; 0] = [];
    let rwasm = instance.execute(&mut store, &[], &mut empty);

    let config = config(&linker);
    let wasmtime =
        StrategyDefinition::new_as_wasmtime(config, wat::parse_str(RESULT_I32).unwrap(), None)
            .expect("wasmtime compiles the module")
            .create_executor(
                linker.clone(),
                (),
                always_failing_syscall_handler,
                Some(1_000_000),
                None,
            )
            .expect("wasmtime instantiates the module");
    let mut wasmtime = wasmtime;
    let mut empty: [Value; 0] = [];
    let wasmtime = wasmtime.execute("main", &[], &mut empty);

    assert_eq!(
        wasmtime,
        Err(TrapCode::IllegalOpcode),
        "the oracle validates the result buffer"
    );
    assert_eq!(
        rwasm, wasmtime,
        "a wrong-sized result buffer must be reported like the oracle instead of being ignored \
         (rwasm returned {rwasm:?}; in a debug build this call panics instead)"
    );
}
