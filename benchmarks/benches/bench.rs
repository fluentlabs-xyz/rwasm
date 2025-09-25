use criterion::{criterion_main, Bencher, Criterion};
use rwasm::{
    always_failing_syscall_handler, compile_wasmi_module, compile_wasmtime_module,
    CompilationConfig, ExecutionEngine, ImportLinker, RwasmModule, RwasmStore, Strategy, Value,
};
use std::{sync::Arc, time::Duration};

const FIB_VALUE: i32 = 43;

fn bench_comparisons(c: &mut Criterion) {
    let mut group = c.benchmark_group("Comparisons");

    {
        group.bench_function("bench_strategy_rwasm", |b| {
            let wasm_binary = include_bytes!("../lib.wasm");
            let config = CompilationConfig::default()
                .with_entrypoint_name("main".into())
                .with_allow_malformed_entrypoint_func_type(true);
            let (module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
            let strategy = Strategy::Rwasm {
                module,
                engine: ExecutionEngine::acquire_shared(),
            };
            bench_strategy(b, strategy);
        });
    }

    {
        use wasmi::{Engine, Linker, Module, Store};
        let engine = Engine::default();
        let wasm_binary = include_bytes!("../lib.wasm");
        let module = Module::new(&engine, &wasm_binary[..]).unwrap();
        let mut store = Store::new(&engine, ());
        let linker = <Linker<()>>::new(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .unwrap()
            .start(&mut store)
            .unwrap();
        group.bench_function("bench_wasmi", |b| {
            b.iter(|| {
                let result = instance
                    .get_typed_func::<i32, i32>(&store, "main")
                    .unwrap()
                    .call(&mut store, FIB_VALUE)
                    .unwrap();
                core::hint::black_box(result);
            });
        });
    };

    // bench_native
    {
        group.bench_function("bench_native", |b| {
            b.iter(|| {
                pub fn main(n: i32) -> i32 {
                    let (mut a, mut b) = (0, 1);
                    for _ in 0..n {
                        let t = a;
                        a = b;
                        b = t + b;
                    }
                    a
                }
                core::hint::black_box(main(core::hint::black_box(FIB_VALUE)));
            });
        });
    };

    fn bench_strategy(b: &mut Bencher, strategy: Strategy) {
        b.iter(|| {
            let mut store = strategy.create_store(
                Arc::new(ImportLinker::default()),
                (),
                always_failing_syscall_handler,
            );
            let mut result = [Value::I32(0)];
            strategy
                .execute(
                    &mut store,
                    "main",
                    &[Value::I32(FIB_VALUE)],
                    &mut result,
                    Some(1_000_000),
                )
                .unwrap();
            core::hint::black_box(result);
        });
    }

    {
        group.bench_function("bench_strategy_wasmtime", |b| {
            let wasm_binary = include_bytes!("../lib.wasm");
            let strategy = Strategy::Wasmtime {
                module: compile_wasmtime_module(CompilationConfig::default(), wasm_binary).unwrap(),
            };
            bench_strategy(b, strategy);
        });
    }

    {
        group.bench_function("bench_strategy_wasmi", |b| {
            let wasm_binary = include_bytes!("../lib.wasm");
            let strategy = Strategy::Wasmi {
                module: compile_wasmi_module(CompilationConfig::default(), wasm_binary).unwrap(),
            };
            bench_strategy(b, strategy);
        });
    }

    {
        group.bench_function("bench_wasmi_no_cache", |b| {
            use wasmi::{Engine, Linker, Module, Store};
            let engine = Engine::default();
            let wasm = include_bytes!("../lib.wasm");
            b.iter(|| {
                let module = Module::new(&engine, &wasm[..]).unwrap();
                let mut store = Store::new(&engine, ());
                let linker = <Linker<()>>::new(&engine);
                let instance = linker
                    .instantiate(&mut store, &module)
                    .unwrap()
                    .start(&mut store)
                    .unwrap();
                let result = instance
                    .get_typed_func::<i32, i32>(&store, "main")
                    .unwrap()
                    .call(&mut store, FIB_VALUE)
                    .unwrap();
                core::hint::black_box(result);
            });
        });
    }

    {
        group.bench_function("bench_rwasm_no_cache", |b| {
            let wasm_binary = include_bytes!("../lib.wasm");

            let config = CompilationConfig::default()
                .with_entrypoint_name("main".into())
                .with_allow_malformed_entrypoint_func_type(true);
            let (rwasm_module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
            let encoded_rwasm_module = rwasm_module.serialize();
            let mut store = RwasmStore::<()>::default();
            let mut engine = ExecutionEngine::new();

            b.iter(|| {
                let (rwasm_module, _) = RwasmModule::new(&encoded_rwasm_module);
                let mut result = [Value::I32(0)];
                engine
                    .execute(
                        &mut store,
                        &rwasm_module,
                        &[Value::I32(FIB_VALUE)],
                        &mut result,
                        None,
                    )
                    .unwrap();
                core::hint::black_box(result);
                store.reset(true);
            });
        });
    }

    group.finish();
}

// criterion_group!(benches, erc20_transfer_benches);
pub fn benches() {
    let mut criterion: Criterion<_> = Criterion::default()
        .configure_from_args()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(1))
        .sample_size(1000);
    bench_comparisons(&mut criterion);
}
criterion_main!(benches);
