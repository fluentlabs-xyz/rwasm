use criterion::{criterion_main, Bencher, Criterion};
use rwasm::{
    always_failing_syscall_handler, compile_wasmi_module, compile_wasmtime_module,
    CompilationConfig, ExecutionEngine, FuelConfig, ImportLinker, RwasmModule, Strategy, Value,
};
use std::{sync::Arc, time::Duration};

const FIB_VALUE: i32 = 43;

fn bench_comparisons(c: &mut Criterion) {
    let mut group = c.benchmark_group("Comparisons");

    // bench_native
    {
        pub fn fib(n: i32) -> i32 {
            let (mut a, mut b) = (0, 1);
            for _ in 0..n {
                let t = a;
                a = b;
                b = t + b;
            }
            a
        }
        group.bench_function("bench_native", |b| {
            b.iter(|| {
                core::hint::black_box(fib(core::hint::black_box(FIB_VALUE)));
            });
        });
    };

    fn bench_strategy(b: &mut Bencher, strategy: Strategy) {
        b.iter(|| {
            let mut store = strategy.create_store(
                Arc::new(ImportLinker::default()),
                (),
                always_failing_syscall_handler,
                FuelConfig::default().with_fuel_limit(1_000_000_000),
            );
            let mut result = [Value::I32(0)];
            strategy
                .execute(&mut store, "main", &[Value::I32(FIB_VALUE)], &mut result)
                .unwrap();
            core::hint::black_box(result);
        });
    }

    {
        let wasm_binary = include_bytes!("../lib.wasm");
        let config = CompilationConfig::default().with_consume_fuel(true);
        let module = compile_wasmtime_module(config, wasm_binary).unwrap();
        group.bench_function("bench_strategy_wasmtime", |b| {
            let strategy = Strategy::Wasmtime {
                module: module.clone(),
            };
            bench_strategy(b, strategy);
        });
    }

    {
        let wasm_binary = include_bytes!("../lib.wasm");
        let config = CompilationConfig::default().with_consume_fuel(true);
        let module = compile_wasmi_module(config, wasm_binary).unwrap();
        group.bench_function("bench_strategy_wasmi", |b| {
            let strategy = Strategy::Wasmi {
                module: module.clone(),
            };
            bench_strategy(b, strategy);
        });
    }

    {
        let wasm_binary = include_bytes!("../lib.wasm");
        let config = CompilationConfig::default()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_consume_fuel(false);
        let (module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
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
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(1))
        .sample_size(1000);
    bench_comparisons(&mut criterion);
}
criterion_main!(benches);
