//! Value-stack window and native-stack limits: a frame the compiler accepts must run on the
//! rwasm VM and on the Wasmtime backend alike.

use rwasm::{CompilationConfig, ExecutionEngine, ImportLinker, RwasmModule, RwasmStore, Value};

#[test]
fn test_stack_overflow_number_of_params() -> anyhow::Result<()> {
    let wat = r#"
(module
  (type (;0;) (func (param i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i32)))
  (func (;0;) (export "main") (type 0) (param i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i32)
    global.get 0
    i32.eqz
    if  ;; label = @1
      unreachable
    end
    global.get 0
    i32.const 1
    i32.sub
    global.set 0)
  (global (;0;) (mut i32) (i32.const 1000))
  (export "" (func 0)))
    "#;
    let wasm_binary = wat::parse_str(wat)?;
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary)?;
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::default();
    let instance =
        ImportLinker::default().instantiate(&mut store, ExecutionEngine::new(), rwasm_module)?;
    let mut params = vec![Value::I64(0); 18];
    params.push(Value::I32(0));
    let mut result = [];
    instance.execute(&mut store, &params, &mut result)?;
    Ok(())
}

#[test]
fn test_stack_overflow_32_params() -> anyhow::Result<()> {
    let wat = r#"
(module
  (type (;0;) (func (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)))
  (func (;0;) (export "main") (type 0) (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)
    global.get 0
    i32.eqz
    if  ;; label = @1
      unreachable
    end
    global.get 0
    i32.const 1
    i32.sub
    global.set 0)
  (global (;0;) (mut i32) (i32.const 1000))
  (export "" (func 0)))
    "#;
    let wasm_binary = wat::parse_str(wat)?;
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary)?;
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::default();
    let instance =
        ImportLinker::default().instantiate(&mut store, ExecutionEngine::new(), rwasm_module)?;
    let params = vec![Value::I32(0); 32];
    let mut result = [];
    instance.execute(&mut store, &params, &mut result)?;
    Ok(())
}

#[test]
fn test_stack_overflow_33_params() -> anyhow::Result<()> {
    let wat = r#"
(module
  (type (;0;) (func (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)))
  (func (;0;) (export "main") (type 0) (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)
    global.get 0
    i32.eqz
    if  ;; label = @1
      unreachable
    end
    global.get 0
    i32.const 1
    i32.sub
    global.set 0)
  (global (;0;) (mut i32) (i32.const 1000))
  (export "" (func 0)))
    "#;
    let wasm_binary = wat::parse_str(wat)?;
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary)?;
    println!("{}", rwasm_module);
    let mut store = RwasmStore::<()>::default();
    let instance =
        ImportLinker::default().instantiate(&mut store, ExecutionEngine::new(), rwasm_module)?;
    let params = vec![Value::I32(0); 33];
    let mut result = [];
    instance.execute(&mut store, &params, &mut result)?;
    Ok(())
}

#[cfg(feature = "wasmtime")]
mod strategy {
    use rwasm::{
        always_failing_syscall_handler, for_each_strategy, CompilationConfig, ImportLinker,
        StrategyDefinition, StrategyError, TrapCode, Value,
    };
    use std::sync::Arc;

    /// Runs `main` on both strategies: `(strategy, outcome)` in the order rwasm, Wasmtime.
    pub fn both(wasm: &[u8], result: Value) -> Vec<(&'static str, Result<Value, TrapCode>)> {
        let config =
            CompilationConfig::default_strategy_compatible().with_entrypoint_name("main".into());
        let mut out = Vec::new();
        for_each_strategy(
            |definition| {
                let name = match &definition {
                    StrategyDefinition::Rwasm { .. } => "rwasm",
                    _ => "wasmtime",
                };
                let mut executor = definition.create_executor(
                    Arc::new(ImportLinker::default()),
                    (),
                    always_failing_syscall_handler,
                    Some(100_000_000),
                    None,
                )?;
                let mut result = [result.clone()];
                let outcome = executor
                    .execute("main", &[], &mut result)
                    .map(|_| result[0].clone());
                out.push((name, outcome));
                Ok::<(), StrategyError>(())
            },
            config,
            wasm,
        )
        .unwrap();
        out
    }
}

