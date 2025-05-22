use rand::Rng;
use rwasm::{DropKeep, ExecutorConfig, InstructionSet, RwasmExecutor, RwasmModule, StackAlloc};

pub fn emit_i64_mul(is: &mut InstructionSet) {
    is.op_stack_alloc(StackAlloc {
        max_stack_height: 10,
    });
    is.op_i32_const(0); // (local i32)
    is.op_i32_const(0); // (local i32)
    is.op_i32_const(0); // (local i32)
    is.op_local_get(4); // local.get 3
    is.op_local_get(6); // local.get 2
    is.op_i32_add(); // i32.add
    is.op_local_get(7); // local.get 1
    is.op_local_get(9); // local.get 0
    is.op_i32_add(); // i32.add
    is.op_i32_mul(); // i32.mul
    is.op_local_get(5); // local.get 3
    is.op_local_get(8); // local.get 1
    is.op_i32_mul(); // i32.mul
    is.op_local_get(7); // local.get 2
    is.op_i32_const(65535); // i32.const 65535
    is.op_i32_and(); // i32.and
    is.op_local_tee(9); // local.tee 1
    is.op_local_get(10); // local.get 0
    is.op_i32_const(65535); // i32.const 65535
    is.op_i32_and(); // i32.and
    is.op_local_tee(8); // local.tee 3
    is.op_i32_mul(); // i32.mul
    is.op_local_tee(6); // local.tee 4
    is.op_local_get(8); // local.get 2
    is.op_i32_const(16); // i32.const 16
    is.op_i32_shr_u(); // i32.shr_u
    is.op_local_tee(6); // local.tee 5
    is.op_local_get(8); // local.get 3
    is.op_i32_mul(); // i32.mul
    is.op_local_tee(8); // local.tee 3
    is.op_local_get(10); // local.get 1
    is.op_local_get(12); // local.get 0
    is.op_i32_const(16); // i32.const 16
    is.op_i32_shr_u(); // i32.shr_u
    is.op_local_tee(7); // local.tee 6
    is.op_i32_mul(); // i32.mul
    is.op_i32_add(); // i32.add
    is.op_local_tee(11); // local.tee 0
    is.op_i32_const(16); // i32.const 16
    is.op_i32_shl(); // i32.shl
    is.op_i32_add(); // i32.add
    is.op_local_tee(8); // local.tee 2
    is.op_i32_add(); // i32.add
    is.op_i32_sub(); // i32.sub
    is.op_local_get(6); // local.get 2
    is.op_local_get(5); // local.get 4
    is.op_i32_lt_u(); // i32.lt_u
    is.op_i32_add(); // i32.add
    is.op_local_get(3); // local.get 5
    is.op_local_get(3); // local.get 6
    is.op_i32_mul(); // i32.mul
    is.op_local_tee(8); // local.tee 1
    is.op_local_get(9); // local.get 0
    is.op_i32_const(16); // i32.const 16
    is.op_i32_shr_u(); // i32.shr_u
    is.op_local_get(10); // local.get 0
    is.op_local_get(8); // local.get 3
    is.op_i32_lt_u(); // i32.lt_u
    is.op_i32_const(16); // i32.const 16
    is.op_i32_shl(); // i32.shl
    is.op_i32_or(); // i32.or
    is.op_i32_add(); // i32.add
    is.op_local_tee(9); // local.tee 0
    is.op_i32_add(); // i32.add
    is.op_local_get(8); // local.get 0
    is.op_local_get(8); // local.get 1
    is.op_i32_lt_u(); // i32.lt_u
    is.op_i32_add(); // i32.add
    is.op_local_get(6);
    // TODO(dmitry123): "how efficiently make drop=7 keep=2?"
    is.op_local_set(7);
    is.op_local_set(7);
    is.op_drop();
    is.op_drop();
    is.op_drop();
    is.op_drop();
    is.op_drop();
}

