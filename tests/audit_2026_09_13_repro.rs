//! Reproductions for the 2026-09-13 audit (`audits/2026-09-13-rwasm-audit.md`), one module per
//! finding group. Every test is written against the correct behaviour and was red at the audited
//! revision; all of them are green with the fixes on this branch, so the file doubles as the
//! regression suite for that report.

#[cfg(feature = "wasmtime")]
mod instance_isolation {
    //! CRIT-1 / HIGH-1.
    //!
    //! `CRIT-1`: a store holds one instance's state, but nothing stops a host from keeping two live
    //! [`RwasmInstance`] handles for two different modules on the same store. Executing the first one
    //! after the second was instantiated must be rejected before accessing the new instance's state.
    //! Wasmtime supports several instances per store; rwasm exposes one current instance and
    //! invalidates its previous handles when replacement succeeds.

    use rwasm::{
        always_failing_syscall_handler, CompilationConfig, ExecutionEngine, ImportLinker,
        RwasmModule, RwasmStore, StoreTr, StrategyDefinition, TrapCode, Value,
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

    /// `HIGH-1`: `execute`/`execute_named` do not validate that the result buffer matches the
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
}

#[cfg(feature = "wasmtime")]
mod syscall_fuel_dispatch {
    //! HIGH-2 / HIGH-3.
    //!
    //! Both findings live in the import trampoline, the one piece of generated code the differential
    //! fuzzer never exercises (`max_imports = 0`). They are written against the correct behaviour and
    //! fail until fixed:
    //!
    //! * `HIGH-2`: the syscall fuel of an import (`SyscallFuelParams`) is charged by rwasm inside the
    //!   import trampoline, so every way of reaching the import pays it. The Wasmtime strategy charges
    //!   it at Cranelift `call`/`return_call` sites only, so `call_indirect`, `return_call_indirect`,
    //!   an import exported as the entrypoint and an import used as `start` all run the builtin for
    //!   free there.
    //! * `HIGH-3`: the fuel prologue `compile_block_params` emits into the trampoline pushes up to two
    //!   (`LinearFuel`) or four (`QuadraticFuel`) temporaries that are never accounted in the
    //!   translator's stack height, so the trampoline's `StackCheck` is `0`. When the value stack is
    //!   within that many slots of its capacity at the call, rwasm traps `StackOverflow` on a module
    //!   Wasmtime executes.

    use rwasm::{
        CompilationConfig, ImportLinker, ImportName, StoreTr, StrategyDefinition, StrategyExecutor,
        SyscallFuelParams, TrapCode, TypedCaller, ValType, Value,
    };
    use rwasm_fuel_policy::{LinearFuelParams, QuadraticFuelParams};
    use std::sync::Arc;

    const CONST_FUEL: u64 = 1000;

    fn accept(
        _: &mut TypedCaller<'_, ()>,
        _: u32,
        _: &[Value],
        _: &mut [Value],
    ) -> Result<(), TrapCode> {
        Ok(())
    }

    fn linker() -> Arc<ImportLinker> {
        let mut linker = ImportLinker::default();
        linker.insert_function(
            ImportName::new("env", "flat"),
            0x11,
            SyscallFuelParams::Const(CONST_FUEL),
            &[],
            &[],
        );
        linker.insert_function(
            ImportName::new("env", "lin"),
            0x12,
            SyscallFuelParams::LinearFuel(LinearFuelParams {
                param_index: 1,
                word_cost: 3,
                base_fuel: 7,
            }),
            &[ValType::I32],
            &[],
        );
        linker.insert_function(
            ImportName::new("env", "quad"),
            0x13,
            SyscallFuelParams::QuadraticFuel(QuadraticFuelParams {
                local_depth: 1,
                word_cost: 3,
                divisor: 512,
                fuel_denom_rate: 1,
            }),
            &[ValType::I32],
            &[],
        );
        Arc::new(linker)
    }

    fn config(allow_start: bool) -> CompilationConfig {
        CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_allow_start_section(allow_start)
            .with_builtins_consume_fuel(true)
            .with_import_linker(linker())
    }

