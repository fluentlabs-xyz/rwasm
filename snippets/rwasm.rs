use rwasm::RwasmExecutor;

#[allow(unused)]
pub fn trace_execution_logs(vm: &RwasmExecutor<()>) {
    let trace = vm.tracer().unwrap().logs.len();
    println!("execution trace ({} steps):", trace);
    println!("fuel consumed: {}", vm.fuel_consumed());
    let logs = &vm.tracer().unwrap().logs;
    println!("execution trace ({} steps):", logs.len());
    for log in logs.iter().rev().take(100_000).rev() {
        println!(
            " - pc={} opcode={} gas={} stack={:?}",
            log.program_counter,
            log.opcode,
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

#[cfg(test)]
mod tests {
    use rwasm::{ExecutorConfig, InstructionSet, RwasmExecutor, RwasmModule};

    fn run_vm_instr(mut is: InstructionSet, inputs: Vec<u32>) -> Vec<u32> {
        is.op_return();
        let rwasm_module = RwasmModule::with_one_function(is);
        let mut vm = RwasmExecutor::new(
            rwasm_module.into(),
            ExecutorConfig::default().trace_enabled(true),
            (),
        );
        for i in inputs {
            vm.caller().stack_push(i);
        }
        vm.run().unwrap();
        vm.caller()
            .dump_stack()
            .iter()
            .map(|v| v.as_u32())
            .collect::<Vec<_>>()
    }

    fn run_binary_test_case(is: &InstructionSet, a: u64, b: u64, c: u64) {
        let output = run_vm_instr(
            is.clone(),
            vec![a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32],
        );
        assert_eq!(output.len(), 2);
        let r = (output[1] as u64) << 32 | output[0] as u64;
        assert_eq!(c, r);
    }

    fn run_unary_test_case(is: &InstructionSet, a: u64, c: u64) {
        let output = run_vm_instr(is.clone(), vec![a as u32, (a >> 32) as u32]);
        assert_eq!(output.len(), 1);
        let r = output[0] as u64;
        assert_eq!(c, r);
    }

    #[test]
    fn test_i64_const() {
        let test_case_u64 = |a: i64| {
            let mut is = InstructionSet::new();
            is.op_i64_const(a);
            let output = run_vm_instr(is.clone(), vec![]);
            assert_eq!(output.len(), 2);
            let r = (output[1] as u64) << 32 | output[0] as u64;
            assert_eq!(a, r as i64);
        };

        test_case_u64(0); // zero
        test_case_u64(1); // one
        test_case_u64(-1); // minus one
        test_case_u64(i32::MAX as i64); // max i32
        test_case_u64(i32::MIN as i64); // min i32
        test_case_u64(0xFFFF_FFFF); // low all 1s, hi 0
        test_case_u64(0x1_0000_0000); // low 0, hi 1
        test_case_u64(0x7FFF_FFFF_FFFF_FFFF); // max positive i64
        test_case_u64(0x8000_0000_0000_0000u64 as i64); // min i64 (sign bit)
        test_case_u64(0xFFFF_FFFF_FFFF_FFFFu64 as i64); // all bits set (as i64 == -1)
        test_case_u64(0xDEAD_BEEF_DEAD_BEEFu64 as i64); // repeated
        test_case_u64(0x8000_0001_0000_0001u64 as i64); // hi/lo with sign and 1
        test_case_u64(0x0123_4567_89AB_CDEF); // pattern

        // random test cases
        // for _ in 0..100_000 {
        //     use rand::Rng;
        //     let a = rand::rng().random::<u64>();
        //     let b = rand::rng().random::<u64>();
        //     test_case_u64(a, b);
        //     let a = rand::rng().random::<i64>();
        //     let b = rand::rng().random::<i64>();
        //     test_case_u64(a as u64, b as u64);
        // }
    }

    #[test]
    fn test_i64_mul() {
        let mut is = InstructionSet::new();
        is.op_i64_mul();

        let test_case_u64 = |a: u64, b: u64| {
            let c = a.wrapping_mul(b);
            run_binary_test_case(&is, a, b, c);
        };

        let test_case_i64 = |a: i64, b: i64| {
            let c = a.wrapping_mul(b);
            run_binary_test_case(&is, a as u64, b as u64, c as u64);
        };

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

        // let mut rng = StdRng::seed_from_u64(42); // deterministic randomness
        // for _ in 0..100 {
        //     let a: u64 = rng.gen();
        //     let b: u64 = rng.gen();
        //     test_case_u64(a, b);
        // }

        // // random test cases
        // for _ in 0..100_000 {
        //     use rand::Rng;
        //     let a = rand::rng().random::<u64>();
        //     let b = rand::rng().random::<u64>();
        //     test_case_u64(a, b);
        //     let a = rand::rng().random::<i64>();
        //     let b = rand::rng().random::<i64>();
        //     test_case_u64(a as u64, b as u64);
        // }
    }

    #[test]
    fn test_i64_eqz() {
        let mut is = InstructionSet::new();
        is.op_i64_eqz();

        let test_case_u64 = |a: u64| {
            let c = (a == 0) as u64;
            run_unary_test_case(&is, a, c);
        };

        test_case_u64(0x0000_0000_0000_0000); // zero
        test_case_u64(0x0000_0000_0000_0001); // one
        test_case_u64(0x0000_0000_FFFF_FFFF); // low bits all 1
        test_case_u64(0x0000_0001_0000_0000); // single high bit (low 32 overflow)
        test_case_u64(0xFFFF_FFFF_0000_0000); // high bits only
        test_case_u64(0x0000_0000_FFFF_FFFE); // edge low -2
        test_case_u64(0x7FFF_FFFF_FFFF_FFFF); // max signed positive
        test_case_u64(0x8000_0000_0000_0000); // min signed (sign bit set)
        test_case_u64(0xFFFF_FFFF_FFFF_FFFF); // all bits set (max u64)
        test_case_u64(0x0000_0001_FFFF_FFFF); // low+hi edge
        test_case_u64(0x1234_5678_9ABC_DEF0); // random pattern 1
        test_case_u64(0xFEDC_BA98_7654_3210); // random pattern 2
        test_case_u64(0xDEAD_BEEF_DEAD_BEEF); // repeated pattern
        test_case_u64(0x0000_FFFF_0000_FFFF); // pattern
        test_case_u64(0x8000_0000_0000_0001); // sign bit + lo bit
        test_case_u64(0x7FFF_FFFF_8000_0000); // just below and just above sign
        test_case_u64(0xFFFF_FFFF_7FFF_FFFF); // hi max, low just below sign
        test_case_u64(0xFFFF_FFFF_FFFF_0000); // upper max, lower zeros
        test_case_u64(0x0000_0000_8000_0000); // only lo sign bit
        test_case_u64(0x8000_0000_0000_8000); // sign bit and small lo

        // // random test cases
        // for _ in 0..100_000 {
        //     use rand::Rng;
        //     let a = rand::rng().random::<u64>();
        //     test_case_u64(a);
        // }
    }

    #[test]
    fn test_i64_sub() {
        let mut is = InstructionSet::new();
        is.op_i64_sub();

        let test_case_u64 = |a: u64, b: u64| {
            let c = a.wrapping_sub(b);
            run_binary_test_case(&is, a, b, c);
        };

        test_case_u64(0, 0); // 0 - 0 = 0
        test_case_u64(1, 0); // 1 - 0 = 1
        test_case_u64(0, 1); // 0 - 1 = underflow (wraps to max)
        test_case_u64(0xFFFF_FFFFu64, 1); // lo only, no borrow
        test_case_u64(0x1_0000_0000, 1); // hi only, low borrows
        test_case_u64(u64::MAX, 1); // max - 1 = max - 1
        test_case_u64(u64::MAX, u64::MAX); // max - max = 0
        test_case_u64(0x8000_0000_0000_0000, 1); // min signed - 1
        test_case_u64(0x8000_0000_0000_0000, 0x7FFF_FFFF_FFFF_FFFF);
        test_case_u64(0x1_0000_0000, 0xFFFF_FFFF); // (2^32) - (2^32-1) = 1
        test_case_u64(0x1_0000_0001, 0x1_0000_0000); // cross 32-bit boundary
        test_case_u64(0x1234_5678_9ABC_DEF0, 0x1111_1111_1111_1111);
        test_case_u64(0, u64::MAX); // 0 - max = 1 (wrap)
        test_case_u64(0xDEAD_BEEF_DEAD_BEEF, 0xCAFEBABE_CAFEBABE);

        // random test cases
        // for _ in 0..100_000 {
        //     use rand::Rng;
        //     let a = rand::rng().random::<u64>();
        //     let b = rand::rng().random::<u64>();
        //     test_case_u64(a, b);
        //     let a = rand::rng().random::<i64>();
        //     let b = rand::rng().random::<i64>();
        //     test_case_u64(a as u64, b as u64);
        // }
    }

    #[test]
    fn test_i64_le_u() {
        let mut is = InstructionSet::new();
        is.op_i64_le_u();

        let test_case_u64 = |a: u64, b: u64| {
            let c = (a <= b) as u64;
            let output = run_vm_instr(
                is.clone(),
                vec![a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32],
            );
            assert_eq!(output.len(), 1);
            let r = output[0] as u64;
            assert_eq!(c, r);
        };

        // test_case_u64(0, 0); // 0 - 0 = 0
        test_case_u64(1, 0); // 1 - 0 = 1
        test_case_u64(0, 1); // 0 - 1 = underflow (wraps to max)
        test_case_u64(0xFFFF_FFFFu64, 1); // lo only, no borrow
        test_case_u64(0x1_0000_0000, 1); // hi only, low borrows
        test_case_u64(u64::MAX, 1); // max - 1 = max - 1
        test_case_u64(u64::MAX, u64::MAX); // max - max = 0
        test_case_u64(0x8000_0000_0000_0000, 1); // min signed - 1
        test_case_u64(0x8000_0000_0000_0000, 0x7FFF_FFFF_FFFF_FFFF);
        test_case_u64(0x1_0000_0000, 0xFFFF_FFFF); // (2^32) - (2^32-1) = 1
        test_case_u64(0x1_0000_0001, 0x1_0000_0000); // cross 32-bit boundary
        test_case_u64(0x1234_5678_9ABC_DEF0, 0x1111_1111_1111_1111);
        test_case_u64(0, u64::MAX); // 0 - max = 1 (wrap)
        test_case_u64(0xDEAD_BEEF_DEAD_BEEF, 0xCAFEBABE_CAFEBABE);

        // random test cases
        // for _ in 0..100_000 {
        //     use rand::Rng;
        //     let a = rand::rng().random::<u64>();
        //     let b = rand::rng().random::<u64>();
        //     test_case_u64(a, b);
        //     let a = rand::rng().random::<i64>();
        //     let b = rand::rng().random::<i64>();
        //     test_case_u64(a as u64, b as u64);
        // }
    }

    #[test]
    fn test_i64_add() {
        let mut is = InstructionSet::new();
        is.op_i64_add();

        let test_case_u64 = |a: u64, b: u64| {
            let c = a.wrapping_add(b);
            run_binary_test_case(&is, a, b, c);
        };

        test_case_u64(0, 0); // 0 - 0 = 0
        test_case_u64(1, 0); // 1 - 0 = 1
        test_case_u64(0, 1); // 0 - 1 = underflow (wraps to max)
        test_case_u64(0xFFFF_FFFFu64, 1); // lo only, no borrow
        test_case_u64(0x1_0000_0000, 1); // hi only, low borrows
        test_case_u64(u64::MAX, 1); // max - 1 = max - 1
        test_case_u64(u64::MAX, u64::MAX); // max - max = 0
        test_case_u64(0x8000_0000_0000_0000, 1); // min signed - 1
        test_case_u64(0x8000_0000_0000_0000, 0x7FFF_FFFF_FFFF_FFFF);
        test_case_u64(0x1_0000_0000, 0xFFFF_FFFF); // (2^32) - (2^32-1) = 1
        test_case_u64(0x1_0000_0001, 0x1_0000_0000); // cross 32-bit boundary
        test_case_u64(0x1234_5678_9ABC_DEF0, 0x1111_1111_1111_1111);
        test_case_u64(0, u64::MAX); // 0 - max = 1 (wrap)
        test_case_u64(0xDEAD_BEEF_DEAD_BEEF, 0xCAFEBABE_CAFEBABE);

        // random test cases
        // for _ in 0..100_000 {
        //     use rand::Rng;
        //     let a = rand::rng().random::<u64>();
        //     let b = rand::rng().random::<u64>();
        //     test_case_u64(a, b);
        //     let a = rand::rng().random::<i64>();
        //     let b = rand::rng().random::<i64>();
        //     test_case_u64(a as u64, b as u64);
        // }
    }
}
