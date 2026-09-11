use super::*;
use crate::{always_failing_syscall_handler, ImportLinker, Pages, RwasmCaller, RwasmStore};
use alloc::sync::Arc;

/// Covers the syscall handler's host-state and memory operations.
#[test]
fn simple_call_handler_exercises_host_state_and_memory() {
    let context = SimpleCallContext {
        input: b"abcdef".to_vec(),
        state: 42,
        ..Default::default()
    };
    let mut store = RwasmStore::new(
        Arc::new(ImportLinker::default()),
        context,
        always_failing_syscall_handler,
        None,
        Some(1),
    );
    assert_eq!(
        store.global_memory.grow(Pages::new(1).unwrap()),
        Some(Pages::default())
    );
    let mut caller = TypedCaller::Rwasm(RwasmCaller::new(&mut store));

    let mut result = [Value::I32(0)];
    simple_call_handler_syscall_handler(&mut caller, 0x0002, &[], &mut result).unwrap();
    assert_eq!(result[0], Value::I32(42));

    simple_call_handler_syscall_handler(&mut caller, 0x0004, &[], &mut result).unwrap();
    assert_eq!(result[0], Value::I32(6));

    let read_params = [Value::I32(16), Value::I32(1), Value::I32(3)];
    simple_call_handler_syscall_handler(&mut caller, 0x0003, &read_params, &mut []).unwrap();
    assert_eq!(caller.memory_read_into_vec(16, 3).unwrap(), b"bcd");

    let write_params = [Value::I32(16), Value::I32(3)];
    simple_call_handler_syscall_handler(&mut caller, 0x0005, &write_params, &mut []).unwrap();
    assert_eq!(caller.data().output, b"bcd");

    let hash_params = [Value::I32(16), Value::I32(3), Value::I32(64)];
    simple_call_handler_syscall_handler(&mut caller, 0x0101, &hash_params, &mut []).unwrap();
    assert_eq!(
        caller.memory_read_into_vec(64, 32).unwrap(),
        hex_literal::hex!("c08bb9a33a7cd38850fa6ce966af52a86dba268e2d9502b4ccbd012668969455")
    );

    let exit_params = [Value::I32(7)];
    assert_eq!(
        simple_call_handler_syscall_handler(&mut caller, 0x0001, &exit_params, &mut []),
        Err(TrapCode::ExecutionHalted)
    );
    assert_eq!(caller.data().exit_code, 7);
}

/// Covers the rWasm typed-caller accessors and delegated store operations.
#[test]
fn typed_rwasm_caller_exposes_store_operations_and_accessors() {
    let mut store = RwasmStore::new(
        Arc::new(ImportLinker::default()),
        7_u32,
        always_failing_syscall_handler,
        Some(100),
        Some(1),
    );
    assert_eq!(
        store.global_memory.grow(Pages::new(1).unwrap()),
        Some(Pages::default())
    );
    let mut caller = TypedCaller::Rwasm(RwasmCaller::new(&mut store));

    assert_eq!(caller.as_rwasm_ref().data(), &7);
    *caller.as_rwasm_mut().data_mut() = 8;
    caller.memory_write(4, &[1, 2, 3, 4]).unwrap();
    let mut buffer = [0_u8; 4];
    caller.memory_read(4, &mut buffer).unwrap();
    assert_eq!(buffer, [1, 2, 3, 4]);
    assert_eq!(caller.memory_read_into_vec(5, 2).unwrap(), [2, 3]);
    assert_eq!(caller.remaining_fuel(), Some(100));
    caller.try_consume_fuel(9).unwrap();
    assert_eq!(caller.remaining_fuel(), Some(91));
    caller.reset_fuel(50);
    assert_eq!(caller.remaining_fuel(), Some(50));

    let caller = caller.into_rwasm();
    assert_eq!(caller.data(), &8);
}

/// Adversarial parameters must trap, never panic or allocate before the range is validated.
#[test]
fn simple_call_handler_rejects_adversarial_parameters() {
    let context = SimpleCallContext {
        input: b"abcdef".to_vec(),
        ..Default::default()
    };
    let mut store = RwasmStore::new(
        Arc::new(ImportLinker::default()),
        context,
        always_failing_syscall_handler,
        None,
        Some(1),
    );
    store.global_memory.grow(Pages::new(1).unwrap()).unwrap();
    let mut caller = TypedCaller::Rwasm(RwasmCaller::new(&mut store));
    let call = |caller: &mut TypedCaller<SimpleCallContext>, func_idx, params: &[Value]| {
        simple_call_handler_syscall_handler(caller, func_idx, params, &mut [])
    };

    // `offset + length` wraps around on the host input buffer
    let wrap = [Value::I32(0), Value::I32(-1), Value::I32(2)];
    assert_eq!(
        call(&mut caller, 0x0003, &wrap),
        Err(TrapCode::MemoryOutOfBounds)
    );
    // the input range is out of bounds
    let oob = [Value::I32(0), Value::I32(4), Value::I32(3)];
    assert_eq!(
        call(&mut caller, 0x0003, &oob),
        Err(TrapCode::MemoryOutOfBounds)
    );
    // a 4 GiB output read is rejected by the range check before any buffer exists
    let huge = [Value::I32(0), Value::I32(-1)];
    assert_eq!(
        call(&mut caller, 0x0005, &huge),
        Err(TrapCode::MemoryOutOfBounds)
    );
    assert!(caller.data().output.is_empty());
    // the same for the hashed range, and a hash output offset past the end of memory
    let huge = [Value::I32(0), Value::I32(-1), Value::I32(0)];
    assert_eq!(
        call(&mut caller, 0x0101, &huge),
        Err(TrapCode::MemoryOutOfBounds)
    );
    let past_end = [Value::I32(0), Value::I32(4), Value::I32(65535)];
    assert_eq!(
        call(&mut caller, 0x0101, &past_end),
        Err(TrapCode::MemoryOutOfBounds)
    );
    // missing or mistyped parameters are a signature error
    assert_eq!(call(&mut caller, 0x0001, &[]), Err(TrapCode::BadSignature));
    assert_eq!(
        call(
            &mut caller,
            0x0003,
            &[Value::I64(0), Value::I64(0), Value::I64(0)]
        ),
        Err(TrapCode::BadSignature)
    );
    let mut no_result: [Value; 0] = [];
    assert_eq!(
        simple_call_handler_syscall_handler(&mut caller, 0x0002, &[], &mut no_result),
        Err(TrapCode::BadSignature)
    );
    // an unknown import index is an unknown external function
    assert_eq!(
        call(&mut caller, 0xdead, &[]),
        Err(TrapCode::UnknownExternalFunction)
    );
}