    /// Returns `(rwasm, wasmtime)` executors for `wat`.
    fn executors(wat: &str, fuel: u64, allow_start: bool) -> [StrategyExecutor<()>; 2] {
        let wasm = wat::parse_str(wat).expect("the test module parses");
        let config = config(allow_start);
        let rwasm = StrategyDefinition::new_as_rwasm(config.clone(), &wasm)
            .expect("rwasm compiles the module")
            .create_executor(linker(), (), accept, Some(fuel), None)
            .expect("rwasm instantiates the module");
        let wasmtime = StrategyDefinition::new_as_wasmtime(config, &wasm, None)
            .expect("wasmtime compiles the module")
            .create_executor(linker(), (), accept, Some(fuel), None)
            .expect("wasmtime instantiates the module");
        [rwasm, wasmtime]
    }

    fn run(
        exec: &mut StrategyExecutor<()>,
        params: &[Value],
    ) -> (Result<(), TrapCode>, Option<u64>) {
        let outcome = exec.execute("main", params, &mut []);
        (outcome, exec.remaining_fuel())
    }

    // ---------------------------------------------------------------------------------------------
    // HIGH-2
    // ---------------------------------------------------------------------------------------------

    /// Control: a direct `call` to the import charges `CONST_FUEL` on both strategies.
    #[test]
    fn direct_syscall_fuel_is_charged_on_both_strategies() {
        let wat = r#"(module
          (import "env" "flat" (func $flat))
          (memory (export "memory") 1)
          (func (export "main") call $flat))"#;
        let [mut rwasm, mut wasmtime] = executors(wat, 100_000, false);
        let rwasm = run(&mut rwasm, &[]);
        let wasmtime = run(&mut wasmtime, &[]);
        assert_eq!(rwasm, wasmtime);
        assert!(rwasm.1.unwrap() <= 100_000 - CONST_FUEL, "{rwasm:?}");
    }

