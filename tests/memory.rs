use rwasm::{
    always_failing_syscall_handler, instruction_set, CompilationConfig, ExecutionEngine,
    ImportLinker, RwasmModule, RwasmModuleBuilder, RwasmStore, TrapCode, Value,
};

fn execute_module(module: &RwasmModule) -> u64 {
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::new(
        ImportLinker::default().into(),
        (),
        always_failing_syscall_handler,
        None,
        None,
    );
    engine.execute(&mut store, module, &[], &mut []).unwrap();
    store.fuel_consumed()
}

#[test]
fn test_memory_fuel_ddos_not_possible() {
    let code_section = instruction_set! {
         // memory.grow
        I32Const(1)
        MemoryGrow
        Drop
        // memory.init
        I32Const(0) // d
        I32Const(0) // s
        I32Const(3) // n
        .op_memory_init_checked(None, None, 1u32, true) // 1 fuel cost
        // memory.fill
        I32Const(0) // d
        I32Const(0xff) // val
        I32Const(3) // n
        .op_memory_fill_checked(true) // 1 fuel cost
        // memory.copy
        I32Const(0) // d
        I32Const(0xff) // s
        I32Const(3) // n
        .op_memory_copy_checked(true) // 1 fuel cost
        // always terminate
        Return
    };
    let rwasm_module = RwasmModuleBuilder::new(code_section)
        .with_data_section(&[0x01, 0x02, 0x03])
        .build();
    println!("{}", rwasm_module);
    let fuel_consumed = execute_module(&rwasm_module);
    assert_eq!(fuel_consumed, 3);
}

/// Compiles a WAT module exporting a `test` function with an `i32` result and runs it.
fn execute_wat(wat_str: &str) -> Result<i32, TrapCode> {
    let wasm_binary = wat::parse_str(wat_str).expect("valid WAT");
    let config = CompilationConfig::default()
        .with_entrypoint_name("test".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::default();
    let instance = ImportLinker::default()
        .instantiate(&mut store, ExecutionEngine::new(), rwasm_module)
        .unwrap();
    let mut result = [Value::I32(0); 1];
    instance.execute(&mut store, &[], &mut result)?;
    Ok(result[0].i32().unwrap())
}

/// `memory.init x` must trap when `s + n > len(data[x])`, so any `n > 0` on an empty
/// passive segment always traps. All data segments are merged into one flat data section,
/// so without the injected per-segment guard an empty segment reads its neighbor's bytes.
#[test]
fn test_memory_init_empty_first_segment_traps() {
    assert_eq!(
        execute_wat(
            r#"
(module
  (memory 1)
  (data "")          ;; segment 0, empty
  (data "ABCD")
  (func (export "test") (result i32)
    (memory.init 0 (i32.const 0) (i32.const 0) (i32.const 4))
    (i32.load (i32.const 0))))
"#
        ),
        Err(TrapCode::MemoryOutOfBounds),
    );
}

/// Same as above, but the empty segment is not first: the offset rewrite applies, so a
/// missing length guard leaks the *following* segment's bytes instead of the first one's.
#[test]
fn test_memory_init_empty_middle_segment_traps() {
    assert_eq!(
        execute_wat(
            r#"
(module
  (memory 1)
  (data "WXYZ")
  (data "")          ;; segment 1, empty
  (data "1234")
  (func (export "test") (result i32)
    (memory.init 1 (i32.const 0) (i32.const 0) (i32.const 4))
    (i32.load (i32.const 0))))
"#
        ),
        Err(TrapCode::MemoryOutOfBounds),
    );
}

/// A zero-length `memory.init` on an empty segment is well-defined and must not trap.
#[test]
fn test_memory_init_empty_segment_zero_length_is_ok() {
    assert_eq!(
        execute_wat(
            r#"
(module
  (memory 1)
  (data "")          ;; segment 0, empty
  (data "ABCD")
  (func (export "test") (result i32)
    (memory.init 0 (i32.const 0) (i32.const 0) (i32.const 0))
    (i32.load (i32.const 0))))
"#
        ),
        Ok(0),
    );
}

/// Control: a non-empty segment still initializes memory from its own bytes.
#[test]
fn test_memory_init_non_empty_segment_still_copies() {
    assert_eq!(
        execute_wat(
            r#"
(module
  (memory 1)
  (data "WXYZ")
  (data "1234")
  (func (export "test") (result i32)
    (memory.init 1 (i32.const 0) (i32.const 0) (i32.const 4))
    (i32.load (i32.const 0))))
"#
        ),
        Ok(0x34333231),
    );
}
