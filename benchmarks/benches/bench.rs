use bitvec::{order::Lsb0, vec::BitVec};
use criterion::{criterion_main, Bencher, Criterion};
use rwasm::{
    always_failing_syscall_handler,
    bitvec_inlined::{BitVecInlined, USIZE_BITS},
    compile_wasmi_module, compile_wasmtime_module, CompilationConfig, ExecutionEngine, FuelConfig,
    ImportLinker, RwasmModule, RwasmStore, Strategy, Value,
};
use std::{sync::Arc, time::Duration};

const FIB_VALUE: i32 = 43;

fn bench_comparisons(c: &mut Criterion) {
    let mut group = c.benchmark_group("Comparisons");

    const BITVEC_STORE_COUNT: usize = 1;
    const BITVEC_INLINED_STORE_COUNT: usize = BITVEC_STORE_COUNT;
    const BITVEC_INLINED_STORE_COUNT_HALF: usize = BITVEC_STORE_COUNT / 2;
    let bitvec_bits = USIZE_BITS * BITVEC_STORE_COUNT;
    let random_sets_count = 1;

    // // bitvec
    // {
    //     group.bench_function("bitvec", |b| {
    //         b.iter(|| {
    //             for _ in 0..random_sets_count {
    //                 let mut bv = BitVec::<usize, Lsb0>::repeat(true, bitvec_bits);
    //                 // let idx = rand::random_range(..bitvec_bits);
    //                 let idx = 8;
    //                 let value = rand::random();
    //                 bv.set(idx, value);
    //                 core::hint::black_box(bv);
    //             }
    //         });
    //     });
    // };
    //
    // // bitvec_inlined
    // {
    //     group.bench_function("bitvec_inlined", |b| {
    //         b.iter(|| {
    //             for _ in 0..random_sets_count {
    //                 let mut bv =
    //                     BitVecInlined::<{ BITVEC_INLINED_STORE_COUNT }>::repeat(true, bitvec_bits);
    //                 // let idx = rand::random_range(..bitvec_bits);
    //                 let idx = 8;
    //                 let value = rand::random();
    //                 bv.set(idx, value);
    //                 core::hint::black_box(bv);
    //             }
    //         });
    //     });
    // };
    //
    // // bitvec_inlined (half store)
    // {
    //     let mut bv =
    //         BitVecInlined::<{ BITVEC_INLINED_STORE_COUNT_HALF }>::repeat(true, bitvec_bits);
    //     group.bench_function("bitvec_inlined (half of inline store)", |b| {
    //         b.iter(|| {
    //             for _ in 0..random_sets_count {
    //                 let idx = rand::random_range(..bitvec_bits);
    //                 let value = rand::random();
    //                 bv.set(idx, value);
    //             }
    //         });
    //     });
    // };
    //
    // {
    //     let wasm_binary = wat::parse_str(
    //         r#"
    //         (module
    //           (memory 1)
    //           (data (i32.const 0) "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzab")
    //           (func (export "64_good1") (param $i i32) (result i64)
    //             (i64.load offset=0 (local.get $i)) ;; 0x6867666564636261 'abcdefgh'
    //           )
    //         )
    //         "#,
    //     )
    //     .unwrap();
    //     let config = CompilationConfig::default()
    //         .with_entrypoint_name("64_good1".into())
    //         .with_allow_malformed_entrypoint_func_type(true);
    //     let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
    //     println!("{}", rwasm_module);
    //     let mut store = RwasmStore::<()>::default();
    //     let engine = ExecutionEngine::new();
    //     let mut result = [Value::I64(0); 1];
    //     group.bench_function("bitvec_inlined (through ExecutionEngine)", |b| {
    //         b.iter(|| {
    //             for _ in 0..random_sets_count {
    //                 engine
    //                     .execute(&mut store, &rwasm_module, &[Value::I32(0)], &mut result)
    //                     .unwrap();
    //                 assert_eq!(result[0].i64().unwrap(), 0x6867666564636261);
    //             }
    //         });
    //     });
    // };
    //
    // // bench_native
    // {
    //     pub fn fib(n: i32) -> i32 {
    //         let (mut a, mut b) = (0, 1);
    //         for _ in 0..n {
    //             let t = a;
    //             a = b;
    //             b = t + b;
    //         }
    //         a
    //     }
    //     group.bench_function("bench_native", |b| {
    //         b.iter(|| {
    //             core::hint::black_box(fib(core::hint::black_box(FIB_VALUE)));
    //         });
    //     });
    // };
    //
    fn bench_strategy(b: &mut Bencher, strategy: Strategy) {
        b.iter(|| {
            let mut store = strategy.create_store(
                Arc::new(ImportLinker::default()),
                (),
                always_failing_syscall_handler,
                FuelConfig::default(),
            );
            let mut result = [Value::I32(0)];
            strategy
                .execute(&mut store, "main", &[Value::I32(FIB_VALUE)], &mut result)
                .unwrap();
            core::hint::black_box(result);
        });
    }

    // {
    //     let wasm_binary = include_bytes!("../lib.wasm");
    //     let config = CompilationConfig::default().with_consume_fuel(false);
    //     let module = compile_wasmtime_module(config, wasm_binary).unwrap();
    //     group.bench_function("bench_strategy_wasmtime", |b| {
    //         let strategy = Strategy::Wasmtime {
    //             module: module.clone(),
    //         };
    //         bench_strategy(b, strategy);
    //     });
    // }
    //
    // {
    //     let wasm_binary = include_bytes!("../lib.wasm");
    //     let config = CompilationConfig::default().with_consume_fuel(false);
    //     let module = compile_wasmi_module(config, wasm_binary).unwrap();
    //     group.bench_function("bench_strategy_wasmi", |b| {
    //         let strategy = Strategy::Wasmi {
    //             module: module.clone(),
    //         };
    //         bench_strategy(b, strategy);
    //     });
    // }

    {
        let wasm_binary = include_bytes!("../lib.wasm");
        let config = CompilationConfig::default()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_consume_fuel(false);
        let (module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
        println!("module = {}", module);
        group.bench_function("bench_strategy_rwasm", |b| {
            let strategy = Strategy::Rwasm {
                module: module.clone(),
                engine: ExecutionEngine::acquire_shared(),
            };
            bench_strategy(b, strategy);
        });
    }

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
