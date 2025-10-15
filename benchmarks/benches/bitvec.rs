use bitvec::{order::Lsb0, vec::BitVec};
use criterion::{criterion_main, Criterion};
use rwasm::{
    bitvec_inlined::{BitVecInlined, USIZE_BITS},
    CompilationConfig, ExecutionEngine, RwasmModule, RwasmStore, Value,
};
use std::time::Duration;

fn bench_comparisons(c: &mut Criterion) {
    let mut group = c.benchmark_group("Comparisons");

    const BITVEC_STORE_COUNT: usize = 1;
    const BITVEC_INLINED_STORE_COUNT: usize = BITVEC_STORE_COUNT;
    const BITVEC_INLINED_STORE_COUNT_HALF: usize = BITVEC_STORE_COUNT / 2;
    let bitvec_bits = USIZE_BITS * BITVEC_STORE_COUNT;
    let random_sets_count = 1000;
    let random_idxs_values =
        core::iter::repeat_with(|| (rand::random_range(..bitvec_bits), rand::random::<bool>()))
            .take(random_sets_count)
            .collect::<Vec<_>>();

    // bitvec
    {
        group.bench_function("bitvec", |b| {
            b.iter(|| {
                for i in 0..random_sets_count {
                    let mut bv = BitVec::<usize, Lsb0>::repeat(true, bitvec_bits);
                    let (idx, value) = random_idxs_values[i];
                    bv.set(idx, value);
                    core::hint::black_box(bv);
                }
            });
        });
    };

    // bitvec_inlined
    {
        group.bench_function("bitvec_inlined", |b| {
            b.iter(|| {
                for i in 0..random_sets_count {
                    let mut bv =
                        BitVecInlined::<{ BITVEC_INLINED_STORE_COUNT }>::repeat(true, bitvec_bits);
                    let (idx, value) = random_idxs_values[i];
                    bv.set(idx, value);
                    core::hint::black_box(bv);
                }
            });
        });
    };

    // bitvec_inlined (half store)
    {
        let mut bv =
            BitVecInlined::<{ BITVEC_INLINED_STORE_COUNT_HALF }>::repeat(true, bitvec_bits);
        group.bench_function("bitvec_inlined (half of inline store)", |b| {
            b.iter(|| {
                for i in 0..random_sets_count {
                    let (idx, value) = random_idxs_values[i];
                    bv.set(idx, value);
                }
            });
        });
    };

    {
        let wasm_binary = wat::parse_str(
            r#"
            (module
              (memory 1)
              (data (i32.const 0) "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzab")
              (func (export "64_good1") (param $i i32) (result i64)
                (i64.load offset=0 (local.get $i)) ;; 0x6867666564636261 'abcdefgh'
              )
            )
            "#,
        )
        .unwrap();
        let config = CompilationConfig::default()
            .with_entrypoint_name("64_good1".into())
            .with_allow_malformed_entrypoint_func_type(true);
        let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
        println!("{}", rwasm_module);
        let mut store = RwasmStore::<()>::default();
        let engine = ExecutionEngine::default();
        let mut result = [Value::I64(0); 1];
        group.bench_function("bitvec_inlined (through ExecutionEngine)", |b| {
            b.iter(|| {
                for _ in 0..random_sets_count {
                    engine
                        .execute(&mut store, &rwasm_module, &[Value::I32(0)], &mut result)
                        .unwrap();
                    assert_eq!(result[0].i64().unwrap(), 0x6867666564636261);
                }
            });
        });
    };

    group.finish();
}

pub fn benches() {
    let mut criterion: Criterion<_> = Criterion::default()
        .configure_from_args()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(1))
        .sample_size(200);
    bench_comparisons(&mut criterion);
}
criterion_main!(benches);