#[cfg(feature = "wasmtime")]
mod snippet_frame_headroom {
    //! HIGH: an `i64` operator lowered to a code snippet runs in a hidden frame of its own
    //! (`StackCheck(MSH_*)` on top of the caller's operands), which the compile-time frame check
    //! (`params + peak <= N_MAX_STACK_SIZE`) does not see and which `N_STACK_TRAMPOLINE_HEADROOM`
    //! (then 4) did not cover for `i64.mul` (5), `i64.div_u`/`i64.rem_u` (8) and
    //! `i64.div_s`/`i64.rem_s` (13). A module both compilers accept trapped `StackOverflow` on the
    //! rwasm VM while the Wasmtime backend, where the operator is a native instruction, returned
    //! the result. Same class as HIGH-3/HIGH-6 of the 2026-09-13 report (the import trampoline's
    //! temporaries); the headroom now covers the deepest snippet frame.

    use super::strategy::both;
    use rwasm::{Value, N_MAX_STACK_SIZE};

    /// `main` computes `91 op 7` with `locals` i32 locals below the two i64 operands.
    fn module(op: &str, locals: usize) -> Vec<u8> {
        wat::parse_str(format!(
            r#"(module
                (memory (export "memory") 1)
                (func (export "main") (result i64) {locals}
                  i64.const 91 i64.const 7 {op}))"#,
            locals = "(local i32)".repeat(locals)
        ))
        .unwrap()
    }

    #[test]
    fn snippet_calls_at_the_frame_ceiling_run_on_both_strategies() {
        // (operator, the snippet's `StackCheck`)
        let ops = [
            ("i64.add", 4),
            ("i64.mul", 5),
            ("i64.div_u", 8),
            ("i64.rem_u", 8),
            ("i64.div_s", 13),
            ("i64.rem_s", 13),
        ];
        let mut divergences = Vec::new();
        for (op, msh) in ops {
            // the frame is `locals + 4` (two i64 operands); the compiler accepts up to the limit
            let max_locals = N_MAX_STACK_SIZE - 4;
            for locals in (max_locals - msh)..=max_locals {
                let outcomes = both(&module(op, locals), Value::I64(0));
                if outcomes[0].1 != outcomes[1].1 {
                    divergences.push(format!("{op} frame={}: {outcomes:?}", locals + 4));
                }
            }
        }
        assert!(
            divergences.is_empty(),
            "a frame the compiler accepts must run on both strategies:\n{}",
            divergences.join("\n")
        );
    }
}

#[cfg(feature = "wasmtime")]
mod wasmtime_native_frame {
    //! HIGH: the Wasmtime engine was configured with `max_wasm_stack(N_MAX_STACK_SIZE * 4)`,
    //! i.e. one native byte per rwasm slot byte, but Cranelift spills every live value into an
    //! 8-byte slot. A single non-recursive function whose live operands filled more than about
    //! half of the rwasm window (accepted by the compiler and executed by the rwasm VM) trapped
    //! `StackOverflow` on the Wasmtime backend at function entry. The documented backend
    //! difference covered recursion depth, not one accepted frame; the native stack is now sized
    //! from what Cranelift needs per accepted slot and frame (`WASMTIME_MAX_WASM_STACK`).

    use super::strategy::both;
    use rwasm::Value;

