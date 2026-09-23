use rwasm::{
    always_failing_syscall_handler, intrinsic::Intrinsic, CompilationConfig, ExecutionEngine,
    ImportLinker, ImportName, Opcode, RwasmModule, RwasmStore,
};
use std::sync::Arc;
use wasmparser::ValType;

#[test]
fn test_intrinsic_replace() {
    let wasm_binary = wat::parse_str(
        r#"
(module
  (import "env" "consume_fuel" (func $consume_fuel (param i32)))

  (func (export "call_gas")
    i32.const 35
    call $consume_fuel
  )
)
"#,
    )
    .unwrap();
    let mut import_linker = ImportLinker::default();
    import_linker.insert_intrinsic(
        ImportName::new("env", "consume_fuel"),
        71,
        Intrinsic::Replace(vec![Opcode::ConsumeFuelStack]),
        &[ValType::I32],
        &[],
    );
    let import_linker = Arc::new(import_linker);

    let config = CompilationConfig::default()
        .with_entrypoint_name("call_gas".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(import_linker)
        .with_consume_fuel(true);

    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::new(
        Arc::new(ImportLinker::default()),
        (),
        always_failing_syscall_handler,
        Some(100),
        None,
    );
    let engine = ExecutionEngine::new();
    engine
        .execute(&mut store, &rwasm_module, &[], &mut [])
        .unwrap();
    // 35 by consume_fuel, 10 by call, and 2 by base opcodes.
    assert_eq!(store.fuel_consumed(), 35 + 10 + 2);
}

#[test]
fn test_intrinsic_remove() {
    let wasm_binary = wat::parse_str(
        r#"
(module
  (import "env" "consume_fuel" (func $consume_fuel (param i32)))

  (func (export "call_gas")
    i32.const 33
    call $consume_fuel
  )
)
"#,
    )
    .unwrap();
    let mut import_linker = ImportLinker::default();
    import_linker.insert_intrinsic(
        ImportName::new("env", "consume_fuel"),
        71,
        Intrinsic::Remove,
        &[ValType::I32],
        &[],
    );
    let import_linker = Arc::new(import_linker);

    let config = CompilationConfig::default()
        .with_entrypoint_name("call_gas".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(import_linker)
        .with_consume_fuel(true);

    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::new(
        Arc::new(ImportLinker::default()),
        (),
        always_failing_syscall_handler,
        None,
        None,
    );
    let engine = ExecutionEngine::new();
    engine
        .execute(&mut store, &rwasm_module, &[], &mut [])
        .unwrap();
}

mod intrinsic_tail_call {
    //! MEDIUM (host must register an `Intrinsic`): `return_call` to an intrinsic import emitted
    //! the intrinsic's replacement (or the parameter drops) but no `Return`, and the translator
    //! then treats the rest of the body as unreachable, so the function ended without one. The
    //! interpreter fell through into whatever function follows in the code section.

    use rwasm::{
        always_failing_syscall_handler, intrinsic::Intrinsic, CompilationConfig, ExecutionEngine,
        ImportLinker, ImportName, Opcode, RwasmModule, RwasmStore, StoreTr,
    };
    use std::sync::Arc;
    use wasmparser::ValType;

    #[test]
    fn return_call_to_an_intrinsic_returns() {
        // `victim` is laid out right after `main` in the code section and must never run
        let wasm = wat::parse_str(
            r#"(module
                (import "env" "consume_fuel" (func $consume_fuel (param i32)))
                (memory (export "memory") 1)
                (func (export "main") (i32.const 5) (return_call $consume_fuel))
                (func (export "victim") (i32.store (i32.const 0) (i32.const 0xdeadbeef))))"#,
        )
        .unwrap();
        for (name, intrinsic) in [
            (
                "replace",
                Intrinsic::Replace(vec![Opcode::ConsumeFuelStack]),
            ),
            ("remove", Intrinsic::Remove),
        ] {
            let mut linker = ImportLinker::default();
            linker.insert_intrinsic(
                ImportName::new("env", "consume_fuel"),
                71,
                intrinsic,
                &[ValType::I32],
                &[],
            );
            let linker = Arc::new(linker);
            let config = CompilationConfig::default()
                .with_entrypoint_name("main".into())
                .with_import_linker(linker.clone());
            let (module, _) = RwasmModule::compile(config, &wasm).unwrap();
            let mut store = RwasmStore::<()>::new(
                linker.clone(),
                (),
                always_failing_syscall_handler,
                Some(1_000_000),
                None,
            );
            let instance = linker
                .instantiate(&mut store, ExecutionEngine::new(), module)
                .unwrap();
            let outcome = instance.execute(&mut store, &[], &mut []);
            let mut word = [0u8; 4];
            store.memory_read(0, &mut word).unwrap();
            assert_eq!(outcome, Ok(()), "{name}");
            assert_eq!(
                u32::from_le_bytes(word),
                0,
                "{name}: `main` fell through into `victim`"
            );
        }
    }
}

mod intrinsic_trampoline {
    //! A direct call splices an intrinsic into the caller, but the trampoline behind `ref.func`,
    //! element segments and `call_indirect` used to make the syscall the intrinsic stands in for,
    //! which the host does not serve for an intrinsic import: the call failed with
    //! `UnknownExternalFunction`, or skipped the intrinsic's metering under a permissive host.

    use rwasm::{
        always_failing_syscall_handler, intrinsic::Intrinsic, CompilationConfig, ExecutionEngine,
        ImportLinker, ImportName, Opcode, RwasmModule, RwasmStore, TrapCode,
    };
    use std::sync::Arc;
    use wasmparser::ValType;

    const WAT: &str = r#"
        (module
          (type $consume (func (param i32)))
          (import "env" "consume_fuel" (func $consume_fuel (type $consume)))
          (table 1 funcref)
          (elem (i32.const 0) $consume_fuel)
          (func (export "call_gas")
            i32.const 35
            i32.const 0
            call_indirect (type $consume)))
    "#;

    /// Runs `call_gas` and returns the fuel it consumed.
    fn run(intrinsic: Intrinsic) -> Result<u64, TrapCode> {
        let mut import_linker = ImportLinker::default();
        import_linker.insert_intrinsic(
            ImportName::new("env", "consume_fuel"),
            71,
            intrinsic,
            &[ValType::I32],
            &[],
        );
        let config = CompilationConfig::default()
            .with_entrypoint_name("call_gas".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_import_linker(Arc::new(import_linker))
            .with_consume_fuel(true);
        let (rwasm_module, _) =
            RwasmModule::compile(config, &wat::parse_str(WAT).unwrap()).unwrap();
        assert!(
            !rwasm_module
                .code_section
                .iter()
                .any(|opcode| matches!(opcode, Opcode::Call(_))),
            "the trampoline must not call the syscall: {rwasm_module}"
        );
        let linker = Arc::new(ImportLinker::default());
        let mut store = RwasmStore::<()>::new(
            linker.clone(),
            (),
            always_failing_syscall_handler,
            Some(10_000),
            None,
        );
        // instantiation fills the table
        let instance = linker.instantiate(&mut store, ExecutionEngine::new(), rwasm_module)?;
        let before = store.fuel_consumed();
        instance.execute(&mut store, &[], &mut [])?;
        Ok(store.fuel_consumed() - before)
    }

    #[test]
    fn call_indirect_to_a_replaced_intrinsic_runs_the_replacement() {
        let consumed = run(Intrinsic::Replace(vec![Opcode::ConsumeFuelStack])).unwrap();
        assert!(
            consumed >= 35,
            "the replacement charged its argument: {consumed}"
        );
    }

    #[test]
    fn call_indirect_to_a_removed_intrinsic_drops_the_arguments() {
        let consumed = run(Intrinsic::Remove).unwrap();
        assert!(consumed < 35, "nothing charged the argument: {consumed}");
    }
}
