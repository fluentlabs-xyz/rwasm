//! Acceptance limits shared by both execution strategies.
//!
//! These modules are legal Wasm, and every case below used to be accepted by the compiler while
//! the two backends disagreed at run time (an empty table, a trap on one side only). The compiler
//! now rejects them, so a module is either runnable on both strategies or rejected everywhere.

use rwasm::{
    CompilationConfig, CompilationError, RwasmModule, StoreTr, StrategyDefinition, N_MAX_TABLE_SIZE,
};

fn config() -> CompilationConfig {
    CompilationConfig::default_strategy_compatible().with_entrypoint_name("main".into())
}

/// A declared table larger than the runtime can materialize used to come out empty on rwasm
/// (`table.size` returned 0) while the Wasmtime backend reported the declared size.
#[test]
fn oversized_table_is_rejected() {
    let wasm = wat::parse_str(format!(
        r#"(module (table {} funcref)
             (func (export "main") (result i32) (table.size 0)))"#,
        N_MAX_TABLE_SIZE + 1
    ))
    .unwrap();
    let err = RwasmModule::compile(config(), &wasm).expect_err("must be rejected");
    assert!(
        matches!(
            err,
            CompilationError::TableSizeExceedsLimit {
                size,
                limit,
            } if size == N_MAX_TABLE_SIZE + 1 && limit == N_MAX_TABLE_SIZE
        ),
        "unexpected error: {err}"
    );
    assert!(StrategyDefinition::new_as_wasmtime(config(), &wasm, None).is_err());
}

/// The limit itself is inclusive: a table of exactly `N_MAX_TABLE_SIZE` elements runs on both
/// backends and reports its size.
#[test]
fn table_at_the_limit_is_accepted_by_both() {
    let wasm = wat::parse_str(format!(
        r#"(module (table {} funcref)
             (func (export "main") (result i32) (table.size 0)))"#,
        N_MAX_TABLE_SIZE
    ))
    .unwrap();
    for definition in [
        StrategyDefinition::new_as_rwasm(config(), &wasm).expect("rwasm compiles"),
        StrategyDefinition::new_as_wasmtime(config(), &wasm, None).expect("wasmtime compiles"),
    ] {
        let mut executor = definition
            .create_executor(
                Default::default(),
                (),
                rwasm::always_failing_syscall_handler,
                None,
                None,
            )
            .expect("instantiation");
        let mut result = [rwasm::Value::I32(-1)];
        executor.execute("main", &[], &mut result).unwrap();
        assert_eq!(result[0].i32(), Some(N_MAX_TABLE_SIZE as i32));
    }
}

/// `table.grow` used to always report an overflow for a declared maximum >= 2^31, because the
/// guard compared a signed `i32` built from that maximum. It is clamped to the runtime cap now.
#[test]
fn table_grow_with_a_huge_declared_maximum_grows_up_to_the_cap() {
    let wasm = wat::parse_str(
        r#"(module (table 1 3000000000 funcref)
             (func (export "main") (param i32) (result i32)
               (table.grow 0 (ref.null func) (local.get 0))))"#,
    )
    .unwrap();
    for definition in [
        StrategyDefinition::new_as_rwasm(config(), &wasm).expect("rwasm compiles"),
        StrategyDefinition::new_as_wasmtime(config(), &wasm, None).expect("wasmtime compiles"),
    ] {
        let mut executor = definition
            .create_executor(
                Default::default(),
                (),
                rwasm::always_failing_syscall_handler,
                None,
                None,
            )
            .expect("instantiation");
        // One element fits: the result is the previous size.
        let mut result = [rwasm::Value::I32(-1)];
        executor
            .execute("main", &[rwasm::Value::I32(1)], &mut result)
            .unwrap();
        assert_eq!(result[0].i32(), Some(1));
        // Growing past the runtime cap reports the failure sentinel on both backends.
        let mut result = [rwasm::Value::I32(0)];
        executor
            .execute("main", &[rwasm::Value::I32(i32::MAX)], &mut result)
            .unwrap();
        assert_eq!(result[0].i32(), Some(-1));
    }
}