    /// `n` i32 locals loaded from memory (so no constant folding), all live until the final sum.
    fn module(n: usize) -> Vec<u8> {
        let mut body = String::new();
        for i in 0..n {
            body.push_str(&format!(
                "(local.set {i} (i32.load (i32.const {})))\n",
                (i * 4) % 65536
            ));
        }
        body.push_str("(i32.const 0)\n");
        for i in 0..n {
            body.push_str(&format!("(local.get {i}) (i32.add)\n"));
        }
        wat::parse_str(format!(
            r#"(module
                (memory (export "memory") 1)
                (data (i32.const 0) "\01\00\00\00")
                (func (export "main") (result i32) (local {locals})
                  {body}))"#,
            locals = "i32 ".repeat(n)
        ))
        .unwrap()
    }

    /// `f(depth)` keeps `live` i32 locals (loaded, so not folded) alive across its recursive
    /// call; returns `depth + 1` when `live > 0`, else 0.
    fn call_chain(live: usize, depth: u32) -> Vec<u8> {
        let mut sets = String::new();
        let mut uses = String::new();
        for i in 0..live {
            sets.push_str(&format!(
                "(local.set {} (i32.load (i32.const {})))\n",
                i + 1,
                (i * 4) % 65536
            ));
            uses.push_str(&format!("(local.get {}) (i32.add)\n", i + 1));
        }
        wat::parse_str(format!(
            r#"(module
                (memory (export "memory") 1)
                (data (i32.const 0) "\01\00\00\00")
                (func $f (param i32) (result i32) (local {locals})
                  {sets}
                  (if (result i32) (i32.eqz (local.get 0))
                    (then (i32.const 0))
                    (else (call $f (i32.sub (local.get 0) (i32.const 1)))))
                  {uses})
                (func (export "main") (result i32) (call $f (i32.const {depth}))))"#,
            locals = "i32 ".repeat(live)
        ))
        .unwrap()
    }

    /// The deepest call chains the rwasm VM accepts must run on the Wasmtime backend as well;
    /// the reverse direction (a chain rwasm rejects) is the documented remaining difference.
    #[test]
    fn an_accepted_call_chain_runs_on_both_strategies() {
        let mut divergences = Vec::new();
        for (live, depth) in [(0usize, 1023u32), (3, 1023), (7, 700), (15, 400), (60, 120)] {
            let outcomes = both(&call_chain(live, depth), Value::I32(0));
            let expected = Ok(Value::I32(if live > 0 { depth as i32 + 1 } else { 0 }));
            if outcomes[0].1 != expected || outcomes[1].1 != expected {
                divergences.push(format!("{live} live locals x {depth} frames: {outcomes:?}"));
            }
        }
        assert!(
            divergences.is_empty(),
            "a call chain the rwasm VM runs must run on both strategies:\n{}",
            divergences.join("\n")
        );
    }

    #[test]
    fn one_accepted_frame_runs_on_both_strategies() {
        let mut divergences = Vec::new();
        for n in [4000usize, 5000, 6000, 8000] {
            let outcomes = both(&module(n), Value::I32(0));
            if outcomes[0].1 != outcomes[1].1 {
                divergences.push(format!("{n} live i32 locals: {outcomes:?}"));
            }
        }
        assert!(
            divergences.is_empty(),
            "a frame the compiler accepts must run on both strategies:\n{}",
            divergences.join("\n")
        );
    }
}

#[cfg(feature = "wasmtime")]
mod value_stack_window {
    //! Parameters, locals and operands share one `N_MAX_STACK_SIZE` window on both strategies
    //! (audit 2026-09-13, round 2, R2-2), and a syscall at the limit reuses its parameter slots.

    use rwasm::{
        CompilationConfig, CompilationError, ImportLinker, ImportName, StrategyDefinition,
        StrategyExecutor, SyscallFuelParams, SyscallHandler, TrapCode, TypedCaller, Value,
    };
    use std::sync::Arc;
    use wasmparser::ValType;

