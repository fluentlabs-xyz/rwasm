//! Replacement initialization must preserve the old instance until it can commit.

use rwasm::{
    instruction_set, CompilationConfig, ExecutionEngine, ImportLinker, ImportName, RwasmInstance,
    RwasmModule, RwasmModuleBuilder, RwasmStore, StoreTr, SyscallFuelParams, TrapCode, TypedCaller,
    Value,
};
use std::{collections::VecDeque, sync::Arc};

const ORIGINAL: &str = r#"(module
    (type $t (func (result i32)))
    (memory 1) (table 2 funcref) (global $g (mut i32) (i32.const 7))
    (data (i32.const 0) "A") (data $live "L") (data $gone "D")
    (func $f (result i32) i32.const 11)
    (elem (i32.const 0) $f) (elem $live_elem func $f) (elem $gone_elem func $f)
    (func (export "main") (param $op i32) (result i32)
      (if (i32.eq (local.get $op) (i32.const 0)) (then
        (global.set $g (i32.const 17)) (data.drop $gone) (elem.drop $gone_elem)
        (return (i32.const 0))))
      (if (i32.eq (local.get $op) (i32.const 1)) (then
        (memory.init $live (i32.const 16) (i32.const 0) (i32.const 1))
        (return (i32.load8_u (i32.const 16)))))
      (if (i32.eq (local.get $op) (i32.const 2)) (then
        (memory.init $gone (i32.const 17) (i32.const 0) (i32.const 1))
        (return (i32.const 0))))
      (if (i32.eq (local.get $op) (i32.const 3)) (then
        (table.init $live_elem (i32.const 0) (i32.const 0) (i32.const 1))
        (return (call_indirect (type $t) (i32.const 0)))))
      (if (i32.eq (local.get $op) (i32.const 4)) (then
        (table.init $gone_elem (i32.const 0) (i32.const 0) (i32.const 1))
        (return (i32.const 0))))
      (i32.add (global.get $g)
        (i32.add (i32.load8_u (i32.const 0)) (call_indirect (type $t) (i32.const 0)))))
)"#;

const REPLACEMENT: &str = r#"(module
    (import "env" "stop" (func $stop))
    (memory 1) (table 3 funcref) (global (mut i32) (i32.const 99))
    (data (i32.const 0) "B") (data $live "X") (data $gone "Y")
    (func $f (result i32) i32.const 99)
    (elem (i32.const 0) $f $f) (elem $live_elem func $f) (elem $gone_elem func $f)
    (func $start (data.drop $live) (elem.drop $live_elem) call $stop call $stop)
    (start $start)
    (func (export "main") (result i32) i32.const 0 i32.load8_u)
)"#;

/// Compiles validated Wasm with the same start and entrypoint configuration for each instance.
fn compile(linker: &Arc<ImportLinker>, wat: &str) -> RwasmModule {
    RwasmModule::compile(
        CompilationConfig::default_strategy_compatible()
            .with_allow_start_section(true)
            .with_entrypoint_name("main".into())
            .with_import_linker(linker.clone()),
        &wat::parse_str(wat).unwrap(),
    )
    .unwrap()
    .0
}

/// Returns the next requested host outcome; consumed entries expose callback side effects.
fn stop(
    caller: &mut TypedCaller<'_, VecDeque<Result<(), TrapCode>>>,
    _: u32,
    _: &[Value],
    _: &mut [Value],
) -> Result<(), TrapCode> {
    caller.data_mut().pop_front().expect("expected host call")
}

/// Links the syscall used to control initialization traps, interruptions, and completion.
fn linker() -> Arc<ImportLinker> {
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "stop"),
        1,
        SyscallFuelParams::None,
        &[],
        &[],
    );
    Arc::new(linker)
}

