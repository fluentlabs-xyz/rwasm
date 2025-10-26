use rand::Rng;
/// |-----------------------|---------|
/// | Opcode                | Covered |
/// |-----------------------|---------|
/// | op_i64_add            |     +   |
/// | op_i64_sub            |     +   |
/// | op_i64_clz            |     +   |
/// | op_i64_ctz            |     +   |
/// | op_i64_popcnt         |     +   |
/// | op_i64_and            |     +   |
/// | op_i64_or             |     +   |
/// | op_i64_xor            |     +   |
/// | op_i64_shl            |     +   |
/// | op_i64_shr_s          |     +   |
/// | op_i64_shr_u          |     +   |
/// | op_i64_rotl           |     +   |
/// | op_i64_rotr           |     +   |
/// | op_i64_eqz            |     +   |
/// | op_i64_eq             |         |
/// | op_i64_ne             |         |
/// | op_i64_lt_s           |         |
/// | op_i64_lt_u           |         |
/// | op_i64_gt_s           |         |
/// | op_i64_gt_u           |         |
/// | op_i64_le_s           |         |
/// | op_i64_le_u           |     +   |
/// | op_i64_ge_s           |         |
/// | op_i64_ge_u           |         |
/// | op_i32_wrap_i64       |         |
/// | op_i64_extend_i32_s   |     +   |
/// | op_i64_extend_i32_u   |     +   |
/// | op_i64_extend8_s      |     +   |
/// | op_i64_extend16_s     |     +   |
/// | op_i64_extend32_s     |     +   |
/// | op_i64_div_s          |     +   |
/// | op_i64_div_u          |     +   |
/// | op_i64_load           |         |
/// | op_i64_load8_s        |         |
/// | op_i64_load8_u        |         |
/// | op_i64_load16_s       |         |
/// | op_i64_load16_u       |         |
/// | op_i64_load32_s       |         |
/// | op_i64_load32_u       |         |
/// | op_i64_store          |         |
/// | op_i64_store8         |         |
/// | op_i64_store16        |         |
/// | op_i64_store32        |         |
/// | op_i64_const          |     +   |
/// | op_memory_grow_checked|         |
/// | op_i64_mul            |         |
/// | op_i64_rem_s          |         |
/// | op_i64_rem_u          |         |
/// |-----------------------|---------|
use rwasm::{
    CallStack, CompilationConfig, ExecutionEngine, InstructionSet, RwasmExecutor, RwasmModule,
    RwasmStore, TrapCode, Value, ValueStack,
};
use std::{
    fmt::Debug,
    ops::{BitAnd, BitOr, BitXor, Shl, Shr},
};

fn run_vm_instr(mut is: InstructionSet, inputs: Vec<u32>) -> Result<(Vec<u32>, u32), TrapCode> {
    is.op_return();
    let rwasm_module = RwasmModule::with_one_function(is);
    let mut value_stack = ValueStack::default();
    assert_eq!(value_stack.max_stack_height(), 0);
    value_stack.reserve(10)?;
    let mut call_stack = CallStack::default();
    let mut store = RwasmStore::<()>::default();
    let inputs_len = inputs.len();
    for i in inputs {
        value_stack.push(i.into());
    }
    let mut executor =
        RwasmExecutor::entrypoint(&rwasm_module, &mut value_stack, &mut call_stack, &mut store);
    executor.run_with_stack_check()?;
    let output = value_stack
        .as_slice()
        .iter()
        .map(|v| v.as_u32())
        .collect::<Vec<_>>();
    let msh = value_stack.max_stack_height() - inputs_len;
    Ok((output, msh as u32))
}

fn run_binary_test_case(
    is: &InstructionSet,
    a: u64,
    b: u64,
    c: u64,
    msh_allowed: u32,
) -> Result<(), TrapCode> {
    let (output, max_stack_height) = run_vm_instr(
        is.clone(),
        vec![a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32],
    )?;
    assert_eq!(output.len(), 2);
    let r = (output[1] as u64) << 32 | output[0] as u64;
    assert_eq!(c, r, "f({a}, {b})={r}, but expected {c}");
    assert!(
        max_stack_height <= msh_allowed,
        "MSH: {max_stack_height} <= {msh_allowed}"
    );
    Ok(())
}

fn run_unary_test_case(
    is: &InstructionSet,
    a: u64,
    c: u64,
    msh_allowed: u32,
) -> Result<(), TrapCode> {
    let (output, max_stack_height) = run_vm_instr(is.clone(), vec![a as u32, (a >> 32) as u32])?;
    assert_eq!(output.len(), 2);
    let r = (output[1] as u64) << 32 | output[0] as u64;
    assert_eq!(c, r);
    assert!(
        max_stack_height <= msh_allowed,
        "MSH: {max_stack_height} <= {msh_allowed}"
    );
    Ok(())
}

fn run_cmp_test_case(
    is: &InstructionSet,
    a: u64,
    c: u32,
    msh_allowed: u32,
) -> Result<(), TrapCode> {
    let (output, max_stack_height) = run_vm_instr(is.clone(), vec![a as u32, (a >> 32) as u32])?;
    assert_eq!(output.len(), 1);
    let r = output[0];
    assert_eq!(c, r);
    assert!(
        max_stack_height <= msh_allowed,
        "MSH: {max_stack_height} <= {msh_allowed}"
    );
    Ok(())
}