    fn wat_type(ty: &ValType) -> &'static str {
        match ty {
            ValType::I32 => "i32",
            ValType::I64 => "i64",
            ValType::F32 => "f32",
            ValType::F64 => "f64",
            _ => panic!("non-numeric test type"),
        }
    }

    fn config(linker: &Arc<ImportLinker>) -> CompilationConfig {
        CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_import_linker(linker.clone())
    }

    /// Compiles `wasm` for both strategies and instantiates each with `linker`/`handler`.
    fn both_executors(
        wasm: &[u8],
        linker: Arc<ImportLinker>,
        handler: SyscallHandler<()>,
    ) -> (StrategyExecutor<()>, StrategyExecutor<()>) {
        let instantiate = |definition: StrategyDefinition| {
            definition
                .create_executor(linker.clone(), (), handler, Some(1_000_000), None)
                .expect("the module must instantiate")
        };
        let rwasm = instantiate(
            StrategyDefinition::new_as_rwasm(config(&linker), wasm)
                .expect("rwasm compiles the module"),
        );
        let wasmtime = instantiate(
            StrategyDefinition::new_as_wasmtime(config(&linker), wasm, None)
                .expect("wasmtime compiles the module"),
        );
        (rwasm, wasmtime)
    }

    /// R2-2: parameters, locals and operands share the same 8192-slot window.
    #[test]
    fn parameter_slots_count_towards_the_value_stack_window() {
        const PARAMS: usize = 100;
        for (ty, width) in [(ValType::I32, 1), (ValType::I64, 2), (ValType::F64, 2)] {
            for height in [8191, 8192, 8193] {
                let locals = height - PARAMS * width - 1;
                let wasm = wat::parse_str(format!(
                    r#"(module (func (export "main") {} (result i32) {} (i32.const 42)))"#,
                    format!("(param {})", wat_type(&ty)).repeat(PARAMS),
                    "(local i32)".repeat(locals)
                ))
                .unwrap();
                let linker = Arc::new(ImportLinker::default());
                let definitions = [
                    StrategyDefinition::new_as_rwasm(config(&linker), &wasm),
                    StrategyDefinition::new_as_wasmtime(config(&linker), &wasm, None),
                ];
                for definition in definitions {
                    if height > 8192 {
                        assert!(matches!(
                            definition,
                            Err(CompilationError::StackHeightExceeded {
                                height: 8193,
                                limit: 8192
                            })
                        ));
                    } else {
                        let mut executor = definition.unwrap().default_executor().unwrap();
                        let mut result = [Value::I32(-1)];
                        executor
                            .execute("main", &vec![Value::default(ty); PARAMS], &mut result)
                            .unwrap();
                        assert_eq!(result, [Value::I32(42)]);
                    }
                }
            }
        }
    }

    /// A syscall replaces its arguments even when the operand stack fills the whole window.
    #[test]
    fn syscall_at_the_stack_limit_reuses_parameter_slots() {
        let wasm = wat::parse_str(format!(
            r#"(module
            (import "env" "s" (func $s (param i64 i32) (result i64)))
            (func (export "main") (result i64) {}
                i64.const 0x123456789abcdef0 i32.const 7 call $s))"#,
            "(local i32)".repeat(8189)
        ))
        .unwrap();
        let mut linker = ImportLinker::default();
        linker.insert_function(
            ImportName::new("env", "s"),
            1,
            SyscallFuelParams::default(),
            &[ValType::I64, ValType::I32],
            &[ValType::I64],
        );
        fn handler(
            _: &mut TypedCaller<'_, ()>,
            _: u32,
            params: &[Value],
            result: &mut [Value],
        ) -> Result<(), TrapCode> {
            assert_eq!(params, [Value::I64(0x123456789abcdef0), Value::I32(7)]);
            result[0] = params[0].clone();
            Ok(())
        }
        let (rwasm, wasmtime) = both_executors(&wasm, Arc::new(linker), handler);
        for mut executor in [rwasm, wasmtime] {
            let mut result = [Value::I64(0)];
            executor.execute("main", &[], &mut result).unwrap();
            assert_eq!(result, [Value::I64(0x123456789abcdef0)]);
        }
    }
}