    /// The same import reached through a table entry or a tail call. rwasm keeps charging
    /// `CONST_FUEL` (it lives in the trampoline), Wasmtime charges nothing.
    #[test]
    fn indirect_syscall_fuel_is_charged_on_both_strategies() {
        let paths = [
            ("call_indirect", "(call_indirect (type $t) (i32.const 0))"),
            (
                "return_call_indirect",
                "(return_call_indirect (type $t) (i32.const 0))",
            ),
            (
                "ref.func + table.set + call_indirect",
                "(table.set 0 (i32.const 1) (ref.func $flat)) (call_indirect (type $t) (i32.const 1))",
            ),
        ];
        let mut divergent = Vec::new();
        for (label, body) in paths {
            let wat = format!(
                r#"(module
                  (type $t (func))
                  (import "env" "flat" (func $flat))
                  (memory (export "memory") 1)
                  (table 2 funcref)
                  (elem (i32.const 0) $flat)
                  (func (export "main") {body}))"#
            );
            let [mut rwasm, mut wasmtime] = executors(&wat, 100_000, false);
            let rwasm = run(&mut rwasm, &[]);
            let wasmtime = run(&mut wasmtime, &[]);
            if rwasm != wasmtime {
                divergent.push((label, rwasm, wasmtime));
            }
        }
        assert!(
            divergent.is_empty(),
            "syscall fuel differs by dispatch path (label, rwasm, wasmtime): {divergent:#?}"
        );
    }

    /// An import exported as the entrypoint, and an import used as the start function: neither has a
    /// Cranelift call site, so Wasmtime never charges the syscall fuel rwasm charges.
    #[test]
    fn entrypoint_and_start_imports_charge_syscall_fuel_on_both_strategies() {
        let cases = [
            (
                "export-of-import",
                r#"(module
                  (import "env" "flat" (func $flat))
                  (memory (export "memory") 1)
                  (export "main" (func $flat)))"#,
                false,
            ),
            (
                "start-is-import",
                r#"(module
                  (import "env" "flat" (func $flat))
                  (memory (export "memory") 1)
                  (start $flat)
                  (func (export "main")))"#,
                true,
            ),
        ];
        let mut divergent = Vec::new();
        for (label, wat, allow_start) in cases {
            let [mut rwasm, mut wasmtime] = executors(wat, 100_000, allow_start);
            let rwasm = (rwasm.remaining_fuel(), run(&mut rwasm, &[]));
            let wasmtime = (wasmtime.remaining_fuel(), run(&mut wasmtime, &[]));
            if rwasm != wasmtime {
                divergent.push((label, rwasm, wasmtime));
            }
        }
        assert!(
            divergent.is_empty(),
            "(label, (fuel after instantiation, (outcome, fuel after call))): {divergent:#?}"
        );
    }

    /// The consequence: a loop of 1 MiB `LinearFuel` builtin calls through a table needs ~9.4M fuel
    /// (rwasm traps `OutOfFuel` on a 1M budget), while Wasmtime completes all 100 calls for ~2000.
    #[test]
    fn indirect_builtin_calls_cannot_bypass_fuel_on_wasmtime() {
        let wat = r#"(module
          (type $t (func (param i32)))
          (import "env" "lin" (func $lin (param i32)))
          (memory (export "memory") 1)
          (table 1 funcref)
          (elem (i32.const 0) $lin)
          (func (export "main") (param $bytes i32) (param $iters i32)
            (block
              (loop
                (br_if 1 (i32.eqz (local.get $iters)))
                (call_indirect (type $t) (local.get $bytes) (i32.const 0))
                (local.set $iters (i32.sub (local.get $iters) (i32.const 1)))
                (br 0)))))"#;
        let params = [Value::I32(1_000_000), Value::I32(100)];
        let [mut rwasm, mut wasmtime] = executors(wat, 1_000_000, false);
        let rwasm = run(&mut rwasm, &params);
        let wasmtime = run(&mut wasmtime, &params);
        assert_eq!(
            rwasm.0,
            Err(TrapCode::OutOfFuel),
            "rwasm charges the builtin: {rwasm:?}"
        );
        assert_eq!(
            wasmtime, rwasm,
            "wasmtime must not run 100 MiB of metered builtin work on a 1M budget"
        );
    }

    // ---------------------------------------------------------------------------------------------
    // HIGH-3
    // ---------------------------------------------------------------------------------------------

    /// A function whose stack peak is exactly the initial value-stack capacity (32 slots: one param,
    /// 30 locals, one argument) calling a `LinearFuel` import. The trampoline's `StackCheck(0)`
    /// reserves nothing for the two temporaries of the fuel prologue, so the first `LocalGet` lands
    /// on `ptr == end` and rwasm traps `StackOverflow`; Wasmtime returns the argument.
    #[test]
    fn linear_fuel_trampoline_reserves_its_temporaries() {
        assert_trampoline_runs_at_capacity("lin", 30);
    }

    /// Same with `QuadraticFuel`, whose prologue peaks at four temporaries.
    #[test]
    fn quadratic_fuel_trampoline_reserves_its_temporaries() {
        assert_trampoline_runs_at_capacity("quad", 27);
    }

    fn assert_trampoline_runs_at_capacity(import: &str, locals: usize) {
        let wat = format!(
            r#"(module
              (import "env" "{import}" (func $builtin (param i32)))
              (memory (export "memory") 1)
              (func (export "main") (param i32) (result i32) (local {locals})
                (call $builtin (local.get 0))
                (local.get 0)))"#,
            locals = vec!["i32"; locals].join(" ")
        );
        let [mut rwasm, mut wasmtime] = executors(&wat, 1_000_000, false);
        let mut outcomes = Vec::new();
        for exec in [&mut rwasm, &mut wasmtime] {
            let mut result = [Value::I32(0)];
            let outcome = exec.execute("main", &[Value::I32(64)], &mut result);
            outcomes.push((outcome, result[0].clone()));
        }
        assert_eq!(
            outcomes[1],
            (Ok(()), Value::I32(64)),
            "wasmtime runs the module: {outcomes:?}"
        );
        assert_eq!(
            outcomes[0], outcomes[1],
            "rwasm must not trap on a stack peak the compiler accepted (rwasm, wasmtime): {outcomes:?}"
        );
    }
}