#[test]
fn test_i64_const() {
    let test_case_u64 = |a: i64| {
        let mut is = InstructionSet::new();
        is.op_i64_const(a);
        let (output, max_stack_height) = run_vm_instr(is.clone(), vec![]).unwrap();
        assert_eq!(output.len(), 2);
        let r = (output[1] as u64) << 32 | output[0] as u64;
        assert_eq!(a, r as i64);
        assert!(max_stack_height <= InstructionSet::MSH_I64_CONST);
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
}

#[test]
fn test_i64_mul() {
    let mut is = InstructionSet::new();
    is.op_i64_mul();

    let test_case_u64 = |a: u64, b: u64| {
        let c = a.wrapping_mul(b);
        run_binary_test_case(&is, a, b, c, InstructionSet::MSH_I64_MUL).unwrap();
    };

    let test_case_i64 = |a: i64, b: i64| {
        let c = a.wrapping_mul(b);
        run_binary_test_case(
            &is,
            a as u64,
            b as u64,
            c as u64,
            InstructionSet::MSH_I64_MUL,
        )
        .unwrap();
    };

    // u64 test cases
    test_case_u64(15, 71);
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

    pairwise_fuzzing_test(test_case_u64, generate_random_numbers(30));
    pairwise_fuzzing_test(
        test_case_i64,
        generate_random_numbers(30).iter().map(|v| *v as i64),
    );
}

#[test]
fn test_i64_eqz() {
    let mut is = InstructionSet::new();
    is.op_i64_eqz();

    let test_case_u64 = |a: u64| {
        let c = (a == 0) as u32;
        run_cmp_test_case(&is, a, c, InstructionSet::MSH_I64_EQZ).unwrap();
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
}

#[test]
fn test_i64_sub() {
    let mut is = InstructionSet::new();
    is.op_i64_sub();

    let test_case_u64 = |a: u64, b: u64| {
        let c = a.wrapping_sub(b);
        run_binary_test_case(&is, a, b, c, InstructionSet::MSH_I64_SUB).unwrap();
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
}

#[test]
fn test_i64_le_u() {
    let mut is = InstructionSet::new();
    is.op_i64_le_u();

    let test_case_u64 = |a: u64, b: u64| {
        let c = (a <= b) as u64;
        let (output, msh) = run_vm_instr(
            is.clone(),
            vec![a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32],
        )
        .unwrap();
        assert_eq!(output.len(), 1);
        let r = output[0] as u64;
        assert_eq!(c, r);
        assert!(msh <= InstructionSet::MSH_I64_LE_U);
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
}

#[test]
fn test_i64_add() {
    let mut is = InstructionSet::new();
    is.op_i64_add();

    let test_case_u64 = |a: u64, b: u64| {
        let c = a.wrapping_add(b);
        run_binary_test_case(&is, a, b, c, InstructionSet::MSH_I64_ADD).unwrap();
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

    pairwise_fuzzing_test(test_case_u64, generate_random_numbers(30));
}

#[test]
fn test_i64_div_s() {
    let mut is = InstructionSet::new();
    is.op_i64_div_s();

    let test_case_i64 = |a: i64, b: i64| {
        let c = a.wrapping_div(b);
        run_binary_test_case(
            &is,
            a as u64,
            b as u64,
            c as u64,
            InstructionSet::MSH_I64_DIV_S,
        )
        .unwrap();
    };
    let test_case_i64_trap = |a: i64, b: i64, trap_code: TrapCode| {
        assert_eq!(
            run_binary_test_case(
                &is,
                a as u64,
                b as u64,
                u64::MAX,
                InstructionSet::MSH_I64_DIV_S,
            )
            .unwrap_err(),
            trap_code
        );
    };

    test_case_i64_trap(0, 0, TrapCode::IntegerDivisionByZero);
    test_case_i64_trap(1, 0, TrapCode::IntegerDivisionByZero);
    test_case_i64_trap(-1, 0, TrapCode::IntegerDivisionByZero);
    test_case_i64_trap(i64::MAX, 0, TrapCode::IntegerDivisionByZero);
    test_case_i64_trap(i64::MIN, 0, TrapCode::IntegerDivisionByZero);
    test_case_i64_trap(i64::MIN, -1, TrapCode::IntegerOverflow);
    test_case_i64(10, 2);
    test_case_i64(-10, 2);
    test_case_i64(10, -2);
    test_case_i64(-10, -2);
    test_case_i64(0, 1);
    test_case_i64(1, 1);
    test_case_i64(-1, 1);
    test_case_i64(1, -1);
    test_case_i64(-1, -1);
    test_case_i64(i64::MAX, 1);
    test_case_i64(i64::MAX, -1);
    test_case_i64(i64::MIN, 1);
    test_case_i64(i64::MIN, 2);
    test_case_i64(i64::MIN, i64::MAX);
    test_case_i64(123, -3);
    test_case_i64(-123, 3);
    test_case_i64(-123, -3);
    test_case_i64(i64::MIN + 1, -1);
    test_case_i64(i64::MIN + 1, 1);
    test_case_i64(1, 2);
    test_case_i64(-1, 2);
    test_case_i64(1, -2);
    test_case_i64(-1, -2);
    test_case_i64(i64::MAX, 2);
    test_case_i64(-100, 7);
    test_case_i64(100, -7);
    test_case_i64(i64::MIN, i64::MIN);
    test_case_i64(i64::MAX, i64::MAX);
}

#[test]
fn test_i64_div_u() {
    let mut is = InstructionSet::new();
    is.op_i64_div_u();

    let test_case_i64 = |a: u64, b: u64| {
        let c = a.wrapping_div(b);
        run_binary_test_case(&is, a, b, c, InstructionSet::MSH_I64_DIV_U).unwrap();
    };
    let test_case_i64_trap = |a: u64, b: u64, trap_code: TrapCode| {
        assert_eq!(
            run_binary_test_case(&is, a, b, u64::MAX, InstructionSet::MSH_I64_DIV_U).unwrap_err(),
            trap_code
        );
    };

    test_case_i64(15602808788219557311, 9181438499313657906);
    test_case_i64_trap(0u64, 0u64, TrapCode::IntegerDivisionByZero);
    test_case_i64_trap(1, 0, TrapCode::IntegerDivisionByZero);
    test_case_i64_trap(u64::MAX, 0, TrapCode::IntegerDivisionByZero);
    test_case_i64(0, 1);
    test_case_i64(1, 1);
    test_case_i64(1, u64::MAX);
    test_case_i64(u64::MAX, 1);
    test_case_i64(u64::MAX, u64::MAX);
    test_case_i64(u64::MAX - 1, u64::MAX);
    test_case_i64(u64::MAX, 2);
    test_case_i64(2, u64::MAX);
    test_case_i64(2, 1);
    test_case_i64(12345678901234567890, 1234567890);
    test_case_i64(100, 10);
    test_case_i64(10, 100);
    test_case_i64(0xFFFF_FFFF_0000_0000, 0xFFFF_FFFF);
    test_case_i64(0x8000_0000_0000_0000, 2);
    test_case_i64(0x0000_0001_0000_0000, 0x100);
}

#[test]
fn test_i64_rem_s() {
    let mut is = InstructionSet::new();
    is.op_i64_rem_s();

    let test_case_i64 = |a: i64, b: i64| {
        let c = a.wrapping_rem(b);
        run_binary_test_case(
            &is,
            a as u64,
            b as u64,
            c as u64,
            InstructionSet::MSH_I64_REM_S,
        )
        .unwrap();
    };
    let test_case_i64_trap = |a: i64, b: i64, trap_code: TrapCode| {
        assert_eq!(
            run_binary_test_case(
                &is,
                a as u64,
                b as u64,
                u64::MAX,
                InstructionSet::MSH_I64_REM_S
            )
            .unwrap_err(),
            trap_code
        );
    };

    test_case_i64(0i64, 1i64);
    test_case_i64(1, 1);
    test_case_i64(-1, 1);
    test_case_i64(1, -1);
    test_case_i64(-1, -1);
    test_case_i64(5, 2);
    test_case_i64(5, -2);
    test_case_i64(-5, 2);
    test_case_i64(-5, -2);
    test_case_i64(i64::MAX, 2);
    test_case_i64(i64::MIN, 2);
    test_case_i64(i64::MIN, -1); // a special: Rust defines MIN % -1 = ;
    test_case_i64(i64::MIN, i64::MAX);
    test_case_i64_trap(0, 0, TrapCode::IntegerDivisionByZero);
    test_case_i64_trap(1, 0, TrapCode::IntegerDivisionByZero);
    test_case_i64_trap(-1, 0, TrapCode::IntegerDivisionByZero);
}

#[test]
fn test_i64_rem_u() {
    let mut is = InstructionSet::new();
    is.op_i64_rem_u();

    let test_case_i64 = |a: u64, b: u64| {
        let c = a.wrapping_rem(b);
        run_binary_test_case(&is, a, b, c, InstructionSet::MSH_I64_REM_U).unwrap();
    };
    let test_case_i64_trap = |a: u64, b: u64, trap_code: TrapCode| {
        assert_eq!(
            run_binary_test_case(&is, a, b, u64::MAX, InstructionSet::MSH_I64_REM_U).unwrap_err(),
            trap_code
        );
    };

    test_case_i64(0u64, 1u64);
    test_case_i64(1, 1);
    test_case_i64(1, u64::MAX);
    test_case_i64(u64::MAX, 1);
    test_case_i64(u64::MAX, u64::MAX);
    test_case_i64(u64::MAX - 1, u64::MAX);
    test_case_i64(12345678901234567890, 1234567890);
    test_case_i64(100, 10);
    test_case_i64(101, 10);
    test_case_i64(10, 100);
    test_case_i64(0xFFFF_FFFF_0000_0000, 0xFFFF_FFFF);
    test_case_i64(0x8000_0000_0000_0000, 2);
    test_case_i64(0x8000_0000_0000_0001, 2);
    test_case_i64(0x0000_0001_0000_0000, 0x100);
    test_case_i64_trap(0, 0, TrapCode::IntegerDivisionByZero);
    test_case_i64_trap(1, 0, TrapCode::IntegerDivisionByZero);
    test_case_i64_trap(u64::MAX, 0, TrapCode::IntegerDivisionByZero);
}

#[test]
fn test_i64_shr_u() {
    let mut is = InstructionSet::new();
    is.op_i64_shr_u();

    let test_case_u64 = |a: u64, b: u64| {
        let c = a.shr(b & 0x3F);
        run_binary_test_case(&is, a, b, c, InstructionSet::MSH_I64_SHR_U).unwrap();
    };

    // 0 shifted by any amount
    test_case_u64(0, 0);
    test_case_u64(0, 1);
    test_case_u64(0, 63);
    // Shift by 0 does nothing
    test_case_u64(0x123456789abcdef0, 0);
    // normal right shifts
    test_case_u64(0b1000, 3);
    test_case_u64(0b1111, 2);
    test_case_u64(0x8000000000000000, 63);
    // all ones, shifts
    test_case_u64(u64::MAX, 1);
    test_case_u64(u64::MAX, 63);
    // shift amount uses only low 6 bits (modulo 64)
    test_case_u64(0xFFFFFFFFFFFFFFFF, 64); // shift 0
    test_case_u64(0x8000000000000000, 64); // shift 0
    test_case_u64(0xFFFFFFFFFFFFFFFF, 65); // shift 1
    test_case_u64(0x8000000000000000, 127);
    // additional patterns
    test_case_u64(0x123456789abcdef0, 6);
    test_case_u64(0x123456789abcdef0, 70); // shift 70 == shift 6
}

#[test]
fn test_i64_shr_s() {
    let mut is = InstructionSet::new();
    is.op_i64_shr_s();

    let test_case_i64 = |a: i64, b: i64| {
        let c = a.shr(b & 0x3F);
        run_binary_test_case(
            &is,
            a as u64,
            b as u64,
            c as u64,
            InstructionSet::MSH_I64_SHR_S,
        )
        .unwrap();
    };

    // no shift, value unchanged
    test_case_i64(0x0000000000000001, 0);
    test_case_i64(0x7FFFFFFFFFFFFFFF, 0);
    test_case_i64(-1, 0);
    test_case_i64(i64::MIN, 0);
    // shift 1, positive/negative
    test_case_i64(0x0000000000000002, 1); // 2 >> 1 = 1
    test_case_i64(-2, 1); // -2 >> 1 = -1
    test_case_i64(0x7FFFFFFFFFFFFFFF, 1); // max i64 >> 1
    test_case_i64(i64::MIN, 1); // min i64 >> 1 (remains negative, top bit stays 1)
    test_case_i64(-1, 1);
    // shift by 31
    test_case_i64(0x7FFFFFFF00000000, 31);
    test_case_i64(-0x80000000, 31);
    test_case_i64(-1, 31);
    // shift by 32
    test_case_i64(0x7FFFFFFF00000000, 32);
    test_case_i64(0x8000000000000000u64 as i64, 32);
    test_case_i64(-1, 32);
    // shift by 33
    test_case_i64(0x7FFFFFFF00000000, 33);
    test_case_i64(0x8000000000000000u64 as i64, 33);
    test_case_i64(-1, 33);
    // shift by 63
    test_case_i64(1, 63); // only the lowest bit, a result is 0
    test_case_i64(-1, 63); // -1 >> 63 = -1 (all bits 1)
    test_case_i64(i64::MIN, 63);
    // shift by more than 63 (masked to 0-63)
    test_case_i64(0x4000000000000000, 64); // treated as shift 0 (identity)
    test_case_i64(-123456789, 128);
    // random bit patterns
    test_case_i64(0xAAAAAAAAAAAAAAAAu64 as i64, 4);
    test_case_i64(0x5555555555555555, 4);
    test_case_i64(0x123456789ABCDEF0, 8);
    test_case_i64(-0x123456789ABCDEF, 8);
    // zero shifted any amount is zero
    test_case_i64(0, 1);
    test_case_i64(0, 63);
    test_case_i64(0, 64);
    // single sign bit
    test_case_i64(0x8000000000000000u64 as i64, 1);
    test_case_i64(0x8000000000000000u64 as i64, 63);
}

#[test]
fn test_i64_shl() {
    let mut is = InstructionSet::new();
    is.op_i64_shl();

    let test_case_u64 = |a: u64, b: u64| {
        let c = a.shl(b & 0x3F);
        run_binary_test_case(&is, a, b, c, InstructionSet::MSH_I64_SHL).unwrap();
    };

    // no shift: identity
    test_case_u64(0x0000000000000001, 0);
    test_case_u64(0x123456789ABCDEF0, 0);
    // small shifts
    test_case_u64(0x0000000000000001, 1); // 1 << 1 = 2
    test_case_u64(0x0000000000000001, 2); // 1 << 2 = 4
    test_case_u64(0x0000000100000000, 8);
    // high-bit crossing: 1 shifted left 63 becomes the highest bit
    test_case_u64(0x0000000000000001, 63);
    // shift by 32: hi becomes lo, lo becomes 0
    test_case_u64(0x0000000100000001, 32);
    test_case_u64(0xFFFFFFFF00000000, 32);
    test_case_u64(0x00000000FFFFFFFF, 32);
    // shifts > 32, bits only in lo part matter
    test_case_u64(0x0000000000000001, 33);
    test_case_u64(0x00000000FFFFFFFF, 40);
    // shift full 64: always zero
    test_case_u64(0xFFFFFFFFFFFFFFFF, 64);
    // shift by more than 63 (masked): should behave like shift % 64
    test_case_u64(0x0000000000000001, 65);
    test_case_u64(0x0000000000000001, 128);
    // patterned bits
    test_case_u64(0xAAAAAAAAAAAAAAAA, 1);
    test_case_u64(0x5555555555555555, 1);
    test_case_u64(0x8000000000000000, 1);
    // all ones, various shifts
    test_case_u64(0xFFFFFFFFFFFFFFFF, 1);
    test_case_u64(0xFFFFFFFFFFFFFFFF, 32);
    test_case_u64(0xFFFFFFFFFFFFFFFF, 63);
    // zero, any shift is zero
    test_case_u64(0x0000000000000000, 5);
    test_case_u64(0x0000000000000000, 32);
    test_case_u64(0x0000000000000000, 63);
    // alternating nibbles, various shifts
    test_case_u64(0x0F0F0F0F0F0F0F0F, 4);
    test_case_u64(0xF0F0F0F0F0F0F0F0, 4);
    // random
    test_case_u64(0x123456789ABCDEF0, 8);
    test_case_u64(0x7FFFFFFFFFFFFFFF, 1);
}

#[test]
fn test_i64_clz() {
    let mut is = InstructionSet::new();
    is.op_i64_clz();

    let test_case_u64 = |a: u64| {
        let c = a.leading_zeros() as u64;
        run_unary_test_case(&is, a, c, InstructionSet::MSH_I64_CLZ).unwrap();
    };

    test_case_u64(0x00000000_00000000);
    test_case_u64(0x00000000_00000001);
    test_case_u64(0x80000000_00000000);
    test_case_u64(0x00000001_00000000);
    test_case_u64(0x00000000_FFFFFFFF);
    test_case_u64(0xFFFFFFFF_00000000);
    test_case_u64(0x0000FFFF_FFFFFFFF);
    test_case_u64(0x00000000_80000000);
    test_case_u64(0x7FFFFFFF_FFFFFFFF);
    test_case_u64(0x00FF0000_00000000);
    test_case_u64(0x00000000_00008000);
}

#[test]
fn test_i64_ctz() {
    let mut is = InstructionSet::new();
    is.op_i64_ctz();

    let test_case_u64 = |a: u64| {
        let c = a.trailing_zeros() as u64;
        run_unary_test_case(&is, a, c, InstructionSet::MSH_I64_CTZ).unwrap();
    };

    test_case_u64(0x00000000_00000000);
    test_case_u64(0x00000000_00000001);
    test_case_u64(0x80000000_00000000);
    test_case_u64(0x00000001_00000000);
    test_case_u64(0x00000000_FFFFFFFF);
    test_case_u64(0xFFFFFFFF_00000000);
    test_case_u64(0x0000FFFF_FFFFFFFF);
    test_case_u64(0x00000000_80000000);
    test_case_u64(0x7FFFFFFF_FFFFFFFF);
    test_case_u64(0x00FF0000_00000000);
    test_case_u64(0x00000000_00008000);
}

#[test]
fn test_i64_popcnt() {
    let mut is = InstructionSet::new();
    is.op_i64_popcnt();

    let test_case_u64 = |a: u64| {
        let c = a.count_ones() as u64;
        run_unary_test_case(&is, a, c, InstructionSet::MSH_I64_POPCNT).unwrap();
    };

    test_case_u64(0x12345678_9ABCDEF0);
    test_case_u64(0x00000000_00000000); // all zeros
    test_case_u64(0xFFFFFFFF_FFFFFFFF); // all ones
    test_case_u64(0x80000000_00000000); // high bit only
    test_case_u64(0x00000000_00000001); // low bit only
    test_case_u64(0x00000000_00000000);
    test_case_u64(0x00000000_00000001);
    test_case_u64(0x80000000_00000000);
    test_case_u64(0x00000001_00000000);
    test_case_u64(0x00000000_FFFFFFFF);
    test_case_u64(0xFFFFFFFF_00000000);
    test_case_u64(0x0000FFFF_FFFFFFFF);
    test_case_u64(0x00000000_80000000);
    test_case_u64(0x7FFFFFFF_FFFFFFFF);
    test_case_u64(0x00FF0000_00000000);
    test_case_u64(0x00000000_00008000);
}

#[test]
fn test_i64_and() {
    let mut is = InstructionSet::new();
    is.op_i64_and();

    let test_case_u64 = |a: u64, b: u64| {
        let c = a.bitand(b);
        run_binary_test_case(&is, a, b, c, InstructionSet::MSH_I64_AND).unwrap();
    };

    // zero and anything are zero
    test_case_u64(0x0000000000000000, 0xFFFFFFFFFFFFFFFF);
    test_case_u64(0xFFFFFFFFFFFFFFFF, 0x0000000000000000);
    // all ones and anything are the value itself
    test_case_u64(0xFFFFFFFFFFFFFFFF, 0x123456789ABCDEF0);
    test_case_u64(0x123456789ABCDEF0, 0xFFFFFFFFFFFFFFFF);
    // high bit only
    test_case_u64(0x8000000000000000, 0x8000000000000000);
    test_case_u64(0x8000000000000000, 0x7FFFFFFFFFFFFFFF);
    // low bit only
    test_case_u64(0x0000000000000001, 0x0000000000000001);
    test_case_u64(0x0000000000000001, 0xFFFFFFFFFFFFFFFE);
    // alternating bits
    test_case_u64(0xAAAAAAAAAAAAAAAA, 0x5555555555555555);
    test_case_u64(0xAAAAAAAAAAAAAAAA, 0xAAAAAAAAAAAAAAAA);
    test_case_u64(0x5555555555555555, 0x5555555555555555);
    // every nibble is half-set
    test_case_u64(0x0F0F0F0F0F0F0F0F, 0xF0F0F0F0F0F0F0F0);
    // random pattern
    test_case_u64(0x123456789ABCDEF0, 0x0FEDCBA987654321);
    // low and high halves
    test_case_u64(0xFFFFFFFF00000000, 0x00000000FFFFFFFF);
    test_case_u64(0x00000000FFFFFFFF, 0xFFFFFFFF00000000);
    // overlapping bits
    test_case_u64(0x0000FFFF0000FFFF, 0xFFFF0000FFFF0000);
    // 1, 2, 4, 8 pattern
    test_case_u64(0x000000000000000F, 0x0000000000000005);
    // single bit
    test_case_u64(0x0000000000000002, 0x0000000000000004);
    // large numbers, almost all bits
    test_case_u64(0xFFFFFFFFFFFFFFFE, 0xFFFFFFFFFFFFFFFD);
    // mix signedness
    test_case_u64(0xFFFFFFFFFFFFFFFF, 0x8000000000000000);
    test_case_u64(0x7FFFFFFFFFFFFFFF, 0x8000000000000000);
}

#[test]
fn test_i64_or() {
    let mut is = InstructionSet::new();
    is.op_i64_or();

    let test_case_u64 = |a: u64, b: u64| {
        let c = a.bitor(b);
        run_binary_test_case(&is, a, b, c, InstructionSet::MSH_I64_OR).unwrap();
    };

    // zero and anything are zero
    test_case_u64(0x0000000000000000, 0xFFFFFFFFFFFFFFFF);
    test_case_u64(0xFFFFFFFFFFFFFFFF, 0x0000000000000000);
    // all ones and anything are the value itself
    test_case_u64(0xFFFFFFFFFFFFFFFF, 0x123456789ABCDEF0);
    test_case_u64(0x123456789ABCDEF0, 0xFFFFFFFFFFFFFFFF);
    // high bit only
    test_case_u64(0x8000000000000000, 0x8000000000000000);
    test_case_u64(0x8000000000000000, 0x7FFFFFFFFFFFFFFF);
    // low bit only
    test_case_u64(0x0000000000000001, 0x0000000000000001);
    test_case_u64(0x0000000000000001, 0xFFFFFFFFFFFFFFFE);
    // alternating bits
    test_case_u64(0xAAAAAAAAAAAAAAAA, 0x5555555555555555);
    test_case_u64(0xAAAAAAAAAAAAAAAA, 0xAAAAAAAAAAAAAAAA);
    test_case_u64(0x5555555555555555, 0x5555555555555555);
    // every nibble is half-set
    test_case_u64(0x0F0F0F0F0F0F0F0F, 0xF0F0F0F0F0F0F0F0);
    // random pattern
    test_case_u64(0x123456789ABCDEF0, 0x0FEDCBA987654321);
    // low and high halves
    test_case_u64(0xFFFFFFFF00000000, 0x00000000FFFFFFFF);
    test_case_u64(0x00000000FFFFFFFF, 0xFFFFFFFF00000000);
    // overlapping bits
    test_case_u64(0x0000FFFF0000FFFF, 0xFFFF0000FFFF0000);
    // 1, 2, 4, 8 pattern
    test_case_u64(0x000000000000000F, 0x0000000000000005);
    // single bit
    test_case_u64(0x0000000000000002, 0x0000000000000004);
    // large numbers, almost all bits
    test_case_u64(0xFFFFFFFFFFFFFFFE, 0xFFFFFFFFFFFFFFFD);
    // mix signedness
    test_case_u64(0xFFFFFFFFFFFFFFFF, 0x8000000000000000);
    test_case_u64(0x7FFFFFFFFFFFFFFF, 0x8000000000000000);
}

#[test]
fn test_i64_xor() {
    let mut is = InstructionSet::new();
    is.op_i64_xor();

    let test_case_u64 = |a: u64, b: u64| {
        let c = a.bitxor(b);
        run_binary_test_case(&is, a, b, c, InstructionSet::MSH_I64_XOR).unwrap();
    };

    // zero and anything are zero
    test_case_u64(0x0000000000000000, 0xFFFFFFFFFFFFFFFF);
    test_case_u64(0xFFFFFFFFFFFFFFFF, 0x0000000000000000);
    // all ones and anything are the value itself
    test_case_u64(0xFFFFFFFFFFFFFFFF, 0x123456789ABCDEF0);
    test_case_u64(0x123456789ABCDEF0, 0xFFFFFFFFFFFFFFFF);
    // high bit only
    test_case_u64(0x8000000000000000, 0x8000000000000000);
    test_case_u64(0x8000000000000000, 0x7FFFFFFFFFFFFFFF);
    // low bit only
    test_case_u64(0x0000000000000001, 0x0000000000000001);
    test_case_u64(0x0000000000000001, 0xFFFFFFFFFFFFFFFE);
    // alternating bits
    test_case_u64(0xAAAAAAAAAAAAAAAA, 0x5555555555555555);
    test_case_u64(0xAAAAAAAAAAAAAAAA, 0xAAAAAAAAAAAAAAAA);
    test_case_u64(0x5555555555555555, 0x5555555555555555);
    // every nibble is half-set
    test_case_u64(0x0F0F0F0F0F0F0F0F, 0xF0F0F0F0F0F0F0F0);
    // random pattern
    test_case_u64(0x123456789ABCDEF0, 0x0FEDCBA987654321);
    // low and high halves
    test_case_u64(0xFFFFFFFF00000000, 0x00000000FFFFFFFF);
    test_case_u64(0x00000000FFFFFFFF, 0xFFFFFFFF00000000);
    // overlapping bits
    test_case_u64(0x0000FFFF0000FFFF, 0xFFFF0000FFFF0000);
    // 1, 2, 4, 8 pattern
    test_case_u64(0x000000000000000F, 0x0000000000000005);
    // single bit
    test_case_u64(0x0000000000000002, 0x0000000000000004);
    // large numbers, almost all bits
    test_case_u64(0xFFFFFFFFFFFFFFFE, 0xFFFFFFFFFFFFFFFD);
    // mix signedness
    test_case_u64(0xFFFFFFFFFFFFFFFF, 0x8000000000000000);
    test_case_u64(0x7FFFFFFFFFFFFFFF, 0x8000000000000000);
}

#[test]
fn test_i64_rotl() {
    let mut is = InstructionSet::new();
    is.op_i64_rotl();

    let test_case_u64 = |a: u64, b: u64| {
        let c = a.rotate_left(u32::try_from(b).unwrap());
        run_binary_test_case(&is, a, b, c, InstructionSet::MSH_I64_ROTL).unwrap();
    };

    // No rotation: value unchanged
    test_case_u64(0x0000000000000001, 0);
    test_case_u64(0x8000000000000000, 0);
    // rotating by 64 is an identity (Wasm shift amount is masked)
    test_case_u64(0x123456789ABCDEF0, 64);
    test_case_u64(0x123456789ABCDEF0, 128);
    // shift by 1: lowest bit becomes second, the highest bit becomes lowest
    test_case_u64(0x0000000000000001, 1);
    test_case_u64(0x8000000000000000, 1);
    // patterned bits, rotation of 4
    test_case_u64(0x0F0F0F0F0F0F0F0F, 4);
    test_case_u64(0xF0F0F0F0F0F0F0F0, 4);
    // all ones: always all ones
    test_case_u64(0xFFFFFFFFFFFFFFFF, 13);
    test_case_u64(0xFFFFFFFFFFFFFFFF, 63);
    // alternating pattern
    test_case_u64(0xAAAAAAAAAAAAAAAA, 1);
    test_case_u64(0x5555555555555555, 1);
    // high-bit set, rotate into lower bits
    test_case_u64(0x8000000000000000, 4);
    test_case_u64(0x0000000000000001, 63);
    // rotation by 32: upper and lower halves swap
    test_case_u64(0xDEADBEEF12345678, 32);
    test_case_u64(0x00000000FFFFFFFF, 32);
    test_case_u64(0xFFFFFFFF00000000, 32);
    // random value, various shifts
    test_case_u64(0x123456789ABCDEF0, 1);
    test_case_u64(0x123456789ABCDEF0, 8);
    test_case_u64(0x123456789ABCDEF0, 60);
    // zero: always zero
    test_case_u64(0x0000000000000000, 7);
    test_case_u64(0x0000000000000000, 63);
}

#[test]
fn test_i64_rotr() {
    let mut is = InstructionSet::new();
    is.op_i64_rotr();

    let test_case_u64 = |a: u64, b: u64| {
        let c = a.rotate_right(u32::try_from(b).unwrap());
        run_binary_test_case(&is, a, b, c, InstructionSet::MSH_I64_ROTR).unwrap();
    };

    // No rotation: value unchanged
    test_case_u64(0x0000000000000001, 0);
    test_case_u64(0x8000000000000000, 0);
    // rotating by 64 is an identity (Wasm shift amount is masked)
    test_case_u64(0x123456789ABCDEF0, 64);
    test_case_u64(0x123456789ABCDEF0, 128);
    // shift by 1: lowest bit becomes second, the highest bit becomes lowest
    test_case_u64(0x0000000000000001, 1);
    test_case_u64(0x8000000000000000, 1);
    // patterned bits, rotation of 4
    test_case_u64(0x0F0F0F0F0F0F0F0F, 4);
    test_case_u64(0xF0F0F0F0F0F0F0F0, 4);
    // all ones: always all ones
    test_case_u64(0xFFFFFFFFFFFFFFFF, 13);
    test_case_u64(0xFFFFFFFFFFFFFFFF, 63);
    // alternating pattern
    test_case_u64(0xAAAAAAAAAAAAAAAA, 1);
    test_case_u64(0x5555555555555555, 1);
    // high-bit set, rotate into lower bits
    test_case_u64(0x8000000000000000, 4);
    test_case_u64(0x0000000000000001, 63);
    // rotation by 32: upper and lower halves swap
    test_case_u64(0xDEADBEEF12345678, 32);
    test_case_u64(0x00000000FFFFFFFF, 32);
    test_case_u64(0xFFFFFFFF00000000, 32);
    // random value, various shifts
    test_case_u64(0x123456789ABCDEF0, 1);
    test_case_u64(0x123456789ABCDEF0, 8);
    test_case_u64(0x123456789ABCDEF0, 60);
    // zero: always zero
    test_case_u64(0x0000000000000000, 7);
    test_case_u64(0x0000000000000000, 63);
}

#[test]
fn test_i64_extend_i32_s() {
    let mut is = InstructionSet::new();
    is.op_i64_extend_i32_s();
    let test_case = |a: i32, c_lo: i32, c_hi: i32| {
        let (output, msh) = run_vm_instr(is.clone(), vec![a as u32]).unwrap();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0], c_lo as u32);
        assert_eq!(output[1], c_hi as u32);
        assert!(
            msh <= InstructionSet::MSH_I64_EXTEND_I32_S,
            "MSH: {msh} <= {}",
            InstructionSet::MSH_I64_EXTEND_I32_S
        )
    };
    // simple cases
    test_case(0, 0, 0);
    test_case(1, 1, 0);
    test_case(42, 42, 0);
    test_case(-1, -1, -1);
    test_case(-42, -42, -1);
    // 0x80000000, high should be -1
    test_case(i32::MIN, i32::MIN, -1);
    // 0x7FFFFFFF, positive
    test_case(i32::MAX, i32::MAX, 0);
    // 255
    test_case(0x000000FF, 0xFF, 0);
    // -128 in 2's complement
    test_case(0xFFFFFF80u32 as i32, -128, -1);
}

#[test]
fn test_i64_extend8_s() {
    let mut is = InstructionSet::new();
    is.op_i64_extend8_s();
    let test_case = |a: i32, c_lo: i32, c_hi: i32| {
        let (output, msh) = run_vm_instr(is.clone(), vec![a as u32 & 0xff, 0]).unwrap();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0], c_lo as u32);
        assert_eq!(output[1], c_hi as u32);
        assert!(msh <= InstructionSet::MSH_I64_EXTEND8_S)
    };
    test_case(0x00, 0x00, 0); // 0 → [0, 0]
    test_case(0x01, 0x01, 0); // 1 → [1, 0]
    test_case(0x7F, 0x7F, 0); // 127 → [127, 0]
    test_case(0x80, -128, -1); // -128 → [0xFFFFFF80, -1]
    test_case(0xFF, -1, -1); // -1 → [0xFFFFFFFF, -1]
    test_case(0xA5, -91, -1); // -91 → [0xFFFFFFA5, -1]
    test_case(0x1234, 0x34, 0); // truncated to 0x34 → [52, 0]
    test_case(-1, -1, -1); // -1 → [0xFFFFFFFF, -1]
    test_case(255, -1, -1); // 255 interpreted as 0xFF → [-1, -1]
}

#[test]
fn test_i64_extend16_s() {
    let mut is = InstructionSet::new();
    is.op_i64_extend16_s();
    let test_case = |a: i32, c_lo: i32, c_hi: i32| {
        let (output, msh) = run_vm_instr(is.clone(), vec![a as u32 & 0xffff, 0]).unwrap();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0], c_lo as u32);
        assert_eq!(output[1], c_hi as u32);
        assert!(msh <= InstructionSet::MSH_I64_EXTEND16_S)
    };
    test_case(0x0000, 0x0000, 0); // 0 → [0, 0]
    test_case(0x0001, 0x0001, 0); // 1 → [1, 0]
    test_case(0x7FFF, 0x7FFF, 0); // 32767 → [32767, 0]
    test_case(0x8000, -32768, -1); // -32768 → [0xFFFF8000, -1]
    test_case(0xFFFF, -1, -1); // -1 → [0xFFFFFFFF, -1]
    test_case(0xABCD, -21555, -1); // -21555 → [0xFFFFABCD, -1]
    test_case(0x123456, 0x3456, 0); // truncate to 16-bit → [0x3456, 0]
    test_case(-1, -1, -1); // -1 stays [-1, -1]
    test_case(65535, -1, -1); // 0xFFFF = -1 in 16-bit → [-1, -1]
}

