//! The wide-arithmetic proposal (`i64.add128`, `i64.sub128`, `i64.mul_wide_s`,
//! `i64.mul_wide_u`) on both backends.
//!
//! Each operator lowers to one rwasm opcode that the interpreter executes as one 128-bit
//! operation, and Cranelift lowers to `mul`/`mulh` or add/sub with carry. The tests pin the
//! lowering, check every operator against native 128-bit arithmetic on edge cases and random
//! inputs, and run a guest that rustc built with `-C target-feature=+wide-arithmetic`: 256-bit
//! and 384-bit Montgomery multiplications whose limb arithmetic LLVM lowers to the instructions.
//! Every execution runs on the rwasm VM and on Wasmtime through [`for_each_strategy`] and both
//! must agree on results, memory and remaining fuel. The proposal's spec test runs in `e2e`.

#![cfg(feature = "wasmtime")]

#[path = "assets/wide-arithmetic-guest/src/mont.rs"]
mod mont;

use rwasm::{
    for_each_strategy, CompilationConfig, ImportLinker, Opcode, RwasmModule, StoreTr, TrapCode,
    Value,
};
use std::{ops::Range, sync::Arc};
use wasmparser::{Operator, Parser, Payload};

/// Built from `tests/assets/wide-arithmetic-guest` (see its README).
const GUEST: &[u8] = include_bytes!("assets/wide-arithmetic-guest.wasm");

/// Byte offsets of the guest's inputs and outputs, relative to the base it is called with; the
/// guest's `lib.rs` is the source of truth.
const A256: usize = 0;
const B256: usize = 32;
const A384: usize = 64;
const B384: usize = 112;
const R256: usize = 160;
const R384: usize = 192;
const WIDE: usize = 240;
const END: usize = 272;

fn config() -> CompilationConfig {
    CompilationConfig::default_strategy_compatible()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_import_linker(Arc::new(ImportLinker::default()))
}

/// Everything observable about one execution that the two engines must agree on.
#[derive(Debug, Clone, PartialEq)]
struct Outcome {
    trap: Option<TrapCode>,
    remaining_fuel: Option<u64>,
    result: Vec<Value>,
    memory: Vec<u8>,
}

/// Runs `main` on the rwasm VM and on Wasmtime and returns the outcome both agreed on.
fn run(
    wasm: &[u8],
    params: &[Value],
    results: &[Value],
    fuel_limit: u64,
    memory_init: &[(usize, Vec<u8>)],
    memory_window: Range<usize>,
) -> Outcome {
    let outcomes = for_each_strategy(
        |strategy| {
            let mut executor = strategy.create_executor(
                Arc::new(ImportLinker::default()),
                (),
                rwasm::always_failing_syscall_handler,
                Some(fuel_limit),
                None,
            )?;
            for (offset, bytes) in memory_init {
                executor.memory_write(*offset, bytes)?;
            }
            let mut result = results.to_vec();
            let trap = executor.execute("main", params, &mut result).err();
            let memory = if memory_window.is_empty() {
                Vec::new()
            } else {
                executor.snapshot_memory()?[memory_window.clone()].to_vec()
            };
            Ok(Outcome {
                trap,
                remaining_fuel: executor.remaining_fuel(),
                result,
                memory,
            })
        },
        config(),
        wasm,
    )
    .unwrap();
    assert_eq!(
        outcomes.len(),
        2,
        "expected the rwasm and wasmtime strategies"
    );
    let (rwasm, wasmtime) = (&outcomes[0], &outcomes[1]);
    assert_eq!(
        rwasm, wasmtime,
        "\nrwasm and wasmtime diverged:\n  rwasm    = {rwasm:?}\n  wasmtime = {wasmtime:?}\n"
    );
    assert_eq!(rwasm.trap, None);
    rwasm.clone()
}

fn i64_pair(value: u128) -> [Value; 2] {
    [Value::I64(value as i64), Value::I64((value >> 64) as i64)]
}

/// `xorshift64*`: deterministic inputs that cover every limb width.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