/// A function whose operand stack exceeds the runtime window used to trap on rwasm and return a
/// value on the Wasmtime backend.
#[test]
fn function_above_the_value_stack_limit_is_rejected() {
    let body = format!(
        "(func (export \"main\") (result i32) {} {} i32.const 42)",
        "i32.const 1 ".repeat(8200),
        "drop ".repeat(8200)
    );
    let wasm = wat::parse_str(format!("(module {body})")).unwrap();
    let err = RwasmModule::compile(config(), &wasm).expect_err("must be rejected");
    assert!(
        matches!(err, CompilationError::StackHeightExceeded { .. }),
        "unexpected error: {err}"
    );
    assert!(StrategyDefinition::new_as_wasmtime(config(), &wasm, None).is_err());
}

/// A function that stays inside the window still compiles and runs on both backends.
#[test]
fn function_at_the_value_stack_limit_is_accepted() {
    let body = "(func (export \"main\") (result i32) i32.const 42)";
    let wasm = wat::parse_str(format!("(module {body})")).unwrap();
    let mut result = [rwasm::Value::I32(0)];
    StrategyDefinition::new_as_rwasm(config(), &wasm)
        .unwrap()
        .create_executor(
            Default::default(),
            (),
            rwasm::always_failing_syscall_handler,
            None,
            None,
        )
        .unwrap()
        .execute("main", &[], &mut result)
        .unwrap();
    assert_eq!(result[0].i32(), Some(42));
}

