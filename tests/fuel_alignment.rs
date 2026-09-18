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

/// Syscall fuel is part of the import, not of the call site: an import reached through a table
/// entry (`call_indirect`, `return_call_indirect`, a `ref.func` the guest stored itself), an
/// import exported as the entrypoint and an import used as `start` all charge what a direct
/// `call` charges. rwasm charges it in the import trampoline; Wasmtime used to charge it at
/// Cranelift `call` sites only, so every other path ran the builtin for free there (audit round
/// 5, R5-1). Each case charges 17 + 1 entry + the dispatch itself, and the numbers below are
/// pinned so a regression on either engine is visible even if both regress together.
#[test]
fn syscall_fuel_matches_on_every_dispatch_path() {
    let cases = [
        (
            "call",
            "(func (export \"main\") call $c)",
            1_000 - 17 - 1 - 10,
        ),
        (
            "call_indirect",
            "(func (export \"main\") (call_indirect (type $t) (i32.const 0)))",
            1_000 - 17 - 1 - 1 - 10,
        ),
        (
            "return_call_indirect",
            "(func (export \"main\") (return_call_indirect (type $t) (i32.const 0)))",
            1_000 - 17 - 1 - 1 - 10,
        ),
        (
            "ref.func + table.set + call_indirect",
            "(func (export \"main\") (table.set 0 (i32.const 1) (ref.func $c)) \
             (call_indirect (type $t) (i32.const 1)))",
            1_000 - 17 - 1 - 1 - 1 - 3 - 1 - 10,
        ),
        (
            "export-of-import",
            "(export \"main\" (func $c))",
            1_000 - 17,
        ),
    ];
    for (label, body, expected_remaining) in cases {
        let wat = format!(
            r#"
            (module
              (type $t (func))
              (import "env" "const_call" (func $c))
              (table 2 funcref)
              (elem (i32.const 0) $c)
              {body}
            )
            "#
        );
        let run = Run {
            import_linker: linker_with_one_import("const_call", SyscallFuelParams::Const(17)),
            syscall_handler: accepting_syscall_handler,
            ..Run::plain(&wat, Some(1_000))
        };
        let (rwasm, wasmtime) = run.execute();
        assert_eq!(rwasm.trap, None, "{label}");
        assert_eq!(rwasm.remaining_fuel, Some(expected_remaining), "{label}");
        assert_aligned(&rwasm, &wasmtime);
    }
}

/// The `start` variant of the previous test: the import runs during instantiation, before the
/// entrypoint, and charges its fuel there on both engines.
#[test]
fn syscall_fuel_matches_when_start_is_an_import() {
    let wasm_binary = wat::parse_str(
        r#"
        (module
          (import "env" "const_call" (func $c))
          (start $c)
          (func (export "main"))
        )
        "#,
    )
    .unwrap();
    let import_linker = linker_with_one_import("const_call", SyscallFuelParams::Const(17));
    let outcomes = for_each_strategy(
        |strategy| {
            let mut executor = strategy.create_executor(
                import_linker.clone(),
                (),
                accepting_syscall_handler,
                Some(1_000),
                None,
            )?;
            let after_instantiation = executor.remaining_fuel();
            executor.execute("main", &[], &mut [])?;
            Ok((after_instantiation, executor.remaining_fuel()))
        },
        CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_allow_start_section(true)
            .with_import_linker(import_linker.clone())
            .with_builtins_consume_fuel(true),
        &wasm_binary,
    )
    .unwrap();
    assert_eq!(outcomes[0], (Some(1_000 - 17), Some(1_000 - 17 - 1)));
    assert_eq!(outcomes[0], outcomes[1], "rwasm and wasmtime diverged");
}

/// A `LinearFuel` builtin reached through a table with a 1 MiB length is charged
/// 7 + 5 * 32768 on both engines; a loop of them runs out of fuel at the same iteration instead of
/// completing 100 MiB of metered host work for the loop's own cost on one engine.
#[test]
fn out_of_fuel_matches_for_metered_builtins_called_through_a_table() {
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("env", "linear_call"),
        1,
        SyscallFuelParams::LinearFuel(LinearFuelParams {
            base_fuel: 7,
            param_index: 1,
            word_cost: 5,
        }),
        &[ValType::I32],
        &[],
    );
    let run = Run {
        import_linker: Arc::new(import_linker),
        syscall_handler: accepting_syscall_handler,
        params: &[Value::I32(1_048_576), Value::I32(100)],
        ..Run::plain(
            r#"
            (module
              (type $t (func (param i32)))
              (import "env" "linear_call" (func $l (param i32)))
              (table 1 funcref)
              (elem (i32.const 0) $l)
              (func (export "main") (param $bytes i32) (param $iters i32)
                (block
                  (loop
                    (br_if 1 (i32.eqz (local.get $iters)))
                    (call_indirect (type $t) (local.get $bytes) (i32.const 0))
                    (local.set $iters (i32.sub (local.get $iters) (i32.const 1)))
                    (br 0)))
              )
            )
            "#,
            Some(1_000_000),
        )
    };
    let (rwasm, wasmtime) = run.execute();
    assert_eq!(rwasm.trap, Some(TrapCode::OutOfFuel));
    assert_aligned(&rwasm, &wasmtime);
}