mod code_size_bound {
    //! HIGH-4: the translator had no bound on emitted code, so a small input compiled to an
    //! enormous module.
    //!
    //! Both tests assert that the emitted code stays within a sane instruction budget for the input,
    //! either by rejecting the module with a compile-time size limit or by emitting a bounded amount.
    //! They failed at the audited revision: a `br_table` emitted ~2001 instructions (16 KB) per
    //! one-byte target and a `br_if` with a 1000-value `DropKeep` the same per occurrence, with no
    //! limit anywhere in the compiler.
    //!
    //! The bound used here is 2,000,000 instructions, the order of magnitude the host's
    //! `RWASM_MAX_CODE_SIZE` (12 MiB) implies.

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
}

#[cfg(feature = "wasmtime")]
mod metered_import_parameters {
    //! HIGH-5 / HIGH-6: regressions for the syscall-fuel verification findings.
    //!
    //! Metered lengths must be `i32` parameters, and valid calls at the compiler's stack limit must
    //! execute on both strategies. Pin rejection types, results, host arguments, and fuel explicitly:
    //! agreement alone could hide the same failure or missing charge on both backends.

    use rwasm::{
        CompilationConfig, CompilationError, ImportLinker, ImportName, StoreTr, StrategyDefinition,
        SyscallFuelParams, TrapCode, TypedCaller, ValType, Value, N_MAX_STACK_SIZE,
    };
    use rwasm_fuel_policy::{LinearFuelParams, QuadraticFuelParams};
    use std::sync::Arc;

    const INITIAL_FUEL: u64 = 100_000_000;

    #[derive(Debug, Default, PartialEq)]
    struct HostCalls {
        count: usize,
        params: Vec<Value>,
    }

    fn handler(
        caller: &mut TypedCaller<'_, HostCalls>,
        _: u32,
        params: &[Value],
        _: &mut [Value],
    ) -> Result<(), TrapCode> {
        let calls = caller.data_mut();
        calls.count += 1;
        calls.params = params.to_vec();
        Ok(())
    }

