extern crate test;

use rwasm::{Caller, CompilationConfig};
use test::Bencher;

#[bench]
fn bench_wasmi(b: &mut Bencher) {
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
            .call(&mut store, 43)
            .unwrap();
        core::hint::black_box(result);
        assert_eq!(result, 433494437);
    });
}

#[bench]
fn bench_rwasm(b: &mut Bencher) {
    use rwasm::{ExecutorConfig, RwasmExecutor, RwasmModule};
    use std::sync::Arc;

    let wasm = include_bytes!("./lib.wasm");

    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (rwasm_module, _) = RwasmModule::compile(config, wasm).unwrap();
    let encoded_rwasm_module = rwasm_module.serialize();
    let rwasm_module: Arc<RwasmModule> = RwasmModule::new(&encoded_rwasm_module).into();
    let mut vm = RwasmExecutor::new(rwasm_module, ExecutorConfig::default(), ());

    b.iter(|| {
        core::hint::black_box(RwasmModule::new(&encoded_rwasm_module));
        Caller::new(&mut vm).stack_push(43);
        vm.run().unwrap();
        let result: i32 = Caller::new(&mut vm).stack_pop_as();
        core::hint::black_box(result);
        assert_eq!(result, 433494437);
        vm.reset(None, false);
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
        core::hint::black_box(main(core::hint::black_box(43)));
    });
}
