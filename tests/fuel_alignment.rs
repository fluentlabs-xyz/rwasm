//! Differential fuel tests: the same wasm module must burn the same fuel, stop at the same point
//! and report the same remaining fuel on the rwasm VM and on Wasmtime.
//!
//! Every test compiles one module with [`CompilationConfig::default_strategy_compatible`] and
//! runs it through [`for_each_strategy`], which yields the rwasm outcome first and the Wasmtime
//! outcome second. The tests pin the invariants both engines implement:
//!
//! - Metering is eager and region based. A straight-line region (function entry, loop header,
//!   `if` arm, `else` arm, the code after any `end`, the fall-through after `br_if`) is charged
//!   in full before its first instruction runs, so a trap inside a region, or in a callee, leaves
//!   the same counter on both engines.
//! - A charge that does not fit in the remaining fuel is never applied: execution traps with
//!   `OutOfFuel` and the remaining fuel stays what it was before the region.
//! - `fuel_limit: None` is unbounded and reports no remaining fuel.
//! - Syscall fuel parameters are addressed by parameter position counted from the last
//!   parameter, regardless of how many stack slots the parameters above it occupy.
//! - Disabled float operators trap after charging the code up to and including the operator.
//!
//! The absolute numbers asserted below double as a record of the shared schedule.

use rwasm::{
    for_each_strategy, CompilationConfig, ImportLinker, ImportName, StoreTr, SyscallHandler,
    TrapCode, TypedCaller, Value,
};
use rwasm_fuel_policy::{LinearFuelParams, QuadraticFuelParams, SyscallFuelParams};
use std::sync::Arc;
use wasmparser::ValType;

/// Everything observable about one execution that the two engines must agree on.
#[derive(Debug, Clone, PartialEq)]
struct Outcome {
    trap: Option<TrapCode>,
    remaining_fuel: Option<u64>,
    result: Vec<Value>,
    /// First `memory_prefix` bytes of the exported memory, empty when not requested.
    memory: Vec<u8>,
}

struct Run<'a> {
    wat: &'a str,
    import_linker: Arc<ImportLinker>,
    syscall_handler: SyscallHandler<()>,
    fuel_limit: Option<u64>,
    params: &'a [Value],
    /// Typed template for the result buffer; rwasm sizes the returned stack slots from it.
    results: &'a [Value],
    /// How many leading bytes of the exported `memory` to capture (0 = module has no memory).
    memory_prefix: usize,
}

impl<'a> Run<'a> {
    fn plain(wat: &'a str, fuel_limit: Option<u64>) -> Self {
        Self {
            wat,
            import_linker: Arc::new(ImportLinker::default()),
            syscall_handler: rwasm::always_failing_syscall_handler,
            fuel_limit,
            params: &[],
            results: &[],
            memory_prefix: 0,
        }
    }

    fn config(&self) -> CompilationConfig {
        CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_import_linker(self.import_linker.clone())
            .with_builtins_consume_fuel(true)
    }

    /// Returns `(rwasm, wasmtime)` outcomes.
    fn execute(&self) -> (Outcome, Outcome) {
        let wasm_binary = wat::parse_str(self.wat).unwrap();
        let outcomes = for_each_strategy(
            |strategy| {
                let mut executor = strategy.create_executor(
                    self.import_linker.clone(),
                    (),
                    self.syscall_handler,
                    self.fuel_limit,
                    None,
                )?;
                let mut result = self.results.to_vec();
                let trap = executor.execute("main", self.params, &mut result).err();
                let memory = if self.memory_prefix > 0 {
                    executor.snapshot_memory()?[..self.memory_prefix].to_vec()
                } else {
                    Vec::new()
                };
                Ok(Outcome {
                    trap,
                    remaining_fuel: executor.remaining_fuel(),
                    result,
                    memory,
                })
            },
            self.config(),
            &wasm_binary,
        )
        .unwrap();
        assert_eq!(
            outcomes.len(),
            2,
            "expected exactly the rwasm and wasmtime strategies"
        );
        let mut outcomes = outcomes.into_iter();
        (outcomes.next().unwrap(), outcomes.next().unwrap())
    }
}

