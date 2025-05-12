extern crate test;

use rwasm::Caller;
use test::Bencher;

#[bench]
fn bench_wasmi(b: &mut Bencher) {
    use wasmi::{Engine, Linker, Module, Store};
    let wasm = include_bytes!("./lib.wasm");
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm[..]).unwrap();
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
            .call(&mut store, 43)
            .unwrap();
        assert_eq!(result, 433494437);
    });
}

#[bench]
fn bench_rwasm(b: &mut Bencher) {
    use rwasm::{
        legacy::{engine::RwasmConfig, Config, Engine, Module},
        ExecutorConfig,
        RwasmExecutor,
        RwasmModule,
    };
    use std::sync::Arc;

    let wasm = include_bytes!("./lib.wasm");

    let mut config = Config::default();
    config
        .wasm_mutable_global(false)
        .wasm_saturating_float_to_int(false)
        .wasm_sign_extension(false)
        .wasm_multi_value(false)
        .wasm_mutable_global(true)
        .wasm_saturating_float_to_int(true)
        .wasm_sign_extension(true)
        .wasm_multi_value(true)
        .wasm_bulk_memory(true)
        .wasm_reference_types(true)
        .wasm_tail_call(true)
        .wasm_extended_const(true);
    config.rwasm_config(RwasmConfig {
        state_router: None,
        entrypoint_name: Some("main".to_string()),
        import_linker: None,
        wrap_import_functions: true,
        translate_drop_keep: false,
        allow_malformed_entrypoint_func_type: true,
        use_32bit_mode: false,
        builtins_consume_fuel: false,
    });
    let engine = Engine::new(&config);
    let wasm_module = Module::new(&engine, &wasm[..]).unwrap();
    let rwasm_module = rwasm::legacy::rwasm::RwasmModule::from_module(&wasm_module);
    let mut encoded_rwasm_module = Vec::new();
    use rwasm::legacy::rwasm::BinaryFormat;
    rwasm_module
        .write_binary_to_vec(&mut encoded_rwasm_module)
        .unwrap();
    let rwasm_module = RwasmModule::new(&encoded_rwasm_module);
    let mut vm = RwasmExecutor::new(Arc::new(rwasm_module), ExecutorConfig::default(), ());

    b.iter(|| {
        Caller::new(&mut vm).stack_push(43);
        vm.run().unwrap();
        let result: i32 = Caller::new(&mut vm).stack_pop_as();
        assert_eq!(result, 433494437);
        vm.reset_pc();
    });
}
