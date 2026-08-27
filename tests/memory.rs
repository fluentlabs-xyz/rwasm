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

/// The compile-time page check in `SegmentBuilder::add_memory_pages` bounds the module's initial
/// memory by `CompilationConfig::max_allowed_memory_pages`, but the `memory.grow` it emits into the
/// entrypoint prologue is bounded at runtime by `RwasmStore::max_allowed_memory_pages` — an
/// independent knob. When the store's limit is the smaller of the two, that grow fails, and the
/// failure used to be dropped unchecked: instantiation reported success and the module then ran on
/// a 0-page memory, so `memory.size` silently read 0 instead of the declared page count.
///
/// Found by the differential fuzzer (`fuzz/artifacts/differential/crash-d87768b3f90ac…`), where
/// wasmtime instantiated the same module and rwasm reported `MemoryOutOfBounds` from an unrelated
/// later access.
mod initial_memory_exceeding_store_limit {
    use rwasm::{
        always_failing_syscall_handler, CompilationConfig, ExecutionEngine, ImportLinker,
        RwasmModule, RwasmStore, TrapCode, Value, N_DEFAULT_MAX_MEMORY_PAGES,
    };
    use std::sync::Arc;

    /// One page more than the store's default cap, so the prologue grow cannot succeed on a
    /// default store. The compiler is configured to allow exactly this many pages, so the module
    /// compiles and only the store's independent limit stands in the way.
    const PAGES: u32 = N_DEFAULT_MAX_MEMORY_PAGES + 1;

    fn compile() -> RwasmModule {
        let wat = format!(
            r#"(module (memory {PAGES}) (func (export "size") (result i32) memory.size))"#
        );
        let wasm = wat::parse_str(wat).expect("valid WAT");
        let config = CompilationConfig::default()
            .with_entrypoint_name("size".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_max_allowed_memory_pages(PAGES);
        let (module, _) = RwasmModule::compile(config, &wasm).expect("module compiles");
        module
    }

    fn store(max_allowed_memory_pages: Option<u32>) -> RwasmStore<()> {
        RwasmStore::new(
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            None,
            max_allowed_memory_pages,
        )
    }

    #[test]
    fn entrypoint_traps_instead_of_leaving_memory_unallocated() {
        let module = compile();
        let engine = ExecutionEngine::new();
        let mut store = store(None);

        assert_eq!(
            engine.entrypoint(&mut store, &module),
            Err(TrapCode::MemoryOutOfBounds),
            "instantiation must fail when the initial memory exceeds the store's page limit"
        );
    }

    #[test]
    fn memory_is_fully_materialized_when_the_store_allows_it() {
        let module = compile();
        let engine = ExecutionEngine::new();
        let mut store = store(Some(PAGES));

        engine
            .entrypoint(&mut store, &module)
            .expect("instantiation succeeds when the store's limit covers the initial memory");

        let mut result = [Value::I32(0); 1];
        engine
            .execute(&mut store, &module, &[], &mut result)
            .expect("memory.size does not trap");
        assert_eq!(result[0].i32(), Some(PAGES as i32));
    }
}
