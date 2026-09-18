//! Entry checks and execution guards that remain independent of instruction-pointer bounds.
//!
//! The instruction stream is a trusted compiler artifact. These tests cover the retained entry,
//! linker, and resource checks; they do not execute invalid branch targets or unterminated code,
//! except `out_of_range_branch_target_aborts_the_process`, which does so in a child process and
//! asserts the fatal signal.

use rwasm::{
    always_failing_syscall_handler, instruction_set, ExecutionEngine, ImportLinker, RwasmModule,
    RwasmModuleBuilder, RwasmStore, TrapCode,
};

fn execute(module: &RwasmModule) -> Result<(), TrapCode> {
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::new(
        ImportLinker::default().into(),
        (),
        always_failing_syscall_handler,
        None,
        None,
    );
    engine.execute(&mut store, module, &[], &mut [])
}

fn execute_mut(module: &RwasmModule) -> Result<(), TrapCode> {
    execute(module)
}

/// `targets as usize - 1` used to underflow: a panic with overflow checks, a wrapped clamp plus an
/// 8 GiB jump without them.
#[test]
fn br_table_with_zero_targets_traps() {
    let module = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        I32Const(5)
        BrTable(0)
        Return
    })
    .build();
    assert_eq!(execute_mut(&module), Err(TrapCode::UnreachableCodeReached));
}

/// A syscall index the linker cannot resolve used to panic the interpreter.
#[test]
fn unresolved_syscall_traps() {
    let module = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        Call(0x1234_5678u32)
        Return
    })
    .build();
    assert_eq!(execute_mut(&module), Err(TrapCode::UnknownExternalFunction));
}

/// `CallIndirect`/`TableInit` carry a `TableGet` payload; a different opcode in that slot used to
/// hit `unreachable!`. The compiler guarantees that the slot exists.
#[test]
fn incorrect_table_index_payload_traps() {
    let call_indirect = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        I32Const(0)
        CallIndirect(0)
        Return
    })
    .build();
    assert_eq!(
        execute_mut(&call_indirect),
        Err(TrapCode::UnreachableCodeReached)
    );

    let table_init = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        I32Const(0)
        I32Const(0)
        I32Const(0)
        TableInit(1)
        Return
    })
    .build();
    assert_eq!(
        execute_mut(&table_init),
        Err(TrapCode::UnreachableCodeReached)
    );
}

/// An empty code section used to fetch an opcode through the dangling pointer of an empty `Vec`.
#[test]
fn empty_code_section_traps() {
    assert_eq!(
        execute_mut(&RwasmModule::empty()),
        Err(TrapCode::UnreachableCodeReached)
    );
}

/// Initialization and the low-level executor also reject an empty code section before fetching.
#[test]
fn empty_code_section_traps_on_initialization_paths() {
    let module = RwasmModule::empty();
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::<()>::default();
    assert_eq!(
        engine.entrypoint(&mut store, &module),
        Err(TrapCode::UnreachableCodeReached)
    );

    let mut value_stack = rwasm::ValueStack::default();
    let mut call_stack = rwasm::CallStack::default();
    let mut executor =
        rwasm::RwasmExecutor::entrypoint(&module, &mut value_stack, &mut call_stack, &mut store);
    assert_eq!(
        executor.run(&[], &mut []),
        Err(TrapCode::UnreachableCodeReached)
    );
    assert_eq!(
        executor.run_with_stack_check(),
        Err(TrapCode::UnreachableCodeReached)
    );
}

/// `source_pc` is a decoded module field; an out-of-range value used to be checked by a
/// `debug_assert!` only.
#[test]
fn out_of_range_source_pc_traps() {
    let module = RwasmModuleBuilder::new(instruction_set! { Return })
        .with_source_pc(u32::MAX)
        .build();
    assert_eq!(execute_mut(&module), Err(TrapCode::UnreachableCodeReached));
}

/// One immediate used to size the dropped-segment bitset at ~512 MiB.
#[test]
fn drop_with_an_oversized_segment_index_traps() {
    let data_drop = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        DataDrop(u32::MAX)
        Return
    })
    .build();
    assert_eq!(execute_mut(&data_drop), Err(TrapCode::MemoryOutOfBounds));

    let elem_drop = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        ElemDrop(u32::MAX)
        Return
    })
    .build();
    assert_eq!(execute_mut(&elem_drop), Err(TrapCode::TableOutOfBounds));
}