#[test]
fn test_i64_extend32_s() {
    let mut is = InstructionSet::new();
    is.op_i64_extend32_s();
    let test_case = |a: i64, c_lo: i64, c_hi: i64| {
        let a = a as i32;
        let (output, msh) = run_vm_instr(is.clone(), vec![a as u32, 0]).unwrap();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0], c_lo as i32 as u32);
        assert_eq!(output[1], c_hi as i32 as u32);
        assert!(msh <= InstructionSet::MSH_I64_EXTEND32_S)
    };
    test_case(0x00000000, 0x00000000, 0); // 0 → [0, 0]
    test_case(0x00000001, 0x00000001, 0); // 1 → [1, 0]
    test_case(0x7FFFFFFF, 0x7FFFFFFF, 0); // i32::MAX → [0x7FFFFFFF, 0]
    test_case(0x80000000, 0x80000000, -1); // i32::MIN → [0x80000000, -1]
    test_case(0xFFFFFFFF, -1, -1); // -1 → [-1, -1]
    test_case(0xFFFF0000, -65536, -1); // -65536 → [0xFFFF0000, -1]
    test_case(0x12345678, 0x12345678, 0); // 305419896 → [0x12345678, 0]
    test_case(-42, -42, -1); // -42 → [-42, -1]
}