/// The import trampoline reserves the temporaries of its fuel prologue (two slots for
/// `LinearFuel`, four for `QuadraticFuel`). A function whose stack peak sits exactly at the
/// value stack's initial capacity — one parameter, the locals below and one argument make 32 —
/// used to trap `StackOverflow` on rwasm at the prologue's first push while Wasmtime ran it
/// (audit round 5, R5-2). Every peak from well below to well above the boundary must agree.
#[test]
fn metered_builtin_call_at_stack_capacity_matches() {
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(
        ImportName::new("env", "linear_call"),
        1,
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
        2,
        SyscallFuelParams::QuadraticFuel(QuadraticFuelParams {
            local_depth: 1,
            word_cost: 3,
            divisor: 2,
            fuel_denom_rate: 4,
        }),
        &[ValType::I32],
        &[],
    );
    let import_linker = Arc::new(import_linker);
    for import in ["linear_call", "quadratic_call"] {
        for locals in 24..=36 {
            let wat = format!(
                r#"
                (module
                  (import "env" "{import}" (func $builtin (param i32)))
                  (func (export "main") (param i32) (result i32) (local {locals})
                    (call $builtin (local.get 0))
                    (local.get 0))
                )
                "#,
                locals = vec!["i32"; locals].join(" ")
            );
            let run = Run {
                import_linker: import_linker.clone(),
                syscall_handler: accepting_syscall_handler,
                params: &[Value::I32(64)],
                results: &[Value::I32(0)],
                ..Run::plain(&wat, Some(10_000))
            };
            let (rwasm, wasmtime) = run.execute();
            assert_eq!(rwasm.trap, None, "{import} with {locals} locals");
            assert_eq!(
                rwasm.result,
                vec![Value::I32(64)],
                "{import} with {locals} locals"
            );
            assert_aligned(&rwasm, &wasmtime);
        }
    }
}