const EDGE: [u64; 9] = [
    0,
    1,
    2,
    u64::MAX,
    u64::MAX - 1,
    i64::MAX as u64,
    i64::MIN as u64,
    1 << 32,
    0xdead_beef_cafe_f00d,
];

const WIDE_OPERATORS: [(&str, Opcode); 4] = [
    ("i64.add128", Opcode::I64Add128),
    ("i64.sub128", Opcode::I64Sub128),
    ("i64.mul_wide_s", Opcode::I64MulWideS),
    ("i64.mul_wide_u", Opcode::I64MulWideU),
];

fn operator_module(operator: &str) -> Vec<u8> {
    let params = if operator.ends_with("128") {
        "(param i64 i64 i64 i64) (result i64 i64) local.get 0 local.get 1 local.get 2 local.get 3"
    } else {
        "(param i64 i64) (result i64 i64) local.get 0 local.get 1"
    };
    wat::parse_str(format!(
        r#"(module (memory (export "memory") 1) (func (export "main") {params} {operator}))"#
    ))
    .unwrap()
}

/// Each operator is one opcode, in place: no snippet call and no extra stack.
#[test]
fn every_operator_lowers_to_one_opcode() {
    for (operator, opcode) in WIDE_OPERATORS {
        let wasm = operator_module(operator);
        let (module, _) = RwasmModule::compile(config(), &wasm).unwrap();
        let code = module.code_section.iter().copied().collect::<Vec<_>>();
        assert_eq!(
            code.iter().filter(|op| **op == opcode).count(),
            1,
            "{operator} must lower to one {opcode:?}: {code:?}"
        );
        assert!(
            !code.iter().any(|op| matches!(op, Opcode::CallInternal(_))),
            "{operator} must not call a snippet: {code:?}"
        );
    }
}

/// Every operator agrees with native 128-bit arithmetic, on both backends, on edge cases and on
/// random inputs, and both backends charge the same fuel.
#[test]
fn operators_match_native_128_bit_arithmetic() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut inputs: Vec<[u64; 4]> = Vec::new();
    for &x in &EDGE {
        for &y in &EDGE {
            inputs.push([x, y, y, x]);
            inputs.push([x, x, y, y]);
        }
    }
    for _ in 0..64 {
        inputs.push([rng.next(), rng.next(), rng.next(), rng.next()]);
    }
    let modules = WIDE_OPERATORS.map(|(operator, _)| operator_module(operator));
    for [a_lo, a_hi, b_lo, b_hi] in inputs {
        let a = (a_lo as u128) | ((a_hi as u128) << 64);
        let b = (b_lo as u128) | ((b_hi as u128) << 64);
        let expected = [
            a.wrapping_add(b),
            a.wrapping_sub(b),
            ((a_lo as i64 as i128) * (b_lo as i64 as i128)) as u128,
            (a_lo as u128) * (b_lo as u128),
        ];
        for (i, (operator, _)) in WIDE_OPERATORS.iter().enumerate() {
            let params: Vec<Value> = if operator.ends_with("128") {
                vec![
                    Value::I64(a_lo as i64),
                    Value::I64(a_hi as i64),
                    Value::I64(b_lo as i64),
                    Value::I64(b_hi as i64),
                ]
            } else {
                vec![Value::I64(a_lo as i64), Value::I64(b_lo as i64)]
            };
            let outcome = run(&modules[i], &params, &i64_pair(0), 1_000, &[], 0..0);
            assert_eq!(
                outcome.result,
                i64_pair(expected[i]),
                "{operator} of {a:#x} and {b:#x} (lo limbs {a_lo:#x}, {b_lo:#x})"
            );
            assert!(outcome.remaining_fuel.unwrap() < 1_000);
        }
    }
}