#[test]
fn test_swap() {
    let mut is = InstructionSet::new();
    is.op_swap();
    let (output, _) = run_vm_instr(is.clone(), vec![100, 200]).unwrap();
    assert_eq!(output.len(), 2);
    assert_eq!(output[0], 200);
    assert_eq!(output[1], 100);
}

/// Generates a set of random numbers near points of interest,
/// such as [0, 1, -1, i32::MIN, i32::MAX, u64::MAX, ...]
fn generate_random_numbers(n: usize) -> Vec<u64> {
    let mut rng = rand::rng();
    const I32_MAX_I64: i64 = i32::MAX as i64;
    const I32_MIN_I64: i64 = i32::MIN as i64;
    const U32_MAX_U64: u64 = u32::MAX as u64;

    let mut v = Vec::new();

    // 1. Very small values
    for k in 0..=5 {
        v.push(k);
        if k != 0 {
            v.push(k * -1);
        }
    }

    // 2. Small values
    for _ in 0..n {
        v.push(rng.random_range(0..1000));
        v.push(rng.random_range(0..1000) * -1);
    }

    // 3. Big random values
    for _ in 0..n {
        v.push(rng.random());
    }

    // 4. Near i32::MAX
    {
        // just below
        let low = I32_MAX_I64 - 1_000;
        let high = I32_MAX_I64 - 2;
        for _ in 0..n / 2 {
            v.push(rng.random_range(low..=high));
        }
        // the “‑1, exact, +1” trio
        v.push(I32_MAX_I64 - 1);
        v.push(I32_MAX_I64);
        v.push(I32_MAX_I64 + 1);
        // just above
        let low = I32_MAX_I64 + 1;
        let high = I32_MAX_I64 + 1_000;
        for _ in 0..n / 2 {
            v.push(rng.random_range(low..=high));
        }
    }

    // 5. Near  i32::MIN
    {
        // just below
        let low = I32_MIN_I64 - 1_000;
        let high = I32_MIN_I64;
        for _ in 0..n / 2 {
            v.push(rng.random_range(low..=high));
        }
        // the “‑1, exact, +1” trio
        v.push(I32_MIN_I64 - 1);
        v.push(I32_MIN_I64);
        v.push(I32_MIN_I64 + 1);
        // just above
        let low = I32_MIN_I64;
        let high = I32_MIN_I64 + 1_000;
        for _ in 0..n / 2 {
            v.push(rng.random_range(low..=high));
        }
    }

    // 6. Near i64::MAX
    {
        let low = i64::MAX - 1_000;
        let high = i64::MAX - 2;
        for _ in 0..n {
            v.push(rng.random_range(low..=high));
        }
        v.push(i64::MAX - 1);
        v.push(i64::MAX); // exact top
        v.push(i64::MAX - 1_000 / 2); // one more mid‑window value
    }

    // 7. Near i64::MIN
    {
        v.push(i64::MIN); // exact bottom
        v.push(i64::MIN + 1); // just above
        let low = i64::MIN + 2;
        let high = i64::MIN + 1_000;
        for _ in 0..n {
            v.push(rng.random_range(low..=high));
        }
    }

    v.push(i64::MAX);
    v.push(i64::MAX - 1);
    v.push(i64::MIN);
    v.push(i64::MIN + 1);

    let mut v: Vec<u64> = v.iter().map(|val| *val as u64).collect();

    // 8. Near u32::MAX
    for _ in 0..n {
        let low: u64 = U32_MAX_U64 - 1_000;
        let high: u64 = U32_MAX_U64;
        v.push(rng.random_range(low..=high));
    }
    v.push(U32_MAX_U64 - 1);
    v.push(U32_MAX_U64);
    v.push(U32_MAX_U64 + 1);
    for _ in 0..n {
        let low = U32_MAX_U64;
        let high: u64 = U32_MAX_U64 + 1_000;
        v.push(rng.random_range(low..=high));
    }

    // 9. Near u64::MAX
    for _ in 0..n {
        let low = u64::MAX - 1_000;
        let high = u64::MAX;
        v.push(rng.random_range(low..=high));
    }

    v.sort_unstable();
    v.dedup(); // remove duplicates

    v
}