fn assert_aligned(rwasm: &Outcome, wasmtime: &Outcome) {
    assert_eq!(
        rwasm, wasmtime,
        "\nrwasm and wasmtime diverged:\n  rwasm    = {rwasm:?}\n  wasmtime = {wasmtime:?}\n"
    );
}

fn accepting_syscall_handler(
    _caller: &mut TypedCaller<'_, ()>,
    _sys_func_idx: u32,
    _params: &[Value],
    _result: &mut [Value],
) -> Result<(), TrapCode> {
    Ok(())
}

/// Control flow, calls, memory, globals, tables, `select`, the `i64` snippets and every syscall
/// charging mode, on a path that runs to completion.
#[test]
fn fuel_matches_on_completing_path() {
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("env", "const_call"),
        1,
        SyscallFuelParams::Const(17),
        &[ValType::I32],
        &[],
    );
    import_linker.insert_function(
        ImportName::new("env", "linear_call"),
        2,
        SyscallFuelParams::LinearFuel(LinearFuelParams {
            base_fuel: 7,
            param_index: 1,
            word_cost: 5,
        }),
        &[ValType::I32],
        &[],
    );
    import_linker.insert_function(
        ImportName::new("env", "quadratic_call"),
        3,
        SyscallFuelParams::QuadraticFuel(QuadraticFuelParams {
            local_depth: 1,
            word_cost: 3,
            divisor: 2,
            fuel_denom_rate: 4,
        }),
        &[ValType::I32],
        &[],
    );
    let run = Run {
        wat: r#"
        (module
          (import "env" "const_call" (func $c (param i32)))
          (import "env" "linear_call" (func $l (param i32)))
          (import "env" "quadratic_call" (func $q (param i32)))
          (type $t (func (param i32) (result i32)))
          (memory (export "memory") 1)
          (global $g (mut i32) (i32.const 0))
          (table 2 funcref)
          (elem (i32.const 0) $f $f)
          (func $f (type $t) local.get 0 i32.const 1 i32.add)
          (func (export "main") (param i32) (result i64)
            (local i64)
            ;; if / else
            local.get 0
            if (result i32) i32.const 7 else i32.const 9 end
            drop
            ;; loop with a conditional back-edge
            (loop $l
              global.get $g i32.const 1 i32.add global.set $g
              global.get $g i32.const 5 i32.lt_u
              br_if $l)
            ;; br_table
            (block $a (block $b
              local.get 0 br_table $a $b)
              i32.const 1 drop)
            ;; direct and indirect calls
            i32.const 1 call $f drop
            i32.const 2 i32.const 0 call_indirect (type $t) drop
            ;; memory
            i32.const 0 i32.const 42 i32.store
            i32.const 0 i32.load drop
            memory.size drop
            ;; select
            i32.const 1 i32.const 2 local.get 0 select drop
            ;; i64 operators lowered to snippets on rwasm
            i64.const 10 i64.const 3 i64.mul i64.const 2 i64.div_s i64.const 1 i64.lt_s drop
            ;; syscalls in every charging mode
            i32.const 64 call $c
            i32.const 64 call $l
            i32.const 64 call $q
            i64.const 5
          )
        )
        "#,
        import_linker: Arc::new(import_linker),
        syscall_handler: accepting_syscall_handler,
        fuel_limit: Some(10_000),
        params: &[Value::I32(1)],
        results: &[Value::I64(0)],
        memory_prefix: 8,
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, None);
    assert_eq!(rwasm.result, vec![Value::I64(5)]);
    assert_aligned(&rwasm, &wasmtime);
}

/// The caller's region is charged in full on entry, including the instructions after `call`
/// that never run because the callee traps: 1 entry + 10 call + 6 tail in `main`, 1 entry in
/// the callee.
#[test]
fn fuel_matches_when_callee_traps_and_caller_has_a_tail() {
    let run = Run::plain(
        r#"
        (module
          (func $trap unreachable)
          (func (export "main")
            call $trap
            i32.const 1 i32.const 2 i32.add drop
            i32.const 3 i32.const 4 i32.add drop
          )
        )
        "#,
        Some(1_000),
    );
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, Some(TrapCode::UnreachableCodeReached));
    assert_eq!(rwasm.remaining_fuel, Some(1_000 - 18));
    assert_aligned(&rwasm, &wasmtime);
}