/// The checked-in guest really carries the instructions: a rebuild without the target feature
/// would exercise the old lowering and pass the other tests anyway.
#[test]
fn rustc_built_guest_contains_the_instructions() {
    let mut counts = [0usize; 4];
    for payload in Parser::new(0).parse_all(GUEST) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for operator in body.get_operators_reader().unwrap() {
                match operator.unwrap() {
                    Operator::I64Add128 => counts[0] += 1,
                    Operator::I64Sub128 => counts[1] += 1,
                    Operator::I64MulWideS => counts[2] += 1,
                    Operator::I64MulWideU => counts[3] += 1,
                    _ => {}
                }
            }
        }
    }
    let [add128, _sub128, mul_wide_s, mul_wide_u] = counts;
    assert!(
        add128 > 0 && mul_wide_s > 0 && mul_wide_u > 0,
        "the guest was not built with `+wide-arithmetic`: {counts:?}"
    );
}

fn limbs_le<const N: usize>(bytes: &[u8]) -> [u64; N] {
    let mut limbs = [0u64; N];
    for (limb, chunk) in limbs.iter_mut().zip(bytes.chunks_exact(8)) {
        *limb = u64::from_le_bytes(chunk.try_into().unwrap());
    }
    limbs
}

fn bytes_le(limbs: &[u64]) -> Vec<u8> {
    limbs.iter().flat_map(|limb| limb.to_le_bytes()).collect()
}

/// A field element below the modulus: the top limb loses four bits, which is enough for both
/// moduli.
fn field_element<const N: usize>(rng: &mut Rng) -> [u64; N] {
    let mut limbs = [0u64; N];
    for limb in limbs.iter_mut() {
        *limb = rng.next();
    }
    limbs[N - 1] >>= 4;
    limbs
}

/// The rustc-built guest runs 256-bit and 384-bit Montgomery multiplications with the
/// instructions; both backends agree with each other and with the same code run natively.
#[test]
fn rustc_built_guest_agrees_with_native_code_on_both_backends() {
    let base = 4096usize;
    let mut rng = Rng(0x0123_4567_89ab_cdef);
    for round in 0..16 {
        let a256: [u64; 4] = field_element(&mut rng);
        let b256: [u64; 4] = field_element(&mut rng);
        let a384: [u64; 6] = field_element(&mut rng);
        let b384: [u64; 6] = field_element(&mut rng);
        let memory_init = vec![
            (base + A256, bytes_le(&a256)),
            (base + B256, bytes_le(&b256)),
            (base + A384, bytes_le(&a384)),
            (base + B384, bytes_le(&b384)),
        ];
        let outcome = run(
            GUEST,
            &[Value::I32(base as i32)],
            &[],
            1_000_000,
            &memory_init,
            base..base + END,
        );
        let memory = &outcome.memory;
        let r256: [u64; 4] = limbs_le(&memory[R256..R256 + 32]);
        let r384: [u64; 6] = limbs_le(&memory[R384..R384 + 48]);
        let wide: [u64; 4] = limbs_le(&memory[WIDE..WIDE + 32]);
        assert_eq!(
            r256,
            mont::mont_mul(&a256, &b256, &mont::P256, mont::P256_INV),
            "round {round}: 256-bit product"
        );
        assert_eq!(
            r384,
            mont::mont_mul(&a384, &b384, &mont::P384, mont::P384_INV),
            "round {round}: 384-bit product"
        );
        let unsigned = (a256[0] as u128) * (b256[0] as u128);
        let signed = (a256[0] as i64 as i128) * (b256[0] as i64 as i128);
        assert_eq!(
            wide,
            [
                unsigned as u64,
                (unsigned >> 64) as u64,
                signed as u64,
                (signed >> 64) as u64
            ],
            "round {round}: widening products"
        );
        // Montgomery multiplication is commutative: a cheap check that the guest computes a
        // product and not, say, an unreduced accumulator
        let swapped = run(
            GUEST,
            &[Value::I32(base as i32)],
            &[],
            1_000_000,
            &[
                (base + A256, bytes_le(&b256)),
                (base + B256, bytes_le(&a256)),
                (base + A384, bytes_le(&b384)),
                (base + B384, bytes_le(&a384)),
            ],
            base..base + END,
        );
        assert_eq!(swapped.memory[R256..R384 + 48], memory[R256..R384 + 48]);
        assert!(outcome.remaining_fuel.unwrap() < 1_000_000);
    }
}