fn pairwise_fuzzing_test<T: Clone + Debug, F: Fn(T, T)>(f: F, values: impl IntoIterator<Item = T>) {
    let values: Vec<T> = values.into_iter().collect();
    for a in &values {
        for b in &values {
            f(a.clone(), b.clone());
        }
    }
}

fn run_i64_binary_op(op: &str, a: i64, b: i64, expected: i64) {
    let wat_source = format!(
        r#"
(module
  (func (export "main") (param i64 i64) (result i64)
    local.get 0
    local.get 1
    {op}
  )
)
"#,
        op = op
    );

    let wasm_binary = wat::parse_str(&wat_source).unwrap();

    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);

    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();

    let mut store = RwasmStore::<()>::default();
    let engine = ExecutionEngine::default();
    let mut result = [Value::I64(0); 1];

    let execution_result = engine.execute(
        &mut store,
        &rwasm_module,
        &[Value::I64(a), Value::I64(b)],
        &mut result,
    );
    if !execution_result.is_ok() {
        println!("{:?}", execution_result);
    }

    assert!(
        matches!(execution_result, Ok(_)),
        "Execution failed for {}",
        op
    );
    assert_eq!(
        result[0].i64().unwrap(),
        expected,
        "Mismatch for operation {} with inputs ({}, {})",
        op,
        a,
        b
    );
}

