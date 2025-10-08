use criterion::{criterion_main, Criterion};
use hex_literal::hex;
use revm_bytecode::Bytecode;
use rwasm::{
    compile_wasmi_module, compile_wasmtime_module, wasmtime::deserialize_wasmtime_module,
    CompilationConfig, RwasmModule, RwasmModuleView,
};
use std::{sync::Arc, time::Duration};

const FIB_VALUE: i64 = 43;

fn bench_comparisons(c: &mut Criterion) {
    let mut group = c.benchmark_group("Comparisons Module Parsing");

    // bench_evm
    group.bench_function("bench_evm", |b| {
            let evm_bytecode = hex!("608060405234801561000f575f5ffd5b5060043610610029575f3560e01c8063e78692bb1461002d575b5f5ffd5b610047600480360381019061004291906100fd565b61005d565b6040516100549190610137565b60405180910390f35b5f5f5f90505f600190505f600290505b8467ffffffffffffffff168167ffffffffffffffff16116100b1575f8284610095919061017d565b90508293508092505080806100a9906101b8565b91505061006d565b508092505050919050565b5f5ffd5b5f67ffffffffffffffff82169050919050565b6100dc816100c0565b81146100e6575f5ffd5b50565b5f813590506100f7816100d3565b92915050565b5f60208284031215610112576101116100bc565b5b5f61011f848285016100e9565b91505092915050565b610131816100c0565b82525050565b5f60208201905061014a5f830184610128565b92915050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52601160045260245ffd5b5f610187826100c0565b9150610192836100c0565b9250828201905067ffffffffffffffff8111156101b2576101b1610150565b5b92915050565b5f6101c2826100c0565b915067ffffffffffffffff82036101dc576101db610150565b5b60018201905091905056fea2646970667358221220b9932107a06e2c6f884433417401d45c3d48c85efc8e1d3110c6fba210eb5abc64736f6c634300081e0033");
            let bytecode = Bytecode::new_raw(core::hint::black_box(evm_bytecode.into()));
            bytecode.original_bytes();
            b.iter(|| {
                core::hint::black_box(&bytecode);
            });
        });

    group.bench_function("bench_wasmtime", |b| {
        let wasm_binary = include_bytes!("../lib.wasm");
        let config = CompilationConfig::default().with_consume_fuel(false);
        let module = compile_wasmtime_module(config, core::hint::black_box(wasm_binary)).unwrap();
        let raw_module = module.serialize().unwrap();
        b.iter(|| {
            let module = deserialize_wasmtime_module(
                CompilationConfig::default(),
                core::hint::black_box(&raw_module),
            )
            .unwrap();
            core::hint::black_box(module);
        });
    });

    group.bench_function("bench_wasmi", |b| {
        let wasm_binary = include_bytes!("../lib.wasm");
        b.iter(|| {
            let config = CompilationConfig::default().with_consume_fuel(false);
            let module = compile_wasmi_module(config, core::hint::black_box(wasm_binary)).unwrap();
            core::hint::black_box(module);
        });
    });

    group.bench_function("bench_rwasm", |b| {
        let wasm_binary = include_bytes!("../lib.wasm");
        let config = CompilationConfig::default()
            .with_entrypoint_name("fib64".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_consume_fuel(false);
        let (module, _) = RwasmModule::compile(config, core::hint::black_box(wasm_binary)).unwrap();
        let raw_module = module.serialize();
        b.iter(|| {
            let (module, _) = RwasmModule::new(core::hint::black_box(&raw_module));
            core::hint::black_box(module);
        });
    });

    group.bench_function("bench_rwasm_view", |b| {
        let wasm_binary = include_bytes!("../lib.wasm");
        let config = CompilationConfig::default()
            .with_entrypoint_name("fib64".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_consume_fuel(false);
        let (module, _) = RwasmModule::compile(config, core::hint::black_box(wasm_binary)).unwrap();
        let raw_module = module.serialize();
        b.iter(|| {
            let (module, _) = RwasmModuleView::new(core::hint::black_box(&raw_module));
            core::hint::black_box(module);
        });
    });

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