/// A trap in the middle of a straight-line region: the whole region (1 entry + 3 operators) has
/// already been charged and published.
#[test]
fn fuel_matches_when_trap_happens_mid_region() {
    let run = Run {
        params: &[Value::I32(0)],
        results: &[Value::I32(0)],
        ..Run::plain(
            r#"
            (module
              (func (export "main") (param i32) (result i32)
                i32.const 1
                local.get 0
                i32.div_u
              )
            )
            "#,
            Some(1_000),
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, Some(TrapCode::IntegerDivisionByZero));
    assert_eq!(rwasm.remaining_fuel, Some(1_000 - 4));
    assert_aligned(&rwasm, &wasmtime);
}

/// Disabled float operators trap on both engines after charging the code up to and including
/// the operator (1 entry + `f32.const` + `f32.sqrt`); the dead tail behind the trap is neither
/// translated nor charged.
#[cfg(not(feature = "fpu"))]
#[test]
fn fuel_matches_after_disabled_float_opcode() {
    let run = Run {
        results: &[Value::I32(0)],
        ..Run::plain(
            r#"
            (module
              (func (export "main") (result i32)
                f32.const 1
                f32.sqrt
                drop
                i32.const 1 i32.const 2 i32.add
              )
            )
            "#,
            Some(1_000),
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, Some(TrapCode::IllegalOpcode));
    assert_eq!(rwasm.remaining_fuel, Some(1_000 - 3));
    assert_aligned(&rwasm, &wasmtime);
}

/// A leaf function whose straight-line cost (1 entry + 21 operators) exceeds the limit of 10.
/// Neither engine enters the region: both trap with `OutOfFuel` and leave the limit untouched.
#[test]
fn out_of_fuel_matches_on_straight_line_overrun() {
    let run = Run {
        results: &[Value::I32(0)],
        ..Run::plain(
            r#"
            (module
              (func (export "main") (result i32)
                i32.const 0
                i32.const 1 i32.add
                i32.const 1 i32.add
                i32.const 1 i32.add
                i32.const 1 i32.add
                i32.const 1 i32.add
                i32.const 1 i32.add
                i32.const 1 i32.add
                i32.const 1 i32.add
                i32.const 1 i32.add
                i32.const 1 i32.add
              )
            )
            "#,
            Some(10),
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, Some(TrapCode::OutOfFuel));
    assert_eq!(rwasm.remaining_fuel, Some(10));
    assert_aligned(&rwasm, &wasmtime);
}

/// One iteration costs 9, entry costs 1, limit is 30: both engines complete exactly 3 iterations
/// (observable in memory), refuse the 4th at the loop header and keep the 2 fuel left over.
#[test]
fn out_of_fuel_matches_inside_loop() {
    let run = Run {
        memory_prefix: 4,
        ..Run::plain(
            r#"
            (module
              (memory (export "memory") 1)
              (func (export "main")
                (loop $l
                  i32.const 0
                  i32.const 0 i32.load
                  i32.const 1 i32.add
                  i32.store
                  br $l
                )
              )
            )
            "#,
            Some(30),
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, Some(TrapCode::OutOfFuel));
    assert_eq!(rwasm.remaining_fuel, Some(2));
    assert_eq!(rwasm.memory, vec![3, 0, 0, 0]);
    assert_aligned(&rwasm, &wasmtime);
}

/// `fuel_limit: None` runs unbounded and reports no remaining fuel on both engines.
#[test]
fn unbounded_fuel_mode_matches() {
    let run = Run {
        results: &[Value::I32(0)],
        ..Run::plain(
            r#"
            (module
              (func (export "main") (result i32)
                i32.const 40 i32.const 2 i32.add
              )
            )
            "#,
            None,
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, None);
    assert_eq!(rwasm.result, vec![Value::I32(42)]);
    assert_eq!(rwasm.remaining_fuel, None);
    assert_aligned(&rwasm, &wasmtime);
}

/// `LinearFuelParams::param_index` counts parameters from the last one, so with an `i64`
/// parameter above the metered `i32` both engines still meter the `i32`: 320 bytes are 10 words,
/// on top of 1 entry + 2 constants + 10 call.
#[test]
fn syscall_linear_param_index_matches_with_i64_params() {
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("env", "linear_call"),
        1,
        SyscallFuelParams::LinearFuel(LinearFuelParams {
            base_fuel: 0,
            param_index: 2,
            word_cost: 1,
        }),
        &[ValType::I32, ValType::I64],
        &[],
    );
    let run = Run {
        import_linker: Arc::new(import_linker),
        syscall_handler: accepting_syscall_handler,
        ..Run::plain(
            r#"
            (module
              (import "env" "linear_call" (func $l (param i32 i64)))
              (func (export "main")
                i32.const 320
                i64.const 0
                call $l
              )
            )
            "#,
            Some(1_000),
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, None);
    assert_eq!(rwasm.remaining_fuel, Some(1_000 - 23));
    assert_aligned(&rwasm, &wasmtime);
}

fn halting_syscall_handler(
    _caller: &mut TypedCaller<'_, ()>,
    _sys_func_idx: u32,
    _params: &[Value],
    _result: &mut [Value],
) -> Result<(), TrapCode> {
    Err(TrapCode::ExecutionHalted)
}

fn consuming_syscall_handler(
    caller: &mut TypedCaller<'_, ()>,
    _sys_func_idx: u32,
    _params: &[Value],
    _result: &mut [Value],
) -> Result<(), TrapCode> {
    caller.try_consume_fuel(500)
}

fn unbounded_consuming_syscall_handler(
    caller: &mut TypedCaller<'_, ()>,
    _sys_func_idx: u32,
    _params: &[Value],
    _result: &mut [Value],
) -> Result<(), TrapCode> {
    caller.try_consume_fuel(1_000_000)
}

fn linker_with_one_import(name: &'static str, fuel: SyscallFuelParams) -> Arc<ImportLinker> {
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(ImportName::new("env", name), 1, fuel, &[], &[]);
    Arc::new(import_linker)
}

/// Consuming exactly the limit is not out of fuel: 1 entry + 3 operators against a limit of 4.
#[test]
fn exact_limit_matches() {
    let run = Run {
        results: &[Value::I32(0)],
        ..Run::plain(
            r#"
            (module
              (func (export "main") (result i32)
                i32.const 40 i32.const 2 i32.add
              )
            )
            "#,
            Some(4),
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, None);
    assert_eq!(rwasm.result, vec![Value::I32(42)]);
    assert_eq!(rwasm.remaining_fuel, Some(0));
    assert_aligned(&rwasm, &wasmtime);
}

/// The caller's region (1 entry + 10 call) fits in the limit of 15 but the callee's (1 entry +
/// 5 operators) does not: both engines trap at the callee's entry and keep the caller's charge.
#[test]
fn out_of_fuel_matches_at_callee_entry() {
    let run = Run {
        results: &[Value::I32(0)],
        ..Run::plain(
            r#"
            (module
              (func $callee (result i32)
                i32.const 1 i32.const 2 i32.add i32.const 3 i32.add
              )
              (func (export "main") (result i32) call $callee)
            )
            "#,
            Some(15),
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, Some(TrapCode::OutOfFuel));
    assert_eq!(rwasm.remaining_fuel, Some(15 - 11));
    assert_aligned(&rwasm, &wasmtime);
}

/// A failing host call: the caller's region (1 entry + 10 call + 1 tail) and the syscall's
/// constant fuel (5) are both charged before the host runs.
#[test]
fn fuel_matches_when_host_call_fails() {
    let run = Run {
        import_linker: linker_with_one_import("fail", SyscallFuelParams::Const(5)),
        ..Run::plain(
            r#"
            (module
              (import "env" "fail" (func $f))
              (func (export "main") call $f i32.const 1 drop)
            )
            "#,
            Some(1_000),
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, Some(TrapCode::UnknownExternalFunction));
    assert_eq!(rwasm.remaining_fuel, Some(1_000 - 17));
    assert_aligned(&rwasm, &wasmtime);
}

/// A host call that halts execution counts as success on both engines; the caller's tail after
/// the call (3 operators) was already charged with the region.
#[test]
fn fuel_matches_on_execution_halt_with_tail() {
    let run = Run {
        import_linker: linker_with_one_import("exit", SyscallFuelParams::None),
        syscall_handler: halting_syscall_handler,
        ..Run::plain(
            r#"
            (module
              (import "env" "exit" (func $e))
              (func (export "main") call $e i32.const 1 i32.const 2 i32.add drop)
            )
            "#,
            Some(1_000),
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, None);
    assert_eq!(rwasm.remaining_fuel, Some(1_000 - 14));
    assert_aligned(&rwasm, &wasmtime);
}

/// `call_indirect` through a null table entry traps after its region (1 entry + 1 const + 10
/// call + 1 tail) was charged.
#[test]
fn fuel_matches_when_call_indirect_traps() {
    let run = Run::plain(
        r#"
        (module
          (type $t (func))
          (table 1 funcref)
          (func (export "main") i32.const 0 call_indirect (type $t) i32.const 1 drop)
        )
        "#,
        Some(1_000),
    );
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, Some(TrapCode::IndirectCallToNull));
    assert_eq!(rwasm.remaining_fuel, Some(1_000 - 13));
    assert_aligned(&rwasm, &wasmtime);
}

/// An out-of-bounds load in the middle of a region: 1 entry + 1 const + 2 load + 1 const + 1 add.
#[test]
fn fuel_matches_on_memory_out_of_bounds_load() {
    let run = Run {
        results: &[Value::I32(0)],
        ..Run::plain(
            r#"
            (module
              (memory (export "memory") 1)
              (func (export "main") (result i32)
                i32.const 70000 i32.load i32.const 1 i32.add
              )
            )
            "#,
            Some(1_000),
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, Some(TrapCode::MemoryOutOfBounds));
    assert_eq!(rwasm.remaining_fuel, Some(1_000 - 6));
    assert_aligned(&rwasm, &wasmtime);
}

/// Every `end` opens a new region on both engines. Depending on the `br_table` target the trap
/// lands in the region after the inner `end` (1 operator) or after the outer `end` (2
/// operators), on top of the function region (1 entry + `local.get` + `br_table`).
#[test]
fn fuel_matches_across_br_table_targets_and_block_ends() {
    let wat = r#"
        (module
          (func (export "main") (param i32)
            (block $a
              (block $b
                local.get 0
                br_table $b $a)
              i32.const 1 drop
              unreachable)
            i32.const 2 drop
            i32.const 3 drop
            unreachable
          )
        )
        "#;
    for (param, consumed) in [(0, 4), (1, 5)] {
        let run = Run {
            params: &[Value::I32(param)],
            ..Run::plain(wat, Some(1_000))
        };
        let (rwasm, wasmtime) = run.execute();
        assert_eq!(rwasm.trap, Some(TrapCode::UnreachableCodeReached));
        assert_eq!(
            rwasm.remaining_fuel,
            Some(1_000 - consumed),
            "param {param}"
        );
        assert_aligned(&rwasm, &wasmtime);
    }
}

/// A loop with a conditional back-edge: every iteration is one header region (8 for the store,
/// 5 for the compare, 1 for `br_if`) and the fall-through after `br_if` is its own region. With
/// a limit of 40 both engines complete 2 iterations, refuse the 3rd header and keep 11 fuel.
#[test]
fn out_of_fuel_matches_in_loop_with_conditional_back_edge() {
    let run = Run {
        memory_prefix: 4,
        ..Run::plain(
            r#"
            (module
              (memory (export "memory") 1)
              (func (export "main")
                (loop $l
                  i32.const 0 i32.const 0 i32.load i32.const 1 i32.add i32.store
                  i32.const 0 i32.load i32.const 100 i32.lt_u
                  br_if $l)
                i32.const 0 i32.const 0 i32.load i32.const 1000 i32.add i32.store
              )
            )
            "#,
            Some(40),
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, Some(TrapCode::OutOfFuel));
    assert_eq!(rwasm.remaining_fuel, Some(40 - 29));
    assert_eq!(rwasm.memory, vec![2, 0, 0, 0]);
    assert_aligned(&rwasm, &wasmtime);
}

/// `if` without `else`: the function region (1 entry + `local.get` + `if`), the `then` arm (2)
/// and the region after `end` (1) are charged only on the path taken.
#[test]
fn fuel_matches_in_if_without_else_on_both_paths() {
    let wat = r#"
        (module
          (func (export "main") (param i32) (result i32)
            local.get 0
            if
              i32.const 1 drop i32.const 2 drop
            end
            i32.const 7
          )
        )
        "#;
    for (param, consumed) in [(1, 6), (0, 4)] {
        let run = Run {
            params: &[Value::I32(param)],
            results: &[Value::I32(0)],
            ..Run::plain(wat, Some(1_000))
        };
        let (rwasm, wasmtime) = run.execute();
        assert_eq!(rwasm.trap, None);
        assert_eq!(rwasm.result, vec![Value::I32(7)]);
        assert_eq!(
            rwasm.remaining_fuel,
            Some(1_000 - consumed),
            "param {param}"
        );
        assert_aligned(&rwasm, &wasmtime);
    }
}

/// Tail-call recursion until the fuel runs out. `main` costs 1 + 1 + 10; every recursive frame
/// costs 4 on entry (entry, `local.get`, `i32.eqz`, `if`) and 13 after the `end` (`local.get`,
/// `i32.const`, `i32.sub`, `return_call`). The 12th frame's entry does not fit in a limit of 200.
#[test]
fn out_of_fuel_matches_in_tail_call_recursion() {
    let run = Run {
        results: &[Value::I32(0)],
        ..Run::plain(
            r#"
            (module
              (func $rec (param i32) (result i32)
                local.get 0 i32.eqz
                if
                  i32.const 0 return
                end
                local.get 0 i32.const 1 i32.sub
                return_call $rec
              )
              (func (export "main") (result i32) i32.const 1000 return_call $rec)
            )
            "#,
            Some(200),
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, Some(TrapCode::OutOfFuel));
    assert_eq!(rwasm.remaining_fuel, Some(200 - 199));
    assert_aligned(&rwasm, &wasmtime);
}

/// A host call that tries to consume 500 fuel against a limit of 100: the host's charge is
/// refused on both engines and only the region charge (1 entry + 10 call) stays consumed.
#[test]
fn out_of_fuel_matches_when_host_consumes_fuel() {
    let run = Run {
        import_linker: linker_with_one_import("burn", SyscallFuelParams::None),
        syscall_handler: consuming_syscall_handler,
        ..Run::plain(
            r#"
            (module
              (import "env" "burn" (func $b))
              (func (export "main") call $b)
            )
            "#,
            Some(100),
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, Some(TrapCode::OutOfFuel));
    assert_eq!(rwasm.remaining_fuel, Some(100 - 11));
    assert_aligned(&rwasm, &wasmtime);
}

/// In unbounded mode the host can consume any amount of fuel on both engines.
#[test]
fn unbounded_fuel_mode_matches_with_host_consumption() {
    let run = Run {
        import_linker: linker_with_one_import("burn", SyscallFuelParams::None),
        syscall_handler: unbounded_consuming_syscall_handler,
        ..Run::plain(
            r#"
            (module
              (import "env" "burn" (func $b))
              (func (export "main") call $b)
            )
            "#,
            None,
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, None);
    assert_eq!(rwasm.remaining_fuel, None);
    assert_aligned(&rwasm, &wasmtime);
}

/// `reset_fuel` gives the same executor a fresh limit on both engines, including one that is
/// too small for the next run.
#[test]
fn reset_fuel_matches() {
    let run = Run::plain(
        r#"
        (module
          (func (export "main") (result i32) i32.const 40 i32.const 2 i32.add)
        )
        "#,
        Some(100),
    );
    let wasm_binary = wat::parse_str(run.wat).unwrap();
    let outcomes = for_each_strategy(
        |strategy| {
            let mut executor = strategy.create_executor(
                run.import_linker.clone(),
                (),
                run.syscall_handler,
                run.fuel_limit,
                None,
            )?;
            let mut observed = Vec::new();
            for limit in [None, Some(50), Some(3)] {
                if let Some(limit) = limit {
                    executor.reset_fuel(limit);
                }
                let mut result = [Value::I32(0)];
                let trap = executor.execute("main", &[], &mut result).err();
                observed.push((trap, executor.remaining_fuel(), result[0].clone()));
            }
            Ok(observed)
        },
        run.config(),
        &wasm_binary,
    )
    .unwrap();
    assert_eq!(
        outcomes[0],
        vec![
            (None, Some(96), Value::I32(42)),
            (None, Some(46), Value::I32(42)),
            (Some(TrapCode::OutOfFuel), Some(3), Value::I32(0)),
        ]
    );
    assert_eq!(outcomes[0], outcomes[1], "rwasm and wasmtime diverged");
}
