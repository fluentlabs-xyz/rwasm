//! Reproductions for the 2026-09-18 audit round. Every test is written against the correct
//! behaviour and was red at the audited revision (`84cb1740e`, v0.6.0); all of them are green
//! with the fixes on this branch, so the file doubles as the regression suite for that round.

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
