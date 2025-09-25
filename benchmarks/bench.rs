extern crate test;

use rwasm::{
    always_failing_syscall_handler, compile_wasmi_module, compile_wasmtime_module,
    CompilationConfig, ExecutionEngine, ImportLinker, RwasmModule, Strategy, Value,
};
use std::sync::Arc;
use test::Bencher;

const FIB_VALUE: i32 = 43;

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

#[bench]
fn bench_strategy_rwasm(b: &mut Bencher) {
    let wasm_binary = include_bytes!("./lib.wasm");
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    let strategy = Strategy::Rwasm {
        module,
        engine: ExecutionEngine::acquire_shared(),
    };
    bench_strategy(b, strategy)
}

#[bench]
fn bench_strategy_wasmtime(b: &mut Bencher) {
    let wasm_binary = include_bytes!("./lib.wasm");
    let strategy = Strategy::Wasmtime {
        module: compile_wasmtime_module(CompilationConfig::default(), wasm_binary).unwrap(),
    };
    bench_strategy(b, strategy)
}

#[bench]
fn bench_strategy_wasmi(b: &mut Bencher) {
    let wasm_binary = include_bytes!("./lib.wasm");
    let strategy = Strategy::Wasmi {
        module: compile_wasmi_module(CompilationConfig::default(), wasm_binary).unwrap(),
    };
    bench_strategy(b, strategy)
}

#[bench]
fn bench_native(b: &mut Bencher) {
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
}
