use criterion::{criterion_main, Bencher, Criterion};
use hex_literal::hex;
use revm_bytecode::Bytecode;
use revm_interpreter::{
    host::DummyHost,
    instruction_table,
    interpreter::{EthInterpreter, ExtBytecode},
    CallInput, InputsImpl, Interpreter, SharedMemory,
};
use rwasm::{
    always_failing_syscall_handler, compile_wasmi_module, compile_wasmtime_module,
    CompilationConfig, ExecutionEngine, FuelConfig, ImportLinker, RwasmModule, Strategy, Value,
};
use std::{sync::Arc, time::Duration};

const FIB_VALUE: i64 = 43;

fn bench_comparisons(c: &mut Criterion) {
    let mut group = c.benchmark_group("Comparisons fib64");

    // bench_native
    {
        pub fn fib64(n: u64) -> u64 {
            let (mut a, mut b) = (0, 1);
            for _ in 0..n {
                let temp = a;
                a = b;
                b = temp + b;
            }
            a
        }
        group.bench_function("bench_native", |b| {
            b.iter(|| {
                core::hint::black_box(fib64(core::hint::black_box(FIB_VALUE as u64)));
            });
        });
    };

    // bench_evm
    {
        let evm_bytecode = hex!("608060405234801561000f575f5ffd5b5060043610610029575f3560e01c8063e78692bb1461002d575b5f5ffd5b610047600480360381019061004291906100fd565b61005d565b6040516100549190610137565b60405180910390f35b5f5f5f90505f600190505f600290505b8467ffffffffffffffff168167ffffffffffffffff16116100b1575f8284610095919061017d565b90508293508092505080806100a9906101b8565b91505061006d565b508092505050919050565b5f5ffd5b5f67ffffffffffffffff82169050919050565b6100dc816100c0565b81146100e6575f5ffd5b50565b5f813590506100f7816100d3565b92915050565b5f60208284031215610112576101116100bc565b5b5f61011f848285016100e9565b91505092915050565b610131816100c0565b82525050565b5f60208201905061014a5f830184610128565b92915050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52601160045260245ffd5b5f610187826100c0565b9150610192836100c0565b9250828201905067ffffffffffffffff8111156101b2576101b1610150565b5b92915050565b5f6101c2826100c0565b915067ffffffffffffffff82036101dc576101db610150565b5b60018201905091905056fea2646970667358221220b9932107a06e2c6f884433417401d45c3d48c85efc8e1d3110c6fba210eb5abc64736f6c634300081e0033");
        group.bench_function("bench_evm", |b| {
            let bytecode = Bytecode::new_raw(evm_bytecode.into());
            let instruction_table = instruction_table::<EthInterpreter, DummyHost>();
            b.iter(|| {
                let mut interpreter = Interpreter::new(
                    SharedMemory::new(),
                    ExtBytecode::new_with_hash(bytecode.clone(), [1u8; 32].into()),
                    InputsImpl {
                        target_address: Default::default(),
                        bytecode_address: None,
                        caller_address: Default::default(),
                        input: CallInput::Bytes(hex!("e78692bb000000000000000000000000000000000000000000000000000000000000002b").into()),
                        call_value: Default::default(),
                    },
                    true,
                    Default::default(),
                    100_000_000,
                );
                let result = interpreter.run_plain::<DummyHost>(&instruction_table, &mut DummyHost {});
                // match &result {
                //     InterpreterAction::NewFrame(_) => unreachable!(),
                //     InterpreterAction::Return(result) => {
                //         if !result.is_ok() {
                //             println!("{:?}", result);
                //         }
                //         assert!(result.is_ok());
                //         assert_eq!(result.output.len(), 32);
                //         assert_eq!(result.output.as_ref(), hex!("00000000000000000000000000000000000000000000000027f80ddaa1ba7878"));
                //     }
                // }
                core::hint::black_box(result);
            });
        });
    };

    fn bench_strategy(b: &mut Bencher, strategy: Strategy) {
        // b.iter_batched(
        //     || {
        //         strategy.create_store(
        //             Arc::new(ImportLinker::default()),
        //             (),
        //             always_failing_syscall_handler,
        //             FuelConfig::default(),
        //         )
        //     },
        //     |mut store| {
        b.iter(
            || {
                let mut store = strategy.create_store(
                    Arc::new(ImportLinker::default()),
                    (),
                    always_failing_syscall_handler,
                    FuelConfig::default(),
                );
                let mut result = [Value::I64(0)];
                strategy
                    .execute(&mut store, "fib64", &[Value::I64(FIB_VALUE)], &mut result)
                    .unwrap();
                core::hint::black_box(result);
            },
            // BatchSize::SmallInput,
        );
    }

    {
        let wasm_binary = include_bytes!("../lib.wasm");
        let config = CompilationConfig::default().with_consume_fuel(false);
        let module = compile_wasmtime_module(config, wasm_binary).unwrap();
        group.bench_function("bench_wasmtime", |b| {
            let strategy = Strategy::Wasmtime {
                module: module.clone(),
            };
            bench_strategy(b, strategy);
        });
    }

    {
        let wasm_binary = include_bytes!("../lib.wasm");
        let config = CompilationConfig::default().with_consume_fuel(false);
        let module = compile_wasmi_module(config, wasm_binary).unwrap();
        group.bench_function("bench_wasmi", |b| {
            let strategy = Strategy::Wasmi {
                module: module.clone(),
            };
            bench_strategy(b, strategy);
        });
    }

    {
        let wasm_binary = include_bytes!("../lib.wasm");
        let config = CompilationConfig::default()
            .with_entrypoint_name("fib64".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_consume_fuel(false);
        let (module, _) = RwasmModule::compile(config, wasm_binary).unwrap();
        group.bench_function("bench_rwasm", |b| {
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