/// The rwasm VM always has its memory at index 0, so it accepts a module that declares a memory
/// without exporting it. The Wasmtime backend can only reach an instance memory through the
/// module's exports, so compiling such a module for that strategy fails instead of letting host
/// memory access succeed on one backend and fail on the other.
#[test]
fn module_with_an_unexported_memory_is_rejected_by_the_wasmtime_strategy() {
    let wasm = wat::parse_str(r#"(module (memory 1) (func (export "main")))"#).unwrap();
    assert!(
        StrategyDefinition::new_as_rwasm(config(), &wasm).is_ok(),
        "the rwasm backend reaches memory 0 without an export"
    );
    let err = match StrategyDefinition::new_as_wasmtime(config(), &wasm, None) {
        Ok(_) => panic!("the wasmtime strategy must reject an unexported memory"),
        Err(err) => err,
    };
    assert!(
        matches!(err, CompilationError::MissingMemoryExport),
        "unexpected error: {err}"
    );
}

#[test]
fn for_each_strategy_rejects_unexported_memory_before_the_wasmtime_callback() {
    let wasm = wat::parse_str(r#"(module (memory 1) (func (export "main")))"#).unwrap();
    let mut callbacks = 0;
    let result = rwasm::for_each_strategy(
        |definition| {
            callbacks += 1;
            assert!(matches!(definition, StrategyDefinition::Rwasm { .. }));
            let mut executor = definition.default_executor()?;
            executor.memory_write(0, &[42])?;
            Ok(())
        },
        config(),
        &wasm,
    );
    assert!(matches!(
        result,
        Err(rwasm::StrategyError::CompilationError(
            CompilationError::MissingMemoryExport
        ))
    ));
    assert_eq!(callbacks, 1, "only the rwasm callback may run");
}

#[test]
fn for_each_strategy_accepts_exported_memory_and_memoryless_modules() {
    for memory in ["", r#"(memory (export "mem") 1)"#] {
        let wasm = wat::parse_str(format!(r#"(module {memory} (func (export "main")))"#)).unwrap();
        let results = rwasm::for_each_strategy(
            |definition| {
                let is_rwasm = matches!(definition, StrategyDefinition::Rwasm { .. });
                let mut executor = definition.default_executor()?;
                executor.execute("main", &[], &mut [])?;
                if !memory.is_empty() {
                    executor.memory_write(0, &[42])?;
                    let mut bytes = [0];
                    executor.memory_read(0, &mut bytes)?;
                    assert_eq!(bytes, [42]);
                }
                Ok(is_rwasm)
            },
            config(),
            &wasm,
        )
        .unwrap();
        assert_eq!(results, [true, false]);
    }
}

/// A memory exported under any name is reachable by both backends.
#[test]
fn memory_exported_under_any_name_is_reachable() {
    let wasm = wat::parse_str(
        r#"(module (memory (export "mem") 1)
             (func (export "main") (i32.store (i32.const 0) (i32.const 0x04030201))))"#,
    )
    .unwrap();
    for definition in [
        StrategyDefinition::new_as_rwasm(config(), &wasm).expect("rwasm compiles"),
        StrategyDefinition::new_as_wasmtime(config(), &wasm, None).expect("wasmtime compiles"),
    ] {
        let mut executor = definition
            .create_executor(
                Default::default(),
                (),
                rwasm::always_failing_syscall_handler,
                None,
                None,
            )
            .expect("instantiation");
        executor.execute("main", &[], &mut []).unwrap();
        let mut buffer = [0_u8; 4];
        executor.memory_read(0, &mut buffer).unwrap();
        assert_eq!(buffer, [1, 2, 3, 4]);
    }
}

/// A module without a linear memory keeps working on both strategies.
#[test]
fn module_without_memory_is_accepted() {
    let wasm =
        wat::parse_str(r#"(module (func (export "main") (result i32) (i32.const 5)))"#).unwrap();
    for definition in [
        StrategyDefinition::new_as_rwasm(config(), &wasm).expect("rwasm compiles"),
        StrategyDefinition::new_as_wasmtime(config(), &wasm, None).expect("wasmtime compiles"),
    ] {
        let mut executor = definition
            .create_executor(
                Default::default(),
                (),
                rwasm::always_failing_syscall_handler,
                None,
                None,
            )
            .expect("instantiation");
        let mut result = [rwasm::Value::I32(0)];
        executor.execute("main", &[], &mut result).unwrap();
        assert_eq!(result[0].i32(), Some(5));
    }
}

/// The strategy layer exposes one entrypoint on both backends: a call that names a different
/// export used to run the configured entrypoint on rwasm and the named export on Wasmtime.
#[test]
fn calling_another_export_name_fails_on_both_backends() {
    let wasm = wat::parse_str(
        r#"(module
             (func (export "main") (result i32) (i32.const 1))
             (func (export "other") (result i32) (i32.const 2)))"#,
    )
    .unwrap();
    for definition in [
        StrategyDefinition::new_as_rwasm(config(), &wasm).expect("rwasm compiles"),
        StrategyDefinition::new_as_wasmtime(config(), &wasm, None).expect("wasmtime compiles"),
    ] {
        let mut executor = definition
            .create_executor(
                Default::default(),
                (),
                rwasm::always_failing_syscall_handler,
                None,
                None,
            )
            .expect("instantiation");
        // The configured entrypoint keeps working.
        let mut result = [rwasm::Value::I32(0)];
        executor.execute("main", &[], &mut result).unwrap();
        assert_eq!(result[0].i32(), Some(1));
        // Any other name is rejected instead of executing something else.
        assert_eq!(
            executor.execute("other", &[], &mut result),
            Err(rwasm::TrapCode::UnknownExternalFunction)
        );
    }
}

/// A config that charges fuel the Wasmtime engine does not implement would make the same module
/// burn different fuel per strategy. It is rejected at compile time instead.
#[test]
fn strategy_divergent_fuel_config_is_rejected() {
    let wasm = wat::parse_str(r#"(module (func (export "main")))"#).unwrap();
    let divergent = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_consume_fuel_for_bulk_ops(true);
    assert!(!divergent.is_strategy_compatible());
    let err = match StrategyDefinition::new_as_wasmtime(divergent.clone(), &wasm, None) {
        Ok(_) => panic!("a divergent fuel config must be rejected"),
        Err(err) => err,
    };
    assert!(
        matches!(err, CompilationError::StrategyIncompatibleConfig),
        "unexpected error: {err}"
    );
    // The rwasm backend keeps supporting it, and the compatible config works on both.
    assert!(StrategyDefinition::new_as_rwasm(divergent, &wasm).is_ok());
    assert!(StrategyDefinition::new_as_wasmtime(config(), &wasm, None).is_ok());
}

/// `resume` used to panic on the Wasmtime backend and to hit an `unreachable!` on rwasm when no
/// execution was interrupted. Both engines now report the same trap.
#[test]
fn resume_without_an_interruption_reports_a_trap() {
    let wasm = wat::parse_str(r#"(module (func (export "main")))"#).unwrap();
    for definition in [
        StrategyDefinition::new_as_rwasm(config(), &wasm).expect("rwasm compiles"),
        StrategyDefinition::new_as_wasmtime(config(), &wasm, None).expect("wasmtime compiles"),
    ] {
        let mut executor = definition
            .create_executor(
                Default::default(),
                (),
                rwasm::always_failing_syscall_handler,
                None,
                None,
            )
            .expect("instantiation");
        assert_eq!(
            executor.resume(&[], &mut []),
            Err(rwasm::TrapCode::IllegalOpcode)
        );
    }
}

/// The compiled-module cache must not hand back a module compiled from different bytes, even when
/// the caller reuses its caching key.
#[test]
fn module_cache_key_covers_the_bytecode() {
    let first =
        wat::parse_str(r#"(module (func (export "main") (result i32) (i32.const 11)))"#).unwrap();
    let second =
        wat::parse_str(r#"(module (func (export "main") (result i32) (i32.const 22)))"#).unwrap();
    let cache_key = Some([7u8; 32]);

    for (wasm, expected) in [(&first, 11), (&second, 22)] {
        let definition = StrategyDefinition::new_as_wasmtime(config(), wasm, cache_key).unwrap();
        let mut executor = definition
            .create_executor(
                Default::default(),
                (),
                rwasm::always_failing_syscall_handler,
                None,
                None,
            )
            .expect("instantiation");
        let mut result = [rwasm::Value::I32(0)];
        executor.execute("main", &[], &mut result).unwrap();
        assert_eq!(result[0].i32(), Some(expected));
    }
}

/// A `br_table` whose targets keep 1000 values, as used by the code-size tests below: every
/// entry that has to drop the value underneath the results costs a `2·1000 + 1` instruction
/// trampoline, so the emitted size is bounded by the number of *distinct* targets and the
/// configured code bound, not by the input size.
fn br_table_wasm(distinct_targets: usize, entries: usize) -> Vec<u8> {
    let results = vec!["i32"; 1000].join(" ");
    let mut body = String::from("(i32.const 7)\n");
    for _ in 0..1000 {
        body.push_str("(i32.const 1)\n");
    }
    body.push_str("(local.get 0)\n(br_table");
    for entry in 0..entries {
        body.push_str(&format!(" {}", entry % distinct_targets));
    }
    body.push_str(" 0)\n");
    let open = format!("(block (result {results})\n").repeat(distinct_targets);
    wat::parse_str(format!(
        r#"(module
          (func (export "main") (param i32) (result i32)
            {open}{body}{close}
            {drops}))"#,
        close = ")".repeat(distinct_targets),
        drops = "drop ".repeat(999)
    ))
    .unwrap()
}

/// Every branch that keeps values expands into a trampoline, and the expansion is not bounded
/// by the input: a 1 MiB `br_table` module used to compile to ~16 GiB of bytecode. The
/// compiler now stops at `max_code_len` while emitting, so the rejected module never costs more
/// than the bound, and the Wasmtime strategy (which runs the rwasm front end first) rejects it
/// too.
#[test]
fn code_size_bound_rejects_expanding_modules_on_both_strategies() {
    // 1000 distinct targets: dedup cannot help, 1000 trampolines of 2001 instructions
    let wasm = br_table_wasm(1000, 1000);
    let bounded = config().with_max_code_len(1_000_000);
    let err = RwasmModule::compile(bounded.clone(), &wasm).expect_err("must be rejected");
    match err {
        CompilationError::CodeSizeExceeded { len, limit } => {
            assert_eq!(limit, 1_000_000);
            // stopped at the bound, not after building the whole table
            assert!(len > limit && len <= limit + 2_002, "len={len}");
        }
        err => panic!("unexpected error: {err}"),
    }
    assert!(matches!(
        StrategyDefinition::new_as_wasmtime(bounded, &wasm, None),
        Err(CompilationError::CodeSizeExceeded { .. })
    ));
    // the same module fits a bound above its real size
    let module = RwasmModule::compile(config().with_max_code_len(3_000_000), &wasm)
        .expect("fits the bound")
        .0;
    assert!(module.code_section.len() > 2_000_000);
    assert!(module.code_section.len() <= 3_000_000);
}

/// The bound also covers ordinary code: a limit below the module's size rejects it, the limit
/// itself is inclusive, and the default admits every module the runtime can hold.
#[test]
fn code_size_bound_is_inclusive() {
    let wasm = wat::parse_str(
        r#"(module (func (export "main") (result i32) (i32.add (i32.const 1) (i32.const 2))))"#,
    )
    .unwrap();
    let len = RwasmModule::compile(config(), &wasm)
        .unwrap()
        .0
        .code_section
        .len() as u32;
    assert!(RwasmModule::compile(config().with_max_code_len(len), &wasm).is_ok());
    assert!(matches!(
        RwasmModule::compile(config().with_max_code_len(len - 1), &wasm),
        Err(CompilationError::CodeSizeExceeded { limit, .. }) if limit == len - 1
    ));
}

/// Entries of a `br_table` with the same target share one trampoline: 100 000 entries naming
/// one block cost one `2001`-instruction trampoline, not 100 000 of them, and the table still
/// dispatches every entry to it.
#[test]
fn br_table_entries_with_the_same_target_share_a_trampoline() {
    let wasm = br_table_wasm(1, 100_000);
    let module = RwasmModule::compile(config(), &wasm)
        .expect("a deduplicated table fits the default bound")
        .0;
    // the 100 001 two-word entries plus a single trampoline
    assert!(module.code_section.len() < 2 * 100_001 + 2_001 + 2_100);
    for definition in [
        StrategyDefinition::new_as_rwasm(config(), &wasm).expect("rwasm compiles"),
        StrategyDefinition::new_as_wasmtime(config(), &wasm, None).expect("wasmtime compiles"),
    ] {
        let mut executor = definition
            .create_executor(
                Default::default(),
                (),
                rwasm::always_failing_syscall_handler,
                None,
                None,
            )
            .expect("instantiation");
        for index in [0, 1, 50_000, 99_999, 100_000, 7_000_000] {
            let mut result = [rwasm::Value::I32(-1)];
            executor
                .execute("main", &[rwasm::Value::I32(index)], &mut result)
                .unwrap();
            assert_eq!(result[0].i32(), Some(1), "entry {index}");
        }
    }
}

/// `memory.grow` is bounded by the compile-time page cap on both backends. The rwasm compiler
/// bakes the cap into every grow, while the Wasmtime store limiter used to know only the
/// run-time cap, so a module compiled under a lower cap could grow further there.
#[test]
fn memory_grow_is_bounded_by_the_compile_time_page_cap_on_both() {
    let wasm = wat::parse_str(
        r#"(module (memory (export "memory") 1)
             (func (export "main") (param i32) (result i32) (memory.grow (local.get 0))))"#,
    )
    .unwrap();
    // (run-time cap, grow deltas, expected results, final size in pages)
    let cases = [
        (None, [5, 1, 1], [-1, 1, -1], 2),
        (Some(1), [1, 0, 1], [-1, 1, -1], 1),
    ];
    for (max_allowed_memory_pages, deltas, expected, pages) in cases {
        let outcomes = rwasm::for_each_strategy(
            |strategy| {
                let mut executor = strategy.create_executor(
                    Default::default(),
                    (),
                    rwasm::always_failing_syscall_handler,
                    None,
                    max_allowed_memory_pages,
                )?;
                let mut grown = Vec::new();
                for delta in deltas {
                    let mut result = [rwasm::Value::I32(0)];
                    executor.execute("main", &[rwasm::Value::I32(delta)], &mut result)?;
                    grown.push(result[0].i32().unwrap());
                }
                Ok((grown, executor.snapshot_memory()?.len()))
            },
            config().with_max_allowed_memory_pages(2),
            &wasm,
        )
        .unwrap();
        assert_eq!(outcomes[0], outcomes[1], "rwasm and wasmtime diverged");
        assert_eq!(
            outcomes[0],
            (
                expected.to_vec(),
                pages * rwasm::N_BYTES_PER_MEMORY_PAGE as usize
            ),
            "run-time cap {max_allowed_memory_pages:?}"
        );
    }
}