/// Every failure path restores live and dropped segments as well as memory, tables, and globals.
#[test]
fn rollback_restores_all_instance_state_after_traps_and_cancellation() {
    let linker = linker();
    let original = compile(&linker, ORIGINAL);
    let replacement = compile(&linker, REPLACEMENT);
    let engine = ExecutionEngine::new();
    for terminal in [
        TrapCode::UnreachableCodeReached,
        TrapCode::ExecutionHalted,
        TrapCode::OutOfFuel,
    ] {
        for interruptions in [0, 1, 2] {
            // Two calls can either fail on the second one or remain parked for cancellation.
            let outcomes = if interruptions == 2 {
                vec![Err(TrapCode::InterruptionCalled); 2]
            } else {
                let mut outcomes = vec![Err(TrapCode::InterruptionCalled); interruptions];
                outcomes.push(Err(terminal));
                outcomes
            };
            for keep_flags in [false, true] {
                let mut store = RwasmStore::new(
                    linker.clone(),
                    outcomes.clone().into(),
                    stop,
                    Some(10000),
                    Some(1),
                );
                let old = linker
                    .instantiate(&mut store, engine, original.clone())
                    .unwrap();
                old.execute(&mut store, &[Value::I32(0)], &mut [Value::I32(0)])
                    .unwrap();
                let memory = store.memory_snapshot();
                let tables = store.table_snapshots_nullness_prefix(4);
                let fuel_before = store.fuel_consumed();
                let mut outcome = linker
                    .instantiate(&mut store, engine, replacement.clone())
                    .err();
                for step in 0..interruptions {
                    assert_eq!(outcome, Some(TrapCode::InterruptionCalled));
                    assert_eq!(
                        old.execute(&mut store, &[Value::I32(5)], &mut [Value::I32(0)]),
                        Err(TrapCode::IllegalOpcode)
                    );
                    if interruptions == 2 && step == 1 {
                        store.reset(keep_flags);
                        assert_eq!(store.fuel_consumed(), 0);
                    } else {
                        outcome = engine.resume(&mut store, &[], &mut []).err();
                    }
                }
                if interruptions < 2 {
                    assert_eq!(outcome, Some(terminal));
                    assert!(
                        store.fuel_consumed() > fuel_before,
                        "failed initialization must not refund fuel"
                    );
                }
                assert!(
                    store.data().is_empty(),
                    "rollback must not undo host callback side effects"
                );
                assert!(
                    store.memory_snapshot() == memory,
                    "previous memory was not restored"
                );
                assert_eq!(store.table_snapshots_nullness_prefix(4), tables);
                let mut result = [Value::I32(0)];
                old.execute(&mut store, &[Value::I32(5)], &mut result)
                    .unwrap();
                assert_eq!(
                    result,
                    [Value::I32(93)],
                    "restored mutable global + memory byte + indirect call"
                );
                for (op, expected) in [(1, 76), (3, 11)] {
                    old.execute(&mut store, &[Value::I32(op)], &mut result)
                        .unwrap();
                    assert_eq!(
                        result,
                        [Value::I32(expected)],
                        "live segment was not restored"
                    );
                }
                for (op, trap) in [
                    (2, TrapCode::MemoryOutOfBounds),
                    (4, TrapCode::TableOutOfBounds),
                ] {
                    assert_eq!(
                        old.execute(&mut store, &[Value::I32(op)], &mut result),
                        Err(trap),
                        "dropped segment became usable again"
                    );
                }
                assert_eq!(
                    engine.resume(&mut store, &[], &mut []),
                    Err(TrapCode::IllegalOpcode)
                );
            }
        }
    }
}

/// Once resumed initialization succeeds, reset must not resurrect the previous instance.
#[test]
fn successful_resumed_initialization_commits_the_replacement() {
    let linker = linker();
    let engine = ExecutionEngine::new();
    for keep_flags in [false, true] {
        let mut store = RwasmStore::new(
            linker.clone(),
            VecDeque::from([Err(TrapCode::InterruptionCalled), Ok(())]),
            stop,
            Some(10000),
            None,
        );
        let old = linker
            .instantiate(&mut store, engine, compile(&linker, ORIGINAL))
            .unwrap();
        let replacement = compile(&linker, REPLACEMENT);
        assert_eq!(
            linker
                .instantiate(&mut store, engine, replacement.clone())
                .err(),
            Some(TrapCode::InterruptionCalled)
        );
        engine.resume(&mut store, &[], &mut []).unwrap();
        store.reset(keep_flags);
        assert_eq!(
            old.execute(&mut store, &[Value::I32(5)], &mut [Value::I32(0)]),
            Err(TrapCode::IllegalOpcode)
        );
        let mut result = [Value::I32(0)];
        engine
            .execute(&mut store, &replacement, &[], &mut result)
            .unwrap();
        assert_eq!(result, [Value::I32(66)]);
        // A completed transaction must also allow another ordinary instantiation.
        assert!(linker
            .instantiate(&mut store, engine, compile(&linker, ORIGINAL))
            .is_ok());
    }
}

/// A module assembled without an initialization prologue (`source_pc == 0`, what the builder
/// produces) has nothing to run at instantiation: the handle becomes live at once and executes
/// the code section from its start.
#[test]
fn module_without_prologue_instantiates_without_running_anything() {
    let module = RwasmModuleBuilder::new(instruction_set! { I32Const(42) Return }).build();
    assert_eq!(module.source_pc, 0);
    let mut store = RwasmStore::<()>::default();
    let instance = RwasmInstance::new(&mut store, ExecutionEngine::new(), module).unwrap();
    assert_eq!(store.fuel_consumed(), 0);
    let mut result = [Value::I32(0)];
    instance.execute(&mut store, &[], &mut result).unwrap();
    assert_eq!(result, [Value::I32(42)]);
}
