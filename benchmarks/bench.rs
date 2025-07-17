extern crate test;

use rwasm::{
    always_failing_syscall_handler,
    compile_wasmi_module,
    compile_wasmtime_module,
    CompilationConfig,
    ExecutionEngine,
    ExecutorConfig,
    ImportLinker,
    RwasmModule,
    RwasmStore,
    Strategy,
    Value,
};
use std::rc::Rc;
use test::Bencher;

const FIB_VALUE: i32 = 47;

#[bench]
fn bench_wasmi_no_cache(b: &mut Bencher) {
    use wasmi::{Engine, Linker, Module, Store};
    let engine = Engine::default();
    b.iter(|| {
        let wasm = include_bytes!("./lib.wasm");
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
}

#[bench]
fn bench_wasmi(b: &mut Bencher) {
    use wasmi::{Engine, Linker, Module, Store};
    let engine = Engine::default();
    let wasm_binary = include_bytes!("./lib.wasm");
    let module = Module::new(&engine, &wasm_binary[..]).unwrap();
    let mut store = Store::new(&engine, ());
    let linker = <Linker<()>>::new(&engine);
    let instance = linker
        .instantiate(&mut store, &module)
        .unwrap()
        .start(&mut store)
        .unwrap();
    b.iter(|| {
        let result = instance
            .get_typed_func::<i32, i32>(&store, "main")
            .unwrap()
            .call(&mut store, FIB_VALUE)
            .unwrap();
        core::hint::black_box(result);
    });
}

#[bench]
fn bench_rwasm_no_cache(b: &mut Bencher) {
    let wasm_binary = include_bytes!("./lib.wasm");

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
            )
            .unwrap();
        core::hint::black_box(result);
        store.reset(true);
    });
}

#[bench]
fn bench_rwasm(b: &mut Bencher) {
    let wasm_binary = include_bytes!("./lib.wasm");

    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    let encoded_rwasm_module = rwasm_module.serialize();
    let mut store = RwasmStore::<()>::default();
    let mut engine = ExecutionEngine::new();
    let (rwasm_module, _) = RwasmModule::new(&encoded_rwasm_module);

    b.iter(|| {
        let mut result = [Value::I32(0); 1];
        engine
            .execute(
                &mut store,
                &rwasm_module,
                &[Value::I32(FIB_VALUE)],
                &mut result,
            )
            .unwrap();
        core::hint::black_box(result);
        store.reset(true);
    });
}

fn bench_strategy(b: &mut Bencher, strategy: Strategy) {
    let mut store = strategy.create_store(
        ExecutorConfig::default().fuel_limit(1_000_000_000_000),
        Rc::new(ImportLinker::default()),
        (),
        always_failing_syscall_handler,
    );
    b.iter(|| {
        let mut result = [Value::I32(0)];
        strategy
            .execute(&mut store, "main", &[Value::I32(FIB_VALUE)], &mut result)
            .unwrap();
        core::hint::black_box(result);
    });
}

#[bench]
fn bench_strategy_wasmtime(b: &mut Bencher) {
    let wasm_binary = include_bytes!("./lib.wasm");
    let strategy = Strategy::Wasmtime {
        module: Rc::new(
            compile_wasmtime_module(CompilationConfig::default(), wasm_binary).unwrap(),
        ),
        resumable: false,
    };
    bench_strategy(b, strategy)
}

#[bench]
fn bench_strategy_wasmtime_resumable(b: &mut Bencher) {
    let wasm_binary = include_bytes!("./lib.wasm");
    let strategy = Strategy::Wasmtime {
        module: Rc::new(
            compile_wasmtime_module(CompilationConfig::default(), wasm_binary).unwrap(),
        ),
        resumable: true,
    };
    bench_strategy(b, strategy)
}

#[bench]
fn bench_strategy_wasmi(b: &mut Bencher) {
    let wasm_binary = include_bytes!("./lib.wasm");
    let strategy = Strategy::Wasmi {
        module: Rc::new(compile_wasmi_module(CompilationConfig::default(), wasm_binary).unwrap()),
    };
    bench_strategy(b, strategy)
}

#[bench]
fn bench_native(b: &mut Bencher) {
    b.iter(|| {
        pub fn main(n: i32) -> i32 {
            let (mut a, mut b) = (0, 1);
            for _ in 0..n {
                let temp = a;
                a = b;
                b = temp + b;
            }
            a
        }
        core::hint::black_box(main(core::hint::black_box(FIB_VALUE)));
    });
}