    /// Both policies meter 9 bytes (one word); keep these charges independent of the implementation.
    fn policies(param_index: u32) -> [(&'static str, SyscallFuelParams, u64); 2] {
        [
            (
                "linear",
                SyscallFuelParams::LinearFuel(LinearFuelParams {
                    base_fuel: 3,
                    param_index,
                    word_cost: 5,
                }),
                8, // 3 + 5 * 1
            ),
            (
                "quadratic",
                SyscallFuelParams::QuadraticFuel(QuadraticFuelParams {
                    local_depth: param_index,
                    word_cost: 3,
                    divisor: 2,
                    fuel_denom_rate: 4,
                }),
                12, // (3 * 1 + 1 * 1 / 2) * 4, with integer division
            ),
        ]
    }

    fn linker(policy: SyscallFuelParams, params: &'static [ValType]) -> Arc<ImportLinker> {
        let mut linker = ImportLinker::default();
        linker.insert_function(ImportName::new("env", "imp"), 0x71, policy, params, &[]);
        Arc::new(linker)
    }

    fn definitions(
        linker: &Arc<ImportLinker>,
        wasm: &[u8],
    ) -> [(&'static str, Result<StrategyDefinition, CompilationError>); 2] {
        let config = CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_builtins_consume_fuel(true)
            .with_import_linker(linker.clone());
        [
            (
                "rwasm",
                StrategyDefinition::new_as_rwasm(config.clone(), wasm),
            ),
            (
                "wasmtime",
                StrategyDefinition::new_as_wasmtime(config, wasm, None),
            ),
        ]
    }

    fn assert_runs(
        linker: &Arc<ImportLinker>,
        wasm: &[u8],
        params: &[Value],
        host_params: &[Value],
        expected_fuel: u64,
        label: &str,
    ) {
        for (strategy, definition) in definitions(linker, wasm) {
            let definition = definition.unwrap_or_else(|err| panic!("{label}/{strategy}: {err:?}"));
            let mut executor = definition
                .create_executor(
                    linker.clone(),
                    HostCalls::default(),
                    handler,
                    Some(INITIAL_FUEL),
                    None,
                )
                .unwrap_or_else(|trap| panic!("{label}/{strategy}: instantiate: {trap:?}"));
            let mut result = [Value::I64(-1)];
            assert_eq!(
                executor.execute("main", params, &mut result),
                Ok(()),
                "{label}/{strategy}"
            );
            assert_eq!(result, [Value::I64(0)], "{label}/{strategy}");
            assert_eq!(
                executor.data().count,
                1,
                "{label}/{strategy}: host call count"
            );
            assert_eq!(
                executor.data().params,
                host_params,
                "{label}/{strategy}: host arguments"
            );
            assert_eq!(
                executor.remaining_fuel(),
                Some(INITIAL_FUEL - expected_fuel),
                "{label}/{strategy}: fuel"
            );
        }
    }

    /// Use parameters rather than float constants so these fixtures also work with FPU disabled.
    fn parameter_wasm(param_text: &str) -> Vec<u8> {
        wat::parse_str(format!(
            r#"(module
              (import "env" "imp" (func $imp (param {param_text})))
              (func (export "main") (param {param_text}) (result i64)
                local.get 0 local.get 1 call $imp i64.const 0))"#
        ))
        .unwrap()
    }

    /// Parameter signatures and the position of their non-i32 parameter, counted from the end.
    const PARAMETER_CASES: [(&str, &[ValType], u32); 6] = [
        ("i32 i64", &[ValType::I32, ValType::I64], 1),
        ("i64 i32", &[ValType::I64, ValType::I32], 2),
        ("i32 f64", &[ValType::I32, ValType::F64], 1),
        ("f64 i32", &[ValType::F64, ValType::I32], 2),
        ("i32 f32", &[ValType::I32, ValType::F32], 1),
        ("f32 i32", &[ValType::F32, ValType::I32], 2),
    ];

    /// HIGH-5: a non-i32 metered parameter is a configuration error on both strategies. The old rwasm
    /// trampoline accepted wide values and read one 32-bit word, while Wasmtime rejected them.
    #[test]
    fn non_i32_metered_syscall_parameters_are_rejected() {
        for (param_text, params, index) in PARAMETER_CASES {
            let wasm = parameter_wasm(param_text);
            for (policy_name, policy, _) in policies(index) {
                let linker = linker(policy, params);
                for (strategy, definition) in definitions(&linker, &wasm) {
                    let err = definition.err().unwrap_or_else(|| {
                        panic!("{param_text}/{policy_name}/{strategy}: accepted a non-i32 metered parameter")
                    });
                    assert!(
                        matches!(err, CompilationError::InvalidSyscallFuelParam),
                        "{param_text}/{policy_name}/{strategy}: unexpected rejection: {err:?}"
                    );
                }
            }
        }
    }

    /// Rejecting a non-i32 metered length must not reject a different, unmetered wide parameter or
    /// change which parameter is charged when that wide value occupies two rwasm stack slots.
    #[test]
    fn i32_metered_parameters_with_non_i32_neighbors_run_and_charge_correctly() {
        for (param_text, params, non_i32_index) in PARAMETER_CASES {
            let value = match params[2 - non_i32_index as usize] {
                // Either 32-bit half would charge for two words, unlike the one-word i32 length.
                ValType::I64 => Value::I64(0x40_0000_0040),
                ValType::F64 => Value::F64(4.0.into()),
                ValType::F32 => Value::F32(4.0.into()),
                _ => unreachable!(),
            };
            let args = if non_i32_index == 1 {
                [Value::I32(9), value]
            } else {
                [value, Value::I32(9)]
            };
            let wasm = parameter_wasm(param_text);
            for (policy_name, policy, charge) in policies(3 - non_i32_index) {
                // 1 entry + 2 local.get + 10 call + 1 i64.const, plus the syscall policy.
                assert_runs(
                    &linker(policy, params),
                    &wasm,
                    &args,
                    &args,
                    14 + charge,
                    &format!("{param_text}/{policy_name}"),
                );
            }
        }
    }

    fn stack_wasm(locals: usize) -> Vec<u8> {
        wat::parse_str(format!(
            r#"(module
              (import "env" "imp" (func $imp (param i32)))
              (func (export "main") (result i64) {locals}
                i32.const 9 call $imp i64.const 0))"#,
            locals = "(local i32)".repeat(locals)
        ))
        .unwrap()
    }