/// `BulkConst(u32::MAX)` used to spin 4.29e9 times without charging fuel.
#[test]
fn bulk_const_with_an_oversized_immediate_traps_immediately() {
    let module = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        BulkConst(u32::MAX)
        Return
    })
    .build();
    let start = std::time::Instant::now();
    assert_eq!(execute_mut(&module), Err(TrapCode::StackOverflow));
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "the oversized bulk constant must not spin"
    );
}

/// Legitimate control flow keeps working: a counted loop with a backward branch and a forward
/// branch over an untaken arm.
#[test]
fn well_formed_control_flow_is_not_affected() {
    // let i = 3; while (i != 0) { i -= 1; }  -- the counter stays on the stack across the back
    // edge, so the loop head is entered with the same height every iteration.
    let module = RwasmModuleBuilder::new(instruction_set! {
        StackCheck(16)
        I32Const(3)    // 1: [i]
        LocalGet(1)    // 2: loop head, [i, i]
        I32Eqz         // 3: [i, i == 0]
        BrIfNez(4)     // 4: -> 8 (Drop) when the counter reached zero
        I32Const(1)    // 5
        I32Sub         // 6: [i - 1]
        Br(-5)         // 7: -> 2
        Drop           // 8
        Return         // 9
    })
    .build();
    assert_eq!(execute_mut(&module), Ok(()));
}

/// The one test here that does execute an out-of-range branch target, in a child process: the
/// safe public API (`RwasmModuleBuilder`, `RwasmModule::new_checked_exact`) accepts such a module
/// and the interpreter aborts on it, which the maintainers document as intentional. If rwasm ever
/// validates branch targets, invert this into "the module is rejected".
/// Test name of the crashing payload, used to re-invoke this binary in child mode.
const CRASH_TEST: &str = "out_of_range_branch_target_aborts_the_process";

fn run_payload(which: &str) {
    let module = match which {
        // A module built through the safe builder API. `Br` is a compiler-internal opcode, but
        // `RwasmModuleBuilder` is `pub` and accepts any instruction set.
        "builder" => RwasmModuleBuilder::new(instruction_set! { Br(i32::MAX) }).build(),
        // A module loaded from a byte string, the documented way a host restores a cached module.
        // The branch immediate survives `serialize` / `new_checked` unchanged.
        "bytecode" => {
            let bytes = RwasmModuleBuilder::new(instruction_set! { Br(i32::MAX) })
                .build()
                .serialize();
            RwasmModule::new_checked_exact(&bytes).expect("the encoding is well formed")
        }
        other => panic!("unknown payload: {other}"),
    };
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::new(
        ImportLinker::default().into(),
        (),
        always_failing_syscall_handler,
        None,
        None,
    );
    // The interpreter fetches the next instruction from `code_section + i32::MAX`, far outside
    // the code section, and dereferences it.
    let outcome = engine.execute(&mut store, &module, &[], &mut []);
    panic!("rwasm returned {outcome:?} instead of aborting on an out-of-range branch target");
}

#[test]
fn out_of_range_branch_target_aborts_the_process() {
    // Child mode: perform the crashing execution and let the signal kill this process.
    if let Some(which) = std::env::var_os("RWASM_IP_BOUNDS_CHILD") {
        run_payload(&which.to_string_lossy());
        unreachable!("the child must not return from its payload");
    }

    // Parent mode: re-invoke this test binary for each payload and require a fatal signal.
    for which in ["builder", "bytecode"] {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CRASH_TEST, "--nocapture"])
            .env("RWASM_IP_BOUNDS_CHILD", which)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("the test binary must be re-executable");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let status = loop {
            match child
                .try_wait()
                .expect("waiting for the child must succeed")
            {
                Some(status) => break status,
                None if std::time::Instant::now() > deadline => {
                    let _ = child.kill();
                    panic!("the `{which}` payload neither crashed nor returned within 30s");
                }
                None => std::thread::sleep(std::time::Duration::from_millis(25)),
            }
        };

        use std::os::unix::process::ExitStatusExt;
        let signal = status.signal();
        assert!(
            matches!(signal, Some(4) | Some(6) | Some(10) | Some(11)),
            "the `{which}` payload must abort the process, but it exited with {status:?} \
             (signal={signal:?}); a clean exit means the interpreter survived an out-of-range \
             branch target, which would be a correctness bug instead of a crash"
        );
    }
}
