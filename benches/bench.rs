use criterion::{criterion_main, Bencher, Criterion};
use fib_example::FIB_WASM;
use rwasm::{
    always_failing_syscall_handler, wasmtime::compile_wasmtime_module, CompilationConfig,
    ExecutionEngine, ImportLinker, RwasmModule, StrategyDefinition, StrategyExecutor, Value,
};
use std::{sync::Arc, time::Duration};

const FIB_VALUE: i32 = 43;
const FIB_RESULT: i32 = 433494437;

fn create_executor(strategy: &StrategyDefinition) -> StrategyExecutor<()> {
    strategy
        .create_executor(
            Arc::new(ImportLinker::default()),
            (),
            always_failing_syscall_handler,
            // The warm benchmarks never reset fuel, so a finite limit would be exhausted partway
            // through a measurement and turn into an `OutOfFuel` panic.
            Some(u64::MAX),
            None,
        )
        .unwrap()
}

fn run(executor: &mut StrategyExecutor<()>) {
    let mut result = [Value::I32(0)];
    executor
        .execute("main", &[Value::I32(FIB_VALUE)], &mut result)
        .unwrap();
    // A warm executor carries guest state between runs, so check the result rather than only the
    // absence of a trap: silent drift would otherwise look like a faster benchmark.
    assert_eq!(result[0].i32(), Some(FIB_RESULT));
}

/// Execution against an already-instantiated module: the cached-contract path.
///
/// Only sound because `fib` is pure integer arithmetic that leaves no guest state behind. See
/// `benches/README.md` for why this does not generalize.
fn bench_warm(b: &mut Bencher, strategy: StrategyDefinition) {
    let mut executor = create_executor(&strategy);
    b.iter(|| run(&mut executor));
}

/// A fresh store and instance per iteration: the state-trie path, and the one that matches how a
/// blockchain execution environment actually calls a contract.
fn bench_cold(b: &mut Bencher, strategy: StrategyDefinition) {
    b.iter(|| {
        let mut executor = create_executor(&strategy);
        run(&mut executor);
    });
}

fn bench_comparisons(c: &mut Criterion) {
    let mut group = c.benchmark_group("Comparisons");

    {
        pub fn fib(n: i32) -> i32 {
            let (mut a, mut b) = (0, 1);
            for _ in 0..n {
                let t = a;
                a = b;
                b += t;
            }
            a
        }
        group.bench_function("bench_native", |b| {
            b.iter(|| {
                core::hint::black_box(fib(core::hint::black_box(FIB_VALUE)));
            });
        });
    };

    {
        // Both backends meter fuel, otherwise the comparison rewards rwasm for doing less work.
        let config = CompilationConfig::default().with_consume_fuel(true);
        let module = compile_wasmtime_module(config, FIB_WASM).unwrap();
        let strategy = StrategyDefinition::Wasmtime { module };
        group.bench_function("bench_wasmtime_warm", |b| {
            bench_warm(b, strategy.clone());
        });
        group.bench_function("bench_wasmtime_cold", |b| {
            bench_cold(b, strategy.clone());
        });
    }

    {
        let config = CompilationConfig::default()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_consume_fuel(true);
        let (module, _) = RwasmModule::compile(config, FIB_WASM).unwrap();
        let strategy = StrategyDefinition::Rwasm {
            module,
            engine: ExecutionEngine::acquire_shared(),
        };
        group.bench_function("bench_rwasm_warm", |b| {
            bench_warm(b, strategy.clone());
        });
        group.bench_function("bench_rwasm_cold", |b| {
            bench_cold(b, strategy.clone());
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