/// A memory access whose immediate offset plus its size exceeds the largest size the memory can
/// ever have — a declared maximum, or 4 GiB without one — can only trap. Cranelift proves that at
/// compile time, lowers the access to an unconditional trap and stops translating the block, so
/// the Wasmtime backend charges the region only up to and including the access. rwasm used to
/// emit the ordinary access and charge the whole region (audit round 7, R7-1); it now emits the
/// same unconditional trap and ends the path, so both engines charge 1 entry + the operators up
/// to the access and leave the rest of the region unpaid. The dynamic and boundary cases below
/// pin the other side of the rule: an access that *can* be in bounds still charges its whole
/// region on both engines.
#[test]
fn fuel_matches_after_statically_out_of_bounds_access() {
    // (label, module, remaining fuel out of 1000, trap)
    let mut cases: Vec<(&str, &str, u64, TrapCode)> = vec![
        (
            "offset beyond the declared maximum",
            r#"(module (memory (export "memory") 1 2)
               (func (export "main")
                 (drop (i32.load offset=131072 (i32.const 0)))
                 (drop (i32.const 1)) (drop (i32.const 1)) unreachable))"#,
            1000 - 1 - 1 - 2,
            TrapCode::MemoryOutOfBounds,
        ),
        (
            "offset + size beyond the declared maximum",
            r#"(module (memory (export "memory") 1 2)
               (func (export "main")
                 (drop (i32.load offset=131069 (i32.const 0)))
                 (drop (i32.const 1)) (drop (i32.const 1)) unreachable))"#,
            1000 - 1 - 1 - 2,
            TrapCode::MemoryOutOfBounds,
        ),
        (
            "offset + size exactly the maximum is a dynamic access",
            r#"(module (memory (export "memory") 1 2)
               (func (export "main")
                 (drop (i32.load offset=131068 (i32.const 0)))
                 (drop (i32.const 1)) (drop (i32.const 1)) unreachable))"#,
            1000 - 1 - 1 - 2 - 1 - 1,
            TrapCode::MemoryOutOfBounds,
        ),
        (
            "a zero-maximum memory makes every access static",
            r#"(module (memory (export "memory") 0 0)
               (func (export "main")
                 (drop (i32.load (i32.const 0)))
                 (drop (i32.const 1)) (drop (i32.const 1)) unreachable))"#,
            1000 - 1 - 1 - 2,
            TrapCode::MemoryOutOfBounds,
        ),
        (
            "stores follow the same rule",
            r#"(module (memory (export "memory") 0 1)
               (func (export "main")
                 (i32.store offset=65536 (i32.const 0) (i32.const 0))
                 (drop (i32.const 1)) (drop (i32.const 1)) unreachable))"#,
            1000 - 1 - 1 - 1 - 2,
            TrapCode::MemoryOutOfBounds,
        ),
        (
            "without a maximum the bound is 4 GiB: an i64 at 0xffffffff is static",
            r#"(module (memory (export "memory") 1)
               (func (export "main")
                 (drop (i64.load offset=0xffffffff (i32.const 0)))
                 (drop (i32.const 1)) (drop (i32.const 1)) unreachable))"#,
            1000 - 1 - 1 - 2,
            TrapCode::MemoryOutOfBounds,
        ),
        (
            "without a maximum an i32 at 0xfffffffc still fits and is dynamic",
            r#"(module (memory (export "memory") 1)
               (func (export "main")
                 (drop (i32.load offset=0xfffffffc (i32.const 0)))
                 (drop (i32.const 1)) (drop (i32.const 1)) unreachable))"#,
            1000 - 1 - 1 - 2 - 1 - 1,
            TrapCode::MemoryOutOfBounds,
        ),
    ];
    // Without `fpu` a float access is a disabled opcode on both engines, and that lowering takes
    // precedence over the bounds rule. (An `fpu` build pairs with a Wasmtime build that still
    // disables floats, so float operators are not comparable there; see the feature's docs.)
    if !cfg!(feature = "fpu") {
        cases.push((
            "a disabled float access traps as an illegal opcode before the bounds rule",
            r#"(module (memory (export "memory") 1 1)
               (func (export "main")
                 (drop (f32.load offset=65536 (i32.const 0)))
                 (drop (i32.const 1)) (drop (i32.const 1)) unreachable))"#,
            1000 - 1 - 1 - 2,
            TrapCode::IllegalOpcode,
        ));
    }
    for (label, wat, remaining, trap) in cases {
        let (rwasm, wasmtime) = Run::plain(wat, Some(1_000)).execute();
        assert_eq!(rwasm.trap, Some(trap), "{label}");
        assert_eq!(rwasm.remaining_fuel, Some(remaining), "{label}");
        assert_aligned(&rwasm, &wasmtime);
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
    //! of `memory.fill` for 14 fuel, ~24 000× the per-fuel cost of ordinary instructions.
    //!
    //! Ignored until https://github.com/fluentlabs-xyz/wasmtime/pull/12 is merged and released:
    //! the fix is a dynamic charge inside the Wasmtime fork (`Config::rwasm_bulk_fuel`), which is
    //! a substantial change to that code base. It is not needed for the current deployment — the
    //! Wasmtime strategy runs trusted system code only, and contracts run on the rwasm VM with
    //! `CompilationConfig::default()`, where bulk operations are metered — so the tests stay in
    //! the suite as the executable statement of the contract, `--ignored` runs them and prints
    //! the measurement, and they turn green with the fork release.

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
    // Ignored until https://github.com/fluentlabs-xyz/wasmtime/pull/12 is merged: needs
    // `Config::rwasm_bulk_fuel` in wasmtime-rwasm 45.0.0-rwasm.3.
    #[test]
    #[ignore = "HIGH-8: ignored until fluentlabs-xyz/wasmtime#12 is merged; run with --ignored to reproduce"]
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
    /// the fork meters bulk operations, `default_strategy_compatible()` keeps the dynamic charge
    /// and the size-metered config is accepted by the strategy layer. The failure message carries
    /// the measurement that motivates the finding.
    // Ignored until https://github.com/fluentlabs-xyz/wasmtime/pull/12 is merged, like the test
    // above.
    #[test]
    #[ignore = "HIGH-8: ignored until fluentlabs-xyz/wasmtime#12 is merged; run with --ignored to reproduce"]
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