    /// HIGH-6: the i64 result makes the Wasm frame peak `locals + 2`. A linear trampoline needs two
    /// additional slots during the call; a quadratic one needs four. Every formerly failing frame
    /// up to the compiler's limit must run, return the right value and charge the expected fuel.
    #[test]
    fn stack_window_boundary_for_metered_imports_runs_and_charges_correctly() {
        for locals in N_MAX_STACK_SIZE - 4..=N_MAX_STACK_SIZE - 2 {
            let wasm = stack_wasm(locals);
            for (policy_name, policy, charge) in policies(1) {
                // 1 entry + 1 i32.const + 10 call + 1 i64.const, plus the syscall policy.
                assert_runs(
                    &linker(policy, &[ValType::I32]),
                    &wasm,
                    &[],
                    &[Value::I32(9)],
                    13 + charge,
                    &format!("{policy_name}/{locals} locals"),
                );
            }
        }
    }

    /// Control: this frame fit even before the runtime reserved trampoline headroom.
    #[test]
    fn stack_window_below_the_boundary_runs_and_charges_correctly() {
        let wasm = stack_wasm(N_MAX_STACK_SIZE - 5);
        for (policy_name, policy, charge) in policies(1) {
            assert_runs(
                &linker(policy, &[ValType::I32]),
                &wasm,
                &[],
                &[Value::I32(9)],
                13 + charge,
                policy_name,
            );
        }
    }

    /// Trampoline headroom must not enlarge the accepted Wasm frame: one slot above the limit is
    /// still rejected by both strategies, including the height and limit reported by the compiler.
    #[test]
    fn stack_window_above_the_boundary_is_rejected() {
        let wasm = stack_wasm(N_MAX_STACK_SIZE - 1);
        for (policy_name, policy, _) in policies(1) {
            for (strategy, definition) in definitions(&linker(policy, &[ValType::I32]), &wasm) {
                let err = definition.err().unwrap_or_else(|| {
                    panic!("{policy_name}/{strategy}: oversized frame accepted")
                });
                assert!(
                    matches!(err, CompilationError::StackHeightExceeded { height, limit }
                    if height == N_MAX_STACK_SIZE as u32 + 1 && limit == N_MAX_STACK_SIZE as u32),
                    "{policy_name}/{strategy}: unexpected rejection: {err:?}"
                );
            }
        }
    }
}

#[cfg(feature = "wasmtime")]
mod static_out_of_bounds_fuel {
    //! HIGH-7.
    //!
    //! `HIGH-7`: after a memory access that Cranelift can prove out of bounds at compile time — the
    //! access's immediate `offset` plus its size exceeds the memory's declared maximum, or the memory
    //! declares a maximum of zero pages — the Wasmtime strategy charges the region only up to and
    //! including that access, while rwasm charges the whole region on entry (its documented model,
    //! which Wasmtime otherwise follows: a *dynamic* out-of-bounds access, a division trap or an
    //! `unreachable` leave the same counter on both). Both strategies trap `MemoryOutOfBounds`, but
    //! they disagree on the remaining fuel by the cost of everything after the access in the region.
    //! Written against the correct behaviour, so the tests fail until fixed.

    use rwasm::{
        for_each_strategy, CompilationConfig, ImportLinker, StoreTr, StrategyError, TrapCode, Value,
    };
    use std::sync::Arc;

    /// Runs `main` on both strategies with a 1000 fuel budget: `(trap, remaining fuel)` per strategy.
    fn both(wat: &str) -> Vec<(Option<TrapCode>, Option<u64>)> {
        let wasm = wat::parse_str(wat).expect("the test module parses");
        for_each_strategy(
            |strategy| -> Result<_, StrategyError> {
                let mut executor = strategy.create_executor(
                    Arc::new(ImportLinker::default()),
                    (),
                    rwasm::always_failing_syscall_handler,
                    Some(1_000),
                    None,
                )?;
                let trap = executor.execute("main", &[], &mut []).err();
                Ok((trap, executor.remaining_fuel()))
            },
            CompilationConfig::default_strategy_compatible()
                .with_entrypoint_name("main".into())
                .with_allow_malformed_entrypoint_func_type(true),
            &wasm,
        )
        .expect("both strategies compile the module")
    }

