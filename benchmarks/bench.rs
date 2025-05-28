extern crate test;

use rwasm::{CompilationConfig, ExecutionEngine, Store};
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
    use rwasm::{ExecutorConfig, RwasmModule};

    let wasm_binary = include_bytes!("./lib.wasm");

    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    let encoded_rwasm_module = rwasm_module.serialize();
    let mut store = Store::new(ExecutorConfig::default(), ());
    let mut engine = ExecutionEngine::new(&mut store);

    b.iter(|| {
        let rwasm_module = RwasmModule::new(&encoded_rwasm_module);
        engine.value_stack().push(FIB_VALUE.into());
        engine.execute(&rwasm_module).unwrap();
        let result = engine.value_stack().pop();
        core::hint::black_box(result);
        engine.store().reset(true);
    });
}

#[bench]
fn bench_rwasm(b: &mut Bencher) {
    use rwasm::{ExecutorConfig, RwasmModule};

    let wasm_binary = include_bytes!("./lib.wasm");

    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
    let encoded_rwasm_module = rwasm_module.serialize();
    let mut store = Store::new(ExecutorConfig::default(), ());
    let mut engine = ExecutionEngine::new(&mut store);
    let rwasm_module = RwasmModule::new(&encoded_rwasm_module);

    b.iter(|| {
        engine.value_stack().push(FIB_VALUE.into());
        engine.execute(&rwasm_module).unwrap();
        let result = engine.value_stack().pop();
        core::hint::black_box(result);
        engine.store().reset(true);
    });
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