#[allow(unused)]
pub fn trace_execution_logs(vm: &RwasmExecutor<()>) {
    let trace = vm.tracer().unwrap().logs.len();
    println!("execution trace ({} steps):", trace);
    println!("fuel consumed: {}", vm.fuel_consumed());
    let logs = &vm.tracer().unwrap().logs;
    println!("execution trace ({} steps):", logs.len());
    for log in logs.iter().rev().take(100_000).rev() {
        println!(
            " - pc={} opcode={:?}({:?}) gas={} stack={:?}",
            log.program_counter,
            log.opcode,
            log.value,
            log.consumed_fuel,
            log.stack
                .iter()
                .map(|v| v.to_string())
                .rev()
                .take(100)
                .rev()
                .collect::<Vec<_>>(),
        );
    }
}

fn run_vm_instr(mut is: InstructionSet, inputs: Vec<u32>) -> Vec<u32> {
    is.op_return(DropKeep::none());
    let rwasm_module = RwasmModule::with_one_function(is);
    let mut vm = RwasmExecutor::new(
        rwasm_module.into(),
        ExecutorConfig::default().trace_enabled(true),
        (),
    );
    for i in inputs {
        vm.caller().stack_push(i);
    }
    let exit_code = vm.run().unwrap();
    assert_eq!(exit_code, 0);
    vm.caller()
        .dump_stack()
        .iter()
        .map(|v| v.as_u32())
        .collect::<Vec<_>>()
}

#[test]
fn test_rwasm_i64_mul() {
    let mut is = InstructionSet::new();
    emit_i64_mul(&mut is);

    let test_case_u64 = |a: u64, b: u64| {
        let c = a.wrapping_mul(b);
        let output = run_vm_instr(
            is.clone(),
            vec![a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32],
        );
        assert_eq!(output.len(), 2);
        let r = (output[0] as u64) << 32 | output[1] as u64;
        assert_eq!(c, r);
    };

    let test_case_i64 = |a: i64, b: i64| {
        test_case_u64(a as u64, b as u64);
    };

    // random test cases
    for _ in 0..100_000 {
        let a = rand::rng().random::<u64>();
        let b = rand::rng().random::<u64>();
        test_case_u64(a, b);
        let a = rand::rng().random::<i64>();
        let b = rand::rng().random::<i64>();
        test_case_u64(a as u64, b as u64);
    }

    // u64 test cases
    test_case_u64(0x0000_0000_0000_0000, 0x0000_0000_0000_0000); // 0 × 0
    test_case_u64(0x0000_0000_0000_0000, 0x0000_0000_075B_CD15); // 0 × n
    test_case_u64(0x0000_0000_0000_0001, 0xFFFF_FFFF_FFFF_FFFF); // 1 × max
    test_case_u64(0xFFFF_FFFF_FFFF_FFFF, 0xFFFF_FFFF_FFFF_FFFF); // (−1)²  → 1
    test_case_u64(0x0000_0000_FFFF_FFFF, 0x0000_0000_FFFF_FFFF); // 32-bit × 32-bit
    test_case_u64(0x0000_0001_0000_0000, 0x0000_0001_0000_0000); // 2³² × 2³² → wrap 0
    test_case_u64(0xDEAD_BEEF_CAFE_BABE, 0x1234_5678_9ABC_DEF0); // random dense bits
    test_case_u64(0x8000_0000_0000_0000, 0x0000_0000_0000_0002); // high bit × 2 → 0
    test_case_u64(0x7FFF_FFFF_FFFF_FFFF, 0x0000_0000_0000_0002); // just-below-wrap × 2
    test_case_u64(0x5555_5555_5555_5555, 0xAAAA_AAAA_AAAA_AAAA); // alternating masks

    // i64 test cases
    test_case_i64(0, 0); // 0 × 0
    test_case_i64(0, -123_456_789); // 0 × −n
    test_case_i64(1, -1); // 1 × −1
    test_case_i64(-1, -1); // (−1)²
    test_case_i64(9_223_372_036_854_775_807, 2); // i64::MAX × 2 → wrap
    test_case_i64(i64::MIN, -1); // MIN × −1 → wrap
    test_case_i64(i64::MIN, 2); // MIN × 2  → 0
    test_case_i64(-81_985_529_216_486_895, 538_030_035_483_195_255); // mixed signs, dense
    test_case_i64(-81_985_529_216_486_895, -538_030_035_483_195_255); // neg × neg → pos
    test_case_i64(81_985_529_216_486_895, -81_985_529_216_486_895); // pos × neg
}