    fn assert_aligned(label: &str, wat: &str) {
        let outcomes = both(wat);
        assert_eq!(
            outcomes[0].0,
            Some(TrapCode::MemoryOutOfBounds),
            "{label}: rwasm traps"
        );
        assert_eq!(
            outcomes[0], outcomes[1],
            "{label}: (trap, remaining fuel) must agree: rwasm={:?} wasmtime={:?}",
            outcomes[0], outcomes[1]
        );
    }

    /// Control: a *dynamically* out-of-bounds load (offset within the maximum, address past the
    /// current size) charges the whole region on both strategies.
    #[test]
    fn dynamic_out_of_bounds_load_charges_the_whole_region_on_both() {
        assert_aligned(
            "dynamic",
            r#"(module
              (memory (export "memory") 1 2)
              (func (export "main")
                (drop (i32.load offset=131068 (i32.const 0)))
                (drop (i32.const 1)) (drop (i32.const 1))
                unreachable))"#,
        );
    }

    /// A load whose immediate offset lies beyond the declared maximum: statically out of bounds.
    #[test]
    fn statically_out_of_bounds_load_charges_the_whole_region_on_both() {
        assert_aligned(
            "static offset",
            r#"(module
              (memory (export "memory") 1 2)
              (func (export "main")
                (drop (i32.load offset=131072 (i32.const 0)))
                (drop (i32.const 1)) (drop (i32.const 1))
                unreachable))"#,
        );
    }

    /// The degenerate form: a memory that can never hold a page makes every access static.
    #[test]
    fn access_to_a_zero_maximum_memory_charges_the_whole_region_on_both() {
        assert_aligned(
            "max 0",
            r#"(module
              (memory (export "memory") 0 0)
              (func (export "main")
                (drop (i32.load (i32.const 0)))
                (drop (i32.const 1)) (drop (i32.const 1))
                unreachable))"#,
        );
    }

    /// Stores behave the same as loads.
    #[test]
    fn statically_out_of_bounds_store_charges_the_whole_region_on_both() {
        assert_aligned(
            "static store",
            r#"(module
              (memory (export "memory") 0 1)
              (func (export "main")
                (i32.store offset=65536 (i32.const 0) (i32.const 0))
                (drop (i32.const 1)) (drop (i32.const 1))
                unreachable))"#,
        );
    }

    /// The gap scales with the region: everything after the access is uncharged on Wasmtime.
    #[test]
    fn undercharge_grows_with_the_region() {
        let mut tail = String::new();
        for _ in 0..500 {
            tail.push_str("(drop (i32.const 1)) ");
        }
        let wat = format!(
            r#"(module
              (memory (export "memory") 1 1)
              (func (export "main")
                (drop (i32.load offset=65536 (i32.const 0)))
                {tail}
                unreachable))"#
        );
        let outcomes = both(&wat);
        assert_eq!(outcomes[0].0, Some(TrapCode::MemoryOutOfBounds));
        let _ = Value::I32(0);
        assert_eq!(
            outcomes[0], outcomes[1],
            "500 charged operators after a static trap: rwasm={:?} wasmtime={:?}",
            outcomes[0], outcomes[1]
        );
    }
}

#[cfg(feature = "wasmtime")]
mod bulk_operation_metering {
    //! HIGH-8: bulk memory and table operations are priced flat on the Wasmtime strategy, and the
    //! only configuration both strategies accept therefore prices them flat on rwasm too — 64 MiB
    //! of `memory.fill` for 14 fuel, ~24 000× the per-fuel cost of ordinary instructions. The fix
    //! is a dynamic charge in the Wasmtime fork; these tests describe the fixed contract and stay
    //! ignored until the fork ships (`cargo test -- --ignored` runs them, and the first one prints
    //! the measurement either way).

    use rwasm::{
        CompilationConfig, CompilationError, ImportLinker, StoreTr, StrategyDefinition, Value,
    };
    use std::{sync::Arc, time::Instant};

    const PAGES: u32 = 1024;
    const FILLS: i32 = 200;
    const FILL_BYTES: u32 = 64 * 1024 * 1024;
    const FUEL: u64 = 1_000_000_000;