fn run_i64_comparation_op(op: &str, a: i64, b: i64, expected: bool) {
    let wat_source = format!(
        r#"
(module
  (func (export "main") (param i64 i64) (result i32)
    local.get 0
    local.get 1
    {op}
  )
)
"#,
        op = op
    );

    let wasm_binary = wat::parse_str(&wat_source).unwrap();

    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);

    let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();

    let mut store = RwasmStore::<()>::default();
    let engine = ExecutionEngine::default();
    let mut result = [Value::I32(0); 1];

    let execution_result = engine.execute(
        &mut store,
        &rwasm_module,
        &[Value::I64(a), Value::I64(b)],
        &mut result,
    );
    if !execution_result.is_ok() {
        println!("{:?}", execution_result);
    }

    assert!(
        matches!(execution_result, Ok(_)),
        "Execution failed for {}",
        op
    );
    let expected = if expected { 1 } else { 0 };
    assert_eq!(
        result[0].i32().unwrap(),
        expected,
        "Mismatch for operation {} with inputs ({}, {})",
        op,
        a,
        b
    );
}

#[test]
fn test_i64_ops_in_rwasm_interpreter() {
    // Arithmetic
    run_i64_binary_op("i64.add", 10, 732, 10 + 732);
    run_i64_binary_op("i64.sub", 732, 10, 732 - 10);
    run_i64_binary_op("i64.mul", 10, 732, 10 * 732);
    run_i64_binary_op("i64.div_s", 732, 33, 732 / 33);
    run_i64_binary_op("i64.div_u", 732, 33, (732u64 / 33u64) as i64);
    run_i64_binary_op("i64.rem_s", 732, 33, 732 % 33);
    run_i64_binary_op("i64.rem_u", 732, 33, (732u64 % 33u64) as i64);

    // Bitwise
    run_i64_binary_op("i64.and", 0b1101, 0b1011, 0b1001);
    run_i64_binary_op("i64.or", 0b1101, 0b1011, 0b1111);
    run_i64_binary_op("i64.xor", 0b1101, 0b1011, 0b0110);

    // Shifts and rotations
    run_i64_binary_op("i64.shl", 1, 3, 1 << 3);
    run_i64_binary_op("i64.shr_s", -16, 2, -16 >> 2);
    run_i64_binary_op("i64.shr_u", -16, 2, ((-16i64 as u64) >> 2) as i64);
    run_i64_binary_op("i64.rotl", 0x12, 8, 0x12i64.rotate_left(8));
    run_i64_binary_op("i64.rotr", 0x1200, 8, 0x1200i64.rotate_right(8));

    // Comparisons (true = 1, false = 0)
    run_i64_comparation_op("i64.eq", 42, 42, true);
    run_i64_comparation_op("i64.eq", 42, 24, false);
    run_i64_comparation_op("i64.ne", 42, 24, true);
    run_i64_comparation_op("i64.ne", 42, 42, false);
    run_i64_comparation_op("i64.lt_s", -5, 10, true);
    run_i64_comparation_op("i64.lt_s", 10, -5, false);
    run_i64_comparation_op("i64.lt_u", 5, 10, true);
    run_i64_comparation_op("i64.lt_u", 10, 5, false);
    run_i64_comparation_op("i64.gt_s", 10, -5, true);
    run_i64_comparation_op("i64.gt_s", -5, 10, false);
    run_i64_comparation_op("i64.gt_u", 10, 5, true);
    run_i64_comparation_op("i64.gt_u", 5, 10, false);
    run_i64_comparation_op("i64.le_s", -5, -5, true);
    run_i64_comparation_op("i64.le_s", 5, 2, false);
    run_i64_comparation_op("i64.le_u", 5, 5, true);
    run_i64_comparation_op("i64.le_u", 10, 5, false);
    run_i64_comparation_op("i64.ge_s", 5, 5, true);
    run_i64_comparation_op("i64.ge_s", -1, 1, false);
    run_i64_comparation_op("i64.ge_u", 5, 5, true);
    run_i64_comparation_op("i64.ge_u", 5, 10, false);
}