    fn wasm() -> Vec<u8> {
        wat::parse_str(format!(
            r#"(module (memory (export "memory") {PAGES})
              (func (export "main") (param $n i32)
                (block (loop
                  (br_if 1 (i32.eqz (local.get $n)))
                  (memory.fill (i32.const 0) (i32.const 8) (i32.const {FILL_BYTES}))
                  (local.set $n (i32.sub (local.get $n) (i32.const 1)))
                  (br 0)))))"#
        ))
        .unwrap()
    }

    /// `(fuel consumed, wall time)` of `FILLS` fills on `definition`.
    fn measure(definition: StrategyDefinition) -> (u64, std::time::Duration) {
        let mut executor = definition
            .create_executor(
                Arc::new(ImportLinker::default()),
                (),
                rwasm::always_failing_syscall_handler,
                Some(FUEL),
                Some(PAGES),
            )
            .unwrap();
        let started = Instant::now();
        executor
            .execute("main", &[Value::I32(FILLS)], &mut [])
            .unwrap();
        (FUEL - executor.remaining_fuel().unwrap(), started.elapsed())
    }

    /// With `consume_fuel_for_bulk_ops` both strategies must charge `(n + 63) >> 6` per fill —
    /// 1 Mi fuel for 64 MiB — and agree. Today the Wasmtime strategy rejects the config outright,
    /// and the config it does accept charges 14 fuel per fill on both engines.
    #[test]
    #[ignore = "HIGH-8: needs wasmtime-rwasm with a dynamic bulk-operation charge"]
    fn bulk_operations_are_metered_by_size_on_both_strategies() {
        let wasm = wasm();
        let metered = CompilationConfig::default()
            .with_consume_fuel_for_params_and_locals(false)
            .with_entrypoint_name("main".into())
            .with_max_allowed_memory_pages(PAGES);
        let per_fill = u64::from(FILL_BYTES.div_ceil(64));
        let (rwasm_fuel, rwasm_time) =
            measure(StrategyDefinition::new_as_rwasm(metered.clone(), &wasm).unwrap());
        eprintln!("rwasm metered: {rwasm_fuel} fuel in {rwasm_time:?}");
        assert!(rwasm_fuel >= per_fill * FILLS as u64, "{rwasm_fuel}");
        let wasmtime = StrategyDefinition::new_as_wasmtime(metered, &wasm, None)
            .expect("the Wasmtime strategy accepts the size-metered config");
        let (wasmtime_fuel, wasmtime_time) = measure(wasmtime);
        eprintln!("wasmtime metered: {wasmtime_fuel} fuel in {wasmtime_time:?}");
        assert_eq!(rwasm_fuel, wasmtime_fuel);
    }

    /// The strategy-compatible config must not be the one that prices 64 MiB at 14 fuel: once
    /// the fork meters bulk operations, `default_strategy_compatible()` keeps the dynamic charge.
    /// Until then this pins the measurement that motivates the finding.
    #[test]
    #[ignore = "HIGH-8: needs wasmtime-rwasm with a dynamic bulk-operation charge"]
    fn flat_priced_bulk_operations_are_not_offered_as_strategy_compatible() {
        let wasm = wasm();
        let compatible = CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_max_allowed_memory_pages(PAGES);
        for definition in [
            StrategyDefinition::new_as_rwasm(compatible.clone(), &wasm).unwrap(),
            StrategyDefinition::new_as_wasmtime(compatible.clone(), &wasm, None).unwrap(),
        ] {
            let (fuel, time) = measure(definition);
            let per_fill = fuel / FILLS as u64;
            eprintln!(
                "strategy-compatible: {per_fill} fuel per 64 MiB fill, {:.1} µs per fuel unit",
                time.as_micros() as f64 / fuel as f64
            );
            assert!(
                per_fill >= u64::from(FILL_BYTES.div_ceil(64)),
                "a 64 MiB fill must not cost {per_fill} fuel"
            );
        }
        // and the size-metered config must be strategy compatible
        assert!(
            !matches!(
                StrategyDefinition::new(
                    CompilationConfig::default()
                        .with_consume_fuel_for_params_and_locals(false)
                        .with_entrypoint_name("main".into()),
                    &wasm,
                    None
                ),
                Err(CompilationError::StrategyIncompatibleConfig)
            ),
            "the size-metered config is rejected as strategy incompatible"
        );
    }
}
